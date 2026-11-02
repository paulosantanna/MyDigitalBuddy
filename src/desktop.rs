// SPDX-License-Identifier: GPL-3.0-or-later
mod drawing;

use crate::{
    core::physics::Rect as WorldRect,
    engine::{Engine, ThingKind},
    platform::{self, Snapshot},
    storage,
};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2, ViewportBuilder,
    ViewportCommand, ViewportId, WindowLevel,
};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const ACCENT: Color32 = Color32::from_rgb(94, 234, 212);
pub fn run(
    engine: Engine,
    direct: bool,
    start_in_tray: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("MyDigitalBuddy")
            .with_inner_size([550.0, 740.0])
            .with_min_inner_size([470.0, 560.0])
            .with_icon(app_icon()),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "MyDigitalBuddy",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            cc.egui_ctx.all_styles_mut(|style| {
                style.spacing.item_spacing = Vec2::new(10.0, 9.0);
                style.visuals.selection.bg_fill = Color32::from_rgb(21, 94, 87);
                style.visuals.hyperlink_color = ACCENT;
            });
            Ok(Box::new(Desktop::new(engine, direct, start_in_tray)))
        }),
    )
    .map_err(|e| e.to_string().into())
}

struct SensorWorker {
    snapshot: Arc<Mutex<Snapshot>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}
impl SensorWorker {
    fn new() -> Self {
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let output = Arc::clone(&snapshot);
        let halt = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !halt.load(Ordering::Relaxed) {
                let value = platform::sample();
                if let Ok(mut slot) = output.lock() {
                    *slot = value;
                }
                thread::park_timeout(Duration::from_millis(100));
            }
        });
        Self {
            snapshot,
            stop,
            handle: Some(handle),
        }
    }
}
impl Drop for SensorWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            h.thread().unpark();
            let _ = h.join();
        }
    }
}

struct Desktop {
    engine: Engine,
    worker: SensorWorker,
    input: Snapshot,
    last: Instant,
    last_save: Instant,
    tab: u8,
    onboarding: bool,
    page: usize,
    initialized: bool,
    show_monitor: bool,
    show_protest: bool,
    reset_confirm: bool,
    error: Option<String>,
    drag_last: Option<(f64, f64)>,
    drag_velocity: (f64, f64),
    menu_open: bool,
    rub: VecDeque<(f64, f64)>,
    textures: BTreeMap<PathBuf, egui::TextureHandle>,
    tray: Option<platform::Tray>,
    activation: Option<platform::Activation>,
    quit: bool,
    in_tray: bool,
    restore_until: Option<Instant>,
    start_in_tray: bool,
}
impl Desktop {
    fn new(mut engine: Engine, direct: bool, start_in_tray: bool) -> Self {
        let onboarding = !engine.settings.onboarding_done && !direct;
        if direct {
            engine.settings.onboarding_done = true;
        }
        let mut error = None;
        #[cfg(target_os = "windows")]
        let tray = match platform::Tray::install() {
            Ok(tray) => Some(tray),
            Err(message) => {
                error = Some(message);
                None
            }
        };
        #[cfg(not(target_os = "windows"))]
        let tray = None;
        let activation = platform::Activation::listen(&engine.root);
        if engine.settings.launch_at_startup
            && platform::supports_launch_at_startup()
            && let Err(message) = platform::set_launch_at_startup(true)
        {
            error = Some(message);
        }
        Self {
            engine,
            worker: SensorWorker::new(),
            input: Snapshot::default(),
            last: Instant::now(),
            last_save: Instant::now(),
            tab: 0,
            onboarding,
            page: 0,
            initialized: false,
            show_monitor: false,
            show_protest: false,
            reset_confirm: false,
            error,
            drag_last: None,
            drag_velocity: (0.0, 0.0),
            menu_open: false,
            rub: VecDeque::new(),
            textures: BTreeMap::new(),
            tray,
            activation,
            quit: false,
            in_tray: false,
            restore_until: None,
            start_in_tray,
        }
    }
    fn save(&mut self) {
        if let Err(e) = self.engine.save() {
            self.error = Some(format!("Não foi possível salvar: {e}"));
        }
    }
    fn set_startup(&mut self, enabled: bool) {
        match platform::set_launch_at_startup(enabled) {
            Ok(()) => {
                self.engine.settings.launch_at_startup = enabled;
                self.save();
            }
            Err(message) => self.error = Some(message),
        }
    }
    fn startup_switch(&mut self, ui: &mut egui::Ui) {
        if !platform::supports_launch_at_startup() {
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Iniciar com o Windows");
            let on = self.engine.settings.launch_at_startup;
            if ui.selectable_label(on, "On").clicked() && !on {
                self.set_startup(true);
            }
            if ui.selectable_label(!on, "Off").clicked() && on {
                self.set_startup(false);
            }
        });
        ui.label(
            egui::RichText::new(
                "On abre o companheiro na bandeja quando o Windows entra, sem mostrar a central.",
            )
            .small()
            .color(Color32::GRAY),
        );
    }
    fn restore_central(&mut self, ctx: &egui::Context) {
        self.in_tray = false;
        self.start_in_tray = false;
        self.restore_until = Some(Instant::now() + Duration::from_millis(600));
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Visible(true));
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus);
    }
    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        if self.tray.is_none() {
            return;
        }
        self.in_tray = true;
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
    }
    fn poll_tray(&mut self, ctx: &egui::Context) {
        let Some(tray) = self.tray.as_mut() else {
            return;
        };
        tray.set_flags(self.engine.paused, self.engine.hidden);
        let commands = tray.poll();
        for command in commands {
            match command {
                platform::TrayCommand::Open => self.restore_central(ctx),
                platform::TrayCommand::TogglePause => self.engine.paused = !self.engine.paused,
                platform::TrayCommand::ToggleHidden => self.engine.hidden = !self.engine.hidden,
                platform::TrayCommand::Panic => self.engine.panic(),
                platform::TrayCommand::Quit => {
                    self.quit = true;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }
    }
    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("●").color(ACCENT).size(28.0));
            ui.vertical(|ui| {
                ui.heading("MyDigitalBuddy");
                ui.label(
                    egui::RichText::new("Um pequeno cérebro. Uma grande companhia.")
                        .color(Color32::GRAY),
                );
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            for (id, label) in [
                (0, "Companheiro"),
                (1, "Permissões"),
                (2, "Cérebro"),
                (3, "Memória"),
            ] {
                if ui.selectable_label(self.tab == id, label).clicked() {
                    self.tab = id;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                match self.tab {
                    0 => self.companion(ui),
                    1 => self.permissions(ui),
                    2 => self.neural(ui),
                    _ => self.memory(ui),
                }
                if let Some(error) = &self.error {
                    ui.add_space(10.0);
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                for warning in &self.engine.warnings {
                    ui.colored_label(Color32::YELLOW, warning);
                }
                ui.add_space(15.0);
                ui.separator();
                ui.label(
                    egui::RichText::new("Tudo acontece neste computador. Nenhum envio à nuvem.")
                        .small()
                        .color(Color32::GRAY),
                );
                ui.label(
                    egui::RichText::new(&self.input.status)
                        .small()
                        .color(Color32::GRAY),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.hyperlink_to("GPL v3+", "https://www.gnu.org/licenses/gpl-3.0.html");
                    ui.label("· v0.1.0");
                });
            });
    }
    fn companion(&mut self, ui: &mut egui::Ui) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 170.0), Sense::hover());
        drawing::buddy(
            ui.painter(),
            rect.center() + Vec2::new(0.0, -4.0),
            0.0,
            1.2,
            &self.engine,
        );
        ui.vertical_centered(|ui| {
            ui.heading(format!(
                "{} · {}",
                self.engine.pet.name,
                self.engine.emotion.current.name()
            ));
            ui.label(format!(
                "{} neurônios · {} sinapses",
                self.engine.brain.n,
                self.engine.brain.synapses()
            ));
        });
        ui.add_space(10.0);
        let old = self.engine.settings.pet.clone();
        let mut chosen = old.clone();
        ui.horizontal(|ui| {
            ui.label("Personagem");
            egui::ComboBox::from_id_salt("pet-choice")
                .selected_text(&self.engine.pet.name)
                .show_ui(ui, |ui| {
                    for id in self.engine.available_pets() {
                        let label = self.engine.pet_name(&id);
                        ui.selectable_value(&mut chosen, id, label);
                    }
                });
        });
        if chosen != old
            && let Err(e) = self.engine.switch_pet(&chosen)
        {
            self.error = Some(e.to_string());
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if self.engine.settings.active {
                    "Desativar"
                } else {
                    "Ativar companheiro"
                })
                .clicked()
            {
                self.engine.settings.active = !self.engine.settings.active;
                self.save();
            }
            if ui
                .button(if self.engine.paused {
                    "Continuar"
                } else {
                    "Pausar"
                })
                .clicked()
            {
                self.engine.paused = !self.engine.paused;
            }
            if ui
                .button(if self.engine.hidden {
                    "Mostrar"
                } else {
                    "Esconder"
                })
                .clicked()
            {
                self.engine.hidden = !self.engine.hidden;
            }
            if ui.button("Limpar objetos").clicked() {
                self.engine.clear_things();
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Dar carinho").clicked() {
                self.engine.caress(2.0);
            }
            if ui.button("Cutucar").clicked() {
                self.engine.poke();
            }
            if ui.button("Trabalhar juntos").clicked() {
                self.engine.desk.request(self.engine.time);
            }
            if ui
                .button(if self.engine.asleep {
                    "Acordar"
                } else {
                    "Dormir"
                })
                .clicked()
            {
                if self.engine.asleep {
                    self.engine.wake(false);
                } else {
                    self.engine.sleep();
                }
            }
        });
        ui.add_space(8.0);
        bar(ui, "Energia", self.engine.drives.energy, ACCENT);
        bar(
            ui,
            "Estímulo",
            self.engine.drives.stimulation,
            Color32::from_rgb(167, 139, 250),
        );
        bar(
            ui,
            "Vontade de companhia",
            self.engine.drives.contact,
            Color32::from_rgb(251, 191, 36),
        );
        ui.add_space(8.0);
        ui.label("Clique: cutucar · Arraste: carregar e arremessar · Rodinha: girar\nPasse o cursor de um lado para outro: carinho · Botão direito: menu");
        ui.add_space(6.0);
        if ui
            .add(
                egui::Button::new("PÂNICO · Ctrl + Alt + Shift + B")
                    .fill(Color32::from_rgb(122, 35, 45)),
            )
            .clicked()
        {
            self.engine.panic();
        }
        if !self.engine.settings.active {
            ui.colored_label(
                Color32::YELLOW,
                "Desativado. Ele continuará assim até você reativar.",
            );
        }
        ui.label(
            egui::RichText::new(
                "Minimizar ou fechar esta janela manda a central para a bandeja. O companheiro continua no desktop. Sair, no ícone da bandeja ou no menu do pet, encerra o programa.",
            )
            .small(),
        );
        ui.add_space(6.0);
        self.startup_switch(ui);
    }
    fn permissions(&mut self, ui: &mut egui::Ui) {
        ui.heading("Você escolhe o que ele faz");
        ui.label("As opções são salvas ao mudar.");
        ui.add_space(8.0);
        let before = serde_json::to_string(&self.engine.settings).unwrap_or_default();
        let s = &mut self.engine.settings;
        ui.collapsing("Travessuras", |ui| {
            ui.checkbox(&mut s.mischief, "Permitir travessuras");
            ui.add_enabled_ui(s.mischief, |ui| {
                ui.checkbox(&mut s.notes, "Deixar bilhetes");
                ui.checkbox(&mut s.memes, "Trazer imagens de pets/<id>/memes/<emoção>");
                ui.checkbox(&mut s.photos, "Trazer minhas fotos de assets/comum/fotos");
            });
        });
        ui.collapsing("Interação", |ui| {
            ui.checkbox(&mut s.poke, "Cutucar");
            ui.checkbox(&mut s.caress, "Carinho e aprendizado");
            ui.checkbox(&mut s.drag, "Arrastar e arremessar");
            ui.checkbox(&mut s.perch, "Pousar nas janelas");
            ui.checkbox(&mut s.ball, "Brincar com a bolinha");
            ui.checkbox(&mut s.fireworks, "Fogos de artifício");
        });
        ui.collapsing("Contexto e rotina",|ui| {ui.checkbox(&mut s.show_fullscreen,"Aparecer em tela cheia (experimental)");ui.checkbox(&mut s.routine,"Aprender minha rotina (categorias por minuto)");ui.label("Somente nome do processo, tempo ocioso, cursor e geometria.\nTítulos de janelas, documentos e conversas não são lidos.");});
        ui.collapsing("Desenvolvimento", |ui| {
            ui.checkbox(&mut s.test_mode, "Modo teste: tempos acelerados");
            ui.checkbox(&mut s.show_emotion, "Mostrar emoção abaixo do pet");
        });
        if before != serde_json::to_string(&self.engine.settings).unwrap_or_default() {
            if !self.engine.settings.routine {
                self.engine.timeline = crate::timeline::Timeline::default();
            }
            if !self.engine.settings.drag {
                self.engine.body.held = false;
                self.engine.senses.dragging = false;
            }
            self.save();
        }
        ui.add_space(14.0);
        if ui
            .checkbox(&mut self.engine.desk.visible, "Mostrar notebook")
            .changed()
            && !self.engine.desk.visible
        {
            self.engine.desk.seated = false;
        }
        if ui
            .checkbox(
                &mut self.engine.corner.enabled,
                "Mostrar cantinho do castigo",
            )
            .changed()
            && !self.engine.corner.enabled
        {
            self.engine.corner.release();
            self.engine.corner.escape = None;
        }
        ui.label("Solte o pet diante da placa para entrar no cantinho.\nCarinho acalma; frustração pode fazê-lo fugir.");
        ui.add_space(10.0);
        ui.label("Conteúdo local");
        ui.monospace(self.engine.content_root.display().to_string());
        ui.label("Os desenhos são feitos em Rust. Você pode adicionar imagens próprias nas pastas de conteúdo.");
        ui.add_space(12.0);
        ui.heading("Sistema");
        self.startup_switch(ui);
    }
    fn neural(&mut self, ui: &mut egui::Ui) {
        ui.heading("Rede neural pulsada");
        ui.label("Integração e disparo com vazamento (LIF), habituação e sinapses plásticas.");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_monitor, "Monitor flutuante");
            ui.checkbox(&mut self.show_protest, "Painel de protesto");
        });
        ui.label(format!(
            "Valência {:+.3}   Ativação {:+.3}",
            self.engine.emotion.valence, self.engine.emotion.arousal
        ));
        egui::Grid::new("motor-rates")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Motor");
                ui.strong("Hz / neurônio");
                ui.strong("Atividade");
                ui.end_row();
                for m in crate::core::topology::MOTORS {
                    let hz = self.engine.brain.hz(m.0);
                    ui.monospace(m.0);
                    ui.label(format!("{hz:.2}"));
                    ui.add(
                        egui::ProgressBar::new((hz / 20.0).min(1.0) as f32)
                            .desired_width(150.0)
                            .fill(ACCENT),
                    );
                    ui.end_row();
                }
            });
        ui.add_space(12.0);
        ui.collapsing("Sentidos e habituação", |ui| {
            egui::Grid::new("sensor-rates")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Canal");
                    ui.strong("Entrada");
                    ui.strong("Eficácia");
                    ui.end_row();
                    for s in crate::core::topology::SENSORS {
                        ui.monospace(s.0);
                        ui.label(format!(
                            "{:.2}",
                            self.engine.intensities.get(s.0).copied().unwrap_or(0.0)
                        ));
                        ui.label(format!("{:.1}%", self.engine.brain.efficacy(s.0) * 100.0));
                        ui.end_row();
                    }
                });
        });
        ui.add_space(8.0);
        ui.label("A rede é uma simulação pequena, com topologia desenhada à mão. Não é uma LLM e não precisa de GPU.");
    }
    fn memory(&mut self, ui: &mut egui::Ui) {
        ui.heading("Memória local");
        ui.monospace(self.engine.root.display().to_string());
        ui.label(format!(
            "{} episódios recentes · Memória separada por personagem",
            self.engine.memory.recent.len()
        ));
        ui.horizontal(|ui| {
            if ui.button("Salvar agora").clicked() {
                self.save();
            }
            if ui.button("Consolidar preferências").clicked() {
                self.engine.memory.consolidate();
                self.save();
            }
            if ui.button("Apagar histórico de rotina").clicked()
                && let Err(e) = self
                    .engine
                    .timeline
                    .discard(&self.engine.root.join("timeline.csv"))
            {
                self.error = Some(e.to_string());
            }
        });
        ui.add_space(8.0);
        ui.collapsing("Condicionamento", |ui| {
            for (cue, value) in &self.engine.conditioning.values {
                bar(ui, cue, *value, ACCENT);
            }
        });
        ui.collapsing("Episódios recentes", |ui| {
            for ep in self.engine.memory.recent.iter().rev().take(30) {
                ui.label(format!(
                    "{} → {}: {} ({:+.2})",
                    ep.context, ep.action, ep.result, ep.valence
                ));
            }
        });
        ui.add_space(12.0);
        if ui.button("Redefinir memória deste personagem…").clicked() {
            self.reset_confirm = true;
        }
        ui.add_space(10.0);
        ui.label("Importar uma instalação Python: encerre o aplicativo e execute:");
        ui.monospace("mydigitalbuddy --data-dir NOVA_PASTA --import-legacy CAMINHO");
        ui.label("Os originais são preservados. A importação exige destino sem memória existente.");
    }
    fn onboarding(&mut self, ui: &mut egui::Ui) {
        const PAGES: &[(&str, &str)] = &[
            (
                "Olá, sou seu Digital Buddy.",
                "Um companheiro que vive no seu desktop. Tem um pequeno cérebro neural, personalidade, emoções e memória local.",
            ),
            (
                "Você pode brincar comigo.",
                "Cutucar, carregar, arremessar e fazer carinho. Minha bolinha, meu notebook e o cantinho são objetos que você também pode arrastar.",
            ),
            (
                "Seu computador, sua privacidade.",
                "Leio apenas nome do programa em primeiro plano, tempo ocioso, posição do cursor e geometria das janelas. Não leio títulos, conteúdo, conversas nem teclas digitadas. Não envio dados à internet.",
            ),
            (
                "As escolhas são suas.",
                "Travessuras começam desligadas. Você pode ajustar as permissões quando quiser. Ctrl + Alt + Shift + B desativa tudo e mantém o pet desligado até você reativar na central.",
            ),
        ];
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.heading("MyDigitalBuddy");
            ui.label(format!("{}/{}", self.page + 1, PAGES.len()));
        });
        ui.add_space(16.0);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 180.0), Sense::hover());
        drawing::buddy(ui.painter(), rect.center(), 0.0, 1.1, &self.engine);
        ui.heading(PAGES[self.page].0);
        ui.add_space(12.0);
        ui.label(PAGES[self.page].1);
        ui.add_space(24.0);
        ui.horizontal(|ui| {
            if self.page > 0 && ui.button("Voltar").clicked() {
                self.page -= 1;
            }
            if ui
                .button(if self.page + 1 == PAGES.len() {
                    "Começar"
                } else {
                    "Próximo"
                })
                .clicked()
            {
                if self.page + 1 == PAGES.len() {
                    self.onboarding = false;
                    self.engine.settings.onboarding_done = true;
                    self.save();
                } else {
                    self.page += 1;
                }
            }
            if ui.button("Pular apresentação").clicked() {
                self.onboarding = false;
                self.engine.settings.onboarding_done = true;
                self.save();
            }
        });
        ui.add_space(12.0);
        ui.colored_label(
            Color32::GRAY,
            "Versão inicial em Rust. Integrações nativas precisam ser validadas no seu sistema.",
        );
    }

    fn pet_viewport(&mut self, ctx: &egui::Context, shown: bool) {
        let center = Pos2::new(110.0, 125.0);
        let x = self.engine.body.x as f32 - center.x;
        let y = self.engine.body.y as f32 - center.y;
        let builder =
            overlay("MyDigitalBuddy · pet", [220.0, 250.0], [x, y], !shown).with_visible(shown);
        ctx.show_viewport_immediate(ViewportId::from_hash_of("buddy"), builder, |ui, _| {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(
                    self.engine.body.x as f32 - center.x,
                    self.engine.body.y as f32 - center.y,
                )));
            let response = ui.interact(
                Rect::from_center_size(center, Vec2::new(150.0, 140.0)),
                egui::Id::unique("buddy-touch"),
                Sense::click_and_drag(),
            );
            drawing::buddy(
                ui.painter(),
                center,
                self.engine.body.angle as f32,
                1.0,
                &self.engine,
            );
            if self.engine.settings.show_emotion && self.engine.settings.test_mode {
                ui.painter().text(
                    center + Vec2::new(0.0, 75.0),
                    Align2::CENTER_TOP,
                    self.engine.emotion.current.name(),
                    FontId::proportional(12.0),
                    Color32::WHITE,
                );
            }
            if response.clicked_by(egui::PointerButton::Primary) {
                self.engine.poke();
            }
            let cursor = ui
                .input(|i| {
                    i.pointer
                        .hover_pos()
                        .zip(i.viewport().inner_rect)
                        .map(|(local, viewport)| {
                            (
                                (viewport.left() + local.x) as f64,
                                (viewport.top() + local.y) as f64,
                            )
                        })
                })
                .unwrap_or((
                    self.input.cursor.0 / self.engine.scale,
                    self.input.cursor.1 / self.engine.scale,
                ));
            if response.drag_started_by(egui::PointerButton::Primary) && self.engine.settings.drag {
                self.engine.drag_start();
                self.drag_last = Some(cursor);
            }
            if self.engine.body.held {
                let velocity = ui.input(|i| i.pointer.velocity());
                self.drag_velocity = (velocity.x as f64, velocity.y as f64);
                self.engine.body.x = cursor.0;
                self.engine.body.y = cursor.1;
                self.drag_last = Some(cursor);
                if response.drag_stopped_by(egui::PointerButton::Primary) {
                    self.engine
                        .drag_release(self.drag_velocity.0, self.drag_velocity.1);
                    self.drag_last = None;
                }
            }
            if response.hovered() && !self.engine.body.held {
                let x = ui.input(|i| i.pointer.hover_pos().map(|p| p.x as f64));
                if let Some(x) = x {
                    self.rub.push_back((self.engine.time, x));
                    while self
                        .rub
                        .front()
                        .is_some_and(|p| self.engine.time - p.0 > 1.2)
                    {
                        self.rub.pop_front();
                    }
                    let points: Vec<_> = self.rub.iter().map(|p| p.1).collect();
                    let delta: Vec<_> = points
                        .windows(2)
                        .map(|w| w[1] - w[0])
                        .filter(|d| d.abs() > 1.0)
                        .collect();
                    let travel = delta.iter().map(|x| x.abs()).sum::<f64>();
                    let turns = delta
                        .windows(2)
                        .filter(|w| w[0].signum() != w[1].signum())
                        .count();
                    if turns >= 3 && travel >= 60.0 {
                        self.engine.caress(0.4);
                    }
                }
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    self.engine.body.omega =
                        (self.engine.body.omega + scroll as f64 * 4.0).clamp(-1000.0, 1000.0);
                }
            }
            response.context_menu(|ui| {
                self.pet_menu(ui);
            });
            self.menu_open = response.context_menu_opened();
        });
    }
    fn pet_menu(&mut self, ui: &mut egui::Ui) {
        ui.strong(&self.engine.pet.name);
        ui.separator();
        if ui.button("Carinho").clicked() {
            self.engine.caress(2.0);
            ui.close();
        }
        if ui.button("Cutucar").clicked() {
            self.engine.poke();
            ui.close();
        }
        if ui
            .button(if self.engine.ball_stored {
                "Trazer bolinha"
            } else {
                "Guardar bolinha"
            })
            .clicked()
        {
            self.engine.ball_stored = !self.engine.ball_stored;
            ui.close();
        }
        if ui
            .button(if self.engine.asleep {
                "Acordar"
            } else {
                "Dormir"
            })
            .clicked()
        {
            if self.engine.asleep {
                self.engine.wake(false);
            } else {
                self.engine.sleep();
            }
            ui.close();
        }
        if ui.button("Aprontar agora").clicked() {
            self.engine.request_mischief();
            ui.close();
        }
        if ui.button("Pedir para trabalhar").clicked() {
            self.engine.desk.request(self.engine.time);
            ui.close();
        }
        if ui
            .checkbox(&mut self.engine.desk.visible, "Notebook")
            .changed()
        {
            if !self.engine.desk.visible {
                self.engine.desk.seated = false;
            }
            ui.close();
        }
        if ui
            .checkbox(&mut self.engine.corner.enabled, "Cantinho do castigo")
            .changed()
        {
            if !self.engine.corner.enabled {
                self.engine.corner.release();
            }
            ui.close();
        }
        ui.checkbox(&mut self.show_monitor, "Monitor neural");
        ui.checkbox(&mut self.show_protest, "Painel de protesto");
        ui.separator();
        if ui.button("Abrir central").clicked() {
            self.restore_central(ui.ctx());
            ui.close();
        }
        if ui.button("Esconder").clicked() {
            self.engine.hidden = true;
            ui.close();
        }
        if ui.button("Pânico").clicked() {
            self.engine.panic();
            ui.close();
        }
        if ui.button("Fechar aplicativo").clicked() {
            self.quit = true;
            ui.ctx()
                .send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
        }
    }
    fn ball_viewport(&mut self, ctx: &egui::Context, shown: bool) {
        let x = self.engine.ball.x as f32 - 30.0;
        let y = self.engine.ball.y as f32 - 30.0;
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("ball"),
            overlay("MyDigitalBuddy · bolinha", [60.0, 60.0], [x, y], !shown).with_visible(shown),
            |ui, _| {
                let center = Pos2::new(30.0, 30.0);
                drawing::ball(ui.painter(), center, self.engine.ball.angle as f32);
                let r = ui.interact(
                    Rect::from_center_size(center, Vec2::splat(42.0)),
                    egui::Id::unique("ball-touch"),
                    Sense::click_and_drag(),
                );
                if r.drag_started() {
                    self.engine.ball.held = true;
                }
                if self.engine.ball.held {
                    self.engine.ball.x = self.input.cursor.0 / self.engine.scale;
                    self.engine.ball.y = self.input.cursor.1 / self.engine.scale;
                    self.engine.ball_handled_until = self.engine.time + 4.0;
                    ui.ctx()
                        .send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(
                            self.engine.ball.x as f32 - 30.0,
                            self.engine.ball.y as f32 - 30.0,
                        )));
                }
                if r.drag_stopped() {
                    self.engine.ball.held = false;
                    let v = ui.input(|i| i.pointer.velocity());
                    self.engine.ball.jump(v.x as f64, v.y as f64, 0.0);
                }
                r.context_menu(|ui| {
                    if ui.button("Guardar bolinha").clicked() {
                        self.engine.ball_stored = true;
                        ui.close();
                    }
                });
            },
        );
    }
    fn notebook_viewport(&mut self, ctx: &egui::Context, shown: bool) {
        let position = self.engine.desk.position;
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("notebook"),
            overlay(
                "MyDigitalBuddy · notebook",
                [160.0, 110.0],
                [position.0 as f32, position.1 as f32],
                !shown,
            )
            .with_visible(shown),
            |ui, _| {
                drawing::notebook(
                    ui.painter(),
                    self.engine.desk.seated,
                    self.engine.time as f32,
                );
                let r = ui.interact(
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(160.0, 110.0)),
                    egui::Id::unique("notebook-drag"),
                    Sense::drag(),
                );
                if r.dragged() {
                    let d = r.drag_delta();
                    let pos = (position.0 + d.x as f64, position.1 + d.y as f64);
                    if self.engine.desk.seated
                        && (pos.0 - position.0).hypot(pos.1 - position.1) > 300.0
                    {
                        self.engine.desk.seated = false;
                        self.engine.senses.kick("DISMISS", 0.7);
                    }
                    self.engine.desk.position = pos;
                    ui.ctx()
                        .send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(
                            pos.0 as f32,
                            pos.1 as f32,
                        )));
                }
                r.context_menu(|ui| {
                    if ui.button("Pedir para trabalhar").clicked() {
                        self.engine.desk.request(self.engine.time);
                        ui.close();
                    }
                    if ui.button("Guardar notebook").clicked() {
                        self.engine.desk.visible = false;
                        self.engine.desk.seated = false;
                        ui.close();
                    }
                });
            },
        );
    }
    fn corner_viewport(&mut self, ctx: &egui::Context, shown: bool) {
        let pos = self.engine.corner.position;
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("corner"),
            overlay(
                "MyDigitalBuddy · cantinho",
                [190.0, 150.0],
                [pos.0 as f32, pos.1 as f32],
                !shown,
            )
            .with_visible(shown),
            |ui, _| {
                drawing::corner(ui.painter(), self.engine.corner.inside);
                let r = ui.interact(
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(190.0, 150.0)),
                    egui::Id::unique("corner-drag"),
                    Sense::drag(),
                );
                if r.dragged() {
                    let d = r.drag_delta();
                    self.engine.corner.position = (pos.0 + d.x as f64, pos.1 + d.y as f64);
                    ui.ctx()
                        .send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(
                            self.engine.corner.position.0 as f32,
                            self.engine.corner.position.1 as f32,
                        )));
                }
                r.context_menu(|ui| {
                    if ui.button("Guardar cantinho").clicked() {
                        self.engine.corner.enabled = false;
                        self.engine.corner.release();
                        ui.close();
                    }
                });
            },
        );
    }
    fn sign_viewport(&mut self, ctx: &egui::Context) {
        if self.engine.asleep || self.engine.time < self.engine.sign_hidden_until {
            return;
        }
        let text = if self.engine.corner.inside {
            if self.engine.brain.hz("ANGRY") >= 3.0 {
                "@#$%@!!!!".into()
            } else {
                "EU SÓ QUERIA BRINCAR".into()
            }
        } else if self.engine.time < self.engine.sign_until {
            self.engine.sign_text.clone().unwrap_or_default()
        } else {
            return;
        };
        let scale = self.engine.protest.scale() as f32;
        let size = Vec2::new(200.0, 75.0) * scale;
        let pos = Pos2::new(
            self.engine.body.x as f32 - size.x / 2.0,
            self.engine.body.y as f32 - 50.0 - size.y,
        );
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("sign"),
            overlay(
                "MyDigitalBuddy · cartaz",
                [size.x, size.y],
                [pos.x, pos.y],
                false,
            ),
            |ui, _| {
                let r =
                    Rect::from_min_max(Pos2::new(4.0, 4.0), Pos2::new(size.x - 4.0, size.y - 4.0));
                ui.painter().rect_filled(
                    r,
                    8.0,
                    if self.engine.sadness.stage > 0 && self.engine.protest.stage == 0 {
                        Color32::from_rgb(174, 212, 250)
                    } else {
                        Color32::from_rgb(255, 225, 145)
                    },
                );
                ui.painter().rect_stroke(
                    r,
                    8.0,
                    Stroke::new(3.0, Color32::from_rgb(54, 46, 40)),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    &text,
                    FontId::proportional(15.0 * scale),
                    Color32::from_rgb(70, 35, 35),
                );
                if ui
                    .interact(r, egui::Id::unique("sign-click"), Sense::click())
                    .clicked()
                {
                    self.engine.dismiss_sign();
                }
            },
        );
    }
    fn thing_viewports(&mut self, ctx: &egui::Context) {
        let things = self.engine.things.clone();
        let mut dismissed = Vec::new();
        for thing in things {
            let (w, h) = if matches!(thing.kind, ThingKind::Paper) {
                (36.0, 36.0)
            } else {
                (
                    thing.body.half_w as f32 * 2.0,
                    thing.body.half_h as f32 * 2.0,
                )
            };
            let pos = [thing.body.x as f32 - w / 2.0, thing.body.y as f32 - h / 2.0];
            let texture = if let ThingKind::Image(path) = &thing.kind {
                if !self.textures.contains_key(path) {
                    match load_image(path) {
                        Ok(image) => {
                            self.textures.insert(
                                path.clone(),
                                ctx.load_texture(
                                    path.to_string_lossy(),
                                    image,
                                    egui::TextureOptions::LINEAR,
                                ),
                            );
                        }
                        Err(error) => {
                            self.error = Some(error.to_string());
                            dismissed.push(thing.id);
                            continue;
                        }
                    }
                }
                self.textures.get(path).cloned()
            } else {
                None
            };
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of(("thing", thing.id)),
                overlay("MyDigitalBuddy · objeto", [w, h], pos, false),
                |ui, _| {
                    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h));
                    match &thing.kind {
                        ThingKind::Paper => {
                            ui.painter().circle_filled(
                                rect.center(),
                                10.0,
                                Color32::from_rgb(225, 222, 211),
                            );
                            ui.painter().line_segment(
                                [
                                    rect.center() - Vec2::splat(5.0),
                                    rect.center() + Vec2::splat(5.0),
                                ],
                                Stroke::new(1.0, Color32::GRAY),
                            );
                        }
                        ThingKind::Note(text) => {
                            ui.painter().rect_filled(
                                rect.shrink(3.0),
                                4.0,
                                Color32::from_rgb(255, 232, 139),
                            );
                            let galley = ui.painter().layout(
                                text.clone(),
                                FontId::proportional(16.0),
                                Color32::from_rgb(65, 52, 45),
                                w - 24.0,
                            );
                            ui.painter().galley(
                                rect.min + Vec2::splat(12.0),
                                galley,
                                Color32::BLACK,
                            );
                        }
                        ThingKind::Image(_) => {
                            if let Some(texture) = &texture {
                                let size = texture.size_vec2();
                                let scale = (w / size.x).min(h / size.y);
                                let r = Rect::from_center_size(rect.center(), size * scale);
                                ui.painter().image(
                                    texture.id(),
                                    r,
                                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                            }
                        }
                    }
                    if ui
                        .interact(
                            rect,
                            egui::Id::unique(("thing-click", thing.id)),
                            Sense::click(),
                        )
                        .clicked()
                    {
                        dismissed.push(thing.id);
                    }
                },
            );
        }
        for id in dismissed {
            self.engine.dismiss(id);
        }
        let used: Vec<_> = self
            .engine
            .things
            .iter()
            .filter_map(|t| {
                if let ThingKind::Image(p) = &t.kind {
                    Some(p.clone())
                } else {
                    None
                }
            })
            .collect();
        self.textures.retain(|k, _| used.contains(k));
    }
    fn fireworks_viewport(&self, ctx: &egui::Context) {
        if self.engine.particles.is_empty() {
            return;
        }
        let b = self.engine.bounds;
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("fireworks"),
            overlay(
                "MyDigitalBuddy · fogos",
                [b.width() as f32, b.height() as f32],
                [b.left as f32, b.top as f32],
                true,
            ),
            |ui, _| {
                for p in &self.engine.particles {
                    let alpha = (p.life / 1.8 * 255.0).min(255.0) as u8;
                    let pos = Pos2::new((p.x - b.left) as f32, (p.y - b.top) as f32);
                    ui.painter().circle_filled(
                        pos,
                        2.8,
                        Color32::from_rgba_unmultiplied(p.color[0], p.color[1], p.color[2], alpha),
                    );
                }
            },
        );
    }
    fn floating_panels(&mut self, ctx: &egui::Context) {
        self.engine.brain.recording = self.show_monitor;
        if self.show_monitor {
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("monitor"),
                ViewportBuilder::default()
                    .with_title("MyDigitalBuddy · monitor neural")
                    .with_inner_size([780.0, 530.0])
                    .with_active(false),
                |ui, _| {
                    if ui.input(|i| i.viewport().close_requested()) {
                        self.show_monitor = false;
                    }
                    egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                        ui.heading(format!(
                            "LIF · {} neurônios · {} sinapses",
                            self.engine.brain.n,
                            self.engine.brain.synapses()
                        ));
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 200.0),
                            Sense::hover(),
                        );
                        for (column, spikes) in self.engine.brain.raster.iter().enumerate() {
                            for (neuron, &fired) in spikes.iter().enumerate() {
                                if fired {
                                    let x = rect.left() + column as f32 / 300.0 * rect.width();
                                    let y = rect.top()
                                        + neuron as f32 / self.engine.brain.n as f32
                                            * rect.height();
                                    ui.painter().rect_filled(
                                        Rect::from_min_size(Pos2::new(x, y), Vec2::new(1.5, 1.0)),
                                        0.0,
                                        if neuron < self.engine.brain.ns {
                                            ACCENT
                                        } else {
                                            Color32::from_rgb(167, 139, 250)
                                        },
                                    );
                                }
                            }
                        }
                        ui.label("Raster: últimos 3 segundos · 1 coluna a cada 10 ms");
                        egui::ScrollArea::vertical()
                            .max_height(220.0)
                            .show(ui, |ui| {
                                egui::Grid::new("pool-monitor")
                                    .num_columns(4)
                                    .striped(true)
                                    .show(ui, |ui| {
                                        for (i, p) in self.engine.brain.pools.iter().enumerate() {
                                            ui.monospace(&p.name);
                                            ui.label(format!(
                                                "{:.2} Hz",
                                                self.engine.brain.rate[i]
                                            ));
                                            if i % 2 == 1 {
                                                ui.end_row();
                                            }
                                        }
                                    });
                            });
                    });
                },
            );
        }
        if self.show_protest {
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("protest-panel"),
                ViewportBuilder::default()
                    .with_title("MyDigitalBuddy · protesto")
                    .with_inner_size([370.0, 390.0])
                    .with_active(false),
                |ui, _| {
                    if ui.input(|i| i.viewport().close_requested()) {
                        self.show_protest = false;
                    }
                    egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                        ui.heading("Protesto e cantinho");
                        ui.label(format!(
                            "Estágio: {} · Revoluções: {}",
                            self.engine.protest.stage, self.engine.protest.revolutions
                        ));
                        bar(
                            ui,
                            "Insatisfação",
                            self.engine.protest.grievance / 1.3,
                            Color32::LIGHT_RED,
                        );
                        bar(
                            ui,
                            "Tristeza",
                            self.engine.sadness.grievance / 1.3,
                            Color32::LIGHT_BLUE,
                        );
                        bar(ui, "Rancor", self.engine.corner.resentment, Color32::YELLOW);
                        bar(ui, "Receio", self.engine.corner.fear, Color32::GRAY);
                        bar(
                            ui,
                            "Vontade de fugir",
                            self.engine.corner.urge.min(1.0),
                            ACCENT,
                        );
                        ui.label(format!(
                            "Fugas bem-sucedidas: {:.0}",
                            self.engine
                                .corner
                                .victories
                                .get("fugir")
                                .copied()
                                .unwrap_or(0.0)
                        ));
                        ui.label(
                            "Carinho alivia a insatisfação. Arrancar cartazes reforça o protesto.",
                        );
                    });
                },
            );
        }
    }
}

impl eframe::App for Desktop {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(input) = self.worker.snapshot.lock() {
            self.input = input.clone();
        }
        self.engine.scale = ctx.native_pixels_per_point().unwrap_or(1.0) as f64;
        if let Some(b) = self.input.work_area {
            self.engine.bounds = WorldRect {
                left: b.left / self.engine.scale,
                top: b.top / self.engine.scale,
                right: b.right / self.engine.scale,
                bottom: b.bottom / self.engine.scale,
            };
        }
        if !self.initialized && !self.input.status.is_empty() {
            let b = self.engine.bounds;
            self.engine.body.x = b.right - 220.0;
            self.engine.body.y = b.top + 120.0;
            self.engine.ball.x = b.left + 400.0;
            self.engine.ball.y = b.bottom - 40.0;
            self.engine.desk.position.0 =
                self.engine.desk.position.0.clamp(b.left, b.right - 160.0);
            self.engine.desk.position.1 =
                self.engine.desk.position.1.clamp(b.top, b.bottom - 110.0);
            self.engine.corner.position.0 =
                self.engine.corner.position.0.clamp(b.left, b.right - 190.0);
            self.engine.corner.position.1 =
                self.engine.corner.position.1.clamp(b.top, b.bottom - 150.0);
            self.initialized = true;
        }
        self.poll_tray(ctx);
        if self.activation.as_ref().is_some_and(|wake| wake.poll()) {
            self.restore_central(ctx);
        }
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        let minimized = ctx.input(|i| i.viewport().minimized) == Some(true);
        if self.restore_until.is_some() {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            if self
                .restore_until
                .is_some_and(|until| Instant::now() >= until)
            {
                self.restore_until = None;
            }
        } else if !self.quit && close_requested && self.tray.is_some() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_to_tray(ctx);
        } else if !self.quit && minimized {
            self.hide_to_tray(ctx);
        } else if !self.quit && self.start_in_tray && self.initialized && !self.onboarding {
            self.start_in_tray = false;
            self.hide_to_tray(ctx);
        }
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64();
        self.last = now;
        if !self.onboarding && !self.menu_open {
            self.engine.advance(dt, &self.input);
        } else if self.input.panic {
            self.engine.panic();
        }
        if self.last_save.elapsed() >= Duration::from_secs(30) {
            self.save();
            self.last_save = now;
        }
        ctx.send_viewport_cmd(ViewportCommand::RequestPaintWhileHidden);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            if self.onboarding {
                self.onboarding(ui);
            } else {
                self.controls(ui);
            }
        });
        if self.reset_confirm {
            egui::Modal::new(egui::Id::unique("reset-confirm")).show(&ctx,|ui| {ui.heading("Redefinir memória?");ui.label(format!("Isso apaga o aprendizado de {} e o histórico de rotina. Os demais personagens permanecem.",self.engine.pet.name));ui.horizontal(|ui| {if ui.button("Cancelar").clicked() {self.reset_confirm=false;}
if ui.button("Redefinir").clicked() {if let Err(e)=self.engine.wipe_memory() {self.error=Some(e.to_string());}self.reset_confirm=false;}});});
        }
        if !self.onboarding && self.engine.settings.active {
            // Keep the GL windows alive when the desktop is clicked or a game
            // covers the screen. Destroying them here used to end the process.
            let shown = self.engine.visible(&self.input);
            self.pet_viewport(&ctx, shown);
            if self.engine.settings.ball && !self.engine.ball_stored {
                self.ball_viewport(&ctx, shown);
            }
            if self.engine.desk.visible {
                self.notebook_viewport(&ctx, shown);
            }
            if self.engine.corner.enabled {
                self.corner_viewport(&ctx, shown);
            }
            if shown {
                self.sign_viewport(&ctx);
                self.thing_viewports(&ctx);
                self.fireworks_viewport(&ctx);
            }
        }
        self.floating_panels(&ctx);
    }
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        if let Err(e) = self
            .engine
            .timeline
            .flush(&self.engine.root.join("timeline.csv"))
        {
            storage::log(&self.engine.root, &format!("timeline: {e}"));
        }
        self.save();
    }
}

fn app_icon() -> Arc<egui::IconData> {
    Arc::new(egui::IconData {
        rgba: crate::sophia_icon::rgba().to_vec(),
        width: crate::sophia_icon::SIDE as u32,
        height: crate::sophia_icon::SIDE as u32,
    })
}

fn overlay(title: &str, size: [f32; 2], position: [f32; 2], passthrough: bool) -> ViewportBuilder {
    ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_position(position)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_active(false)
        .with_taskbar(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_mouse_passthrough(passthrough)
}
fn bar(ui: &mut egui::Ui, label: &str, value: f64, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::ProgressBar::new(value as f32)
                .desired_width(160.0)
                .show_percentage()
                .fill(color),
        );
    });
}
fn load_image(path: &std::path::Path) -> Result<egui::ColorImage, Box<dyn std::error::Error>> {
    if std::fs::metadata(path)?.len() > 20 * 1024 * 1024 {
        return Err("Image exceeds 20 MiB".into());
    }
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode()?.thumbnail(1024, 1024).to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}
