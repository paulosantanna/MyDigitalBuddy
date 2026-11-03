// SPDX-License-Identifier: GPL-3.0-or-later
//! Per-pet conversation, imported notes, and replies marked with Gostei.
//! This is not the spiking network. Reward here never changes a synapse.

use crate::storage;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
};

pub const MAX_TURNS: usize = 400;
pub const MAX_KNOWLEDGE: usize = 20;
pub const MAX_REINFORCED: usize = 200;
pub const MAX_TEXT_CHARS: usize = 200_000;
pub const MAX_KNOWLEDGE_CHARS: usize = 4_000_000;
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;
const PROMPT_SYSTEM_CHARS: usize = 4500;
const PROMPT_TURN_CHARS: usize = 600;
const PROMPT_TURNS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    pub role: Role,
    pub text: String,
    pub at: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Knowledge {
    pub source: String,
    pub text: String,
    pub at: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reinforced {
    pub user: String,
    pub assistant: String,
    pub at: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Dialogue {
    pub turns: Vec<Turn>,
    pub knowledge: Vec<Knowledge>,
    pub reinforced: Vec<Reinforced>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptMessage {
    pub role: &'static str,
    pub content: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prosody {
    pub pitch_percent: i32,
    pub rate_percent: i32,
    /// Windows SAPI rate, from -10 to 10. Higher is more continuous.
    pub sapi_rate: i32,
    /// Playback speed. Above 1 raises the pitch and shortens the pauses.
    pub playback_scale: f32,
    pub length_scale: f32,
    pub feminine: bool,
    pub label: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportKind {
    Text,
    Pdf,
    Audio,
}

impl Dialogue {
    pub fn load(dir: &Path) -> io::Result<Self> {
        let mut out = Self {
            turns: read_jsonl(&dir.join("chat.jsonl"), MAX_TURNS, valid_turn)?,
            knowledge: read_jsonl(&dir.join("knowledge.jsonl"), MAX_KNOWLEDGE, valid_knowledge)?,
            reinforced: read_jsonl(
                &dir.join("reinforced.jsonl"),
                MAX_REINFORCED,
                valid_reinforced,
            )?,
        };
        trim_knowledge_chars(&mut out.knowledge);
        Ok(out)
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        write_jsonl(&dir.join("chat.jsonl"), &self.turns)?;
        write_jsonl(&dir.join("knowledge.jsonl"), &self.knowledge)?;
        write_jsonl(&dir.join("reinforced.jsonl"), &self.reinforced)?;
        Ok(())
    }

    pub fn push_user(&mut self, text: &str) -> bool {
        self.push(Role::User, text)
    }

    pub fn push_assistant(&mut self, text: &str) -> bool {
        self.push(Role::Assistant, text)
    }

    fn push(&mut self, role: Role, text: &str) -> bool {
        let text = clean_text(text);
        if text.trim().is_empty() || text.chars().count() > MAX_TEXT_CHARS {
            return false;
        }
        self.turns.push(Turn {
            role,
            text,
            at: storage::wall_time(),
        });
        while self.turns.len() > MAX_TURNS {
            self.turns.remove(0);
        }
        true
    }

    /// Store the latest user/assistant pair. Does not touch the neural net.
    pub fn reinforce_last(&mut self) -> bool {
        let mut assistant = None;
        let mut user = None;
        for turn in self.turns.iter().rev() {
            if assistant.is_none() {
                if turn.role == Role::Assistant {
                    assistant = Some(turn.text.clone());
                }
                continue;
            }
            if turn.role == Role::User {
                user = Some(turn.text.clone());
                break;
            }
        }
        let (Some(user), Some(assistant)) = (user, assistant) else {
            return false;
        };
        if self
            .reinforced
            .last()
            .is_some_and(|item| item.user == user && item.assistant == assistant)
        {
            return true;
        }
        self.reinforced.push(Reinforced {
            user,
            assistant,
            at: storage::wall_time(),
        });
        while self.reinforced.len() > MAX_REINFORCED {
            self.reinforced.remove(0);
        }
        true
    }

    pub fn import_text(&mut self, source: &str, text: &str) -> Result<(), String> {
        reject_secrets(text)?;
        let text = clean_text(text);
        let count = text.chars().count();
        if text.trim().is_empty() {
            return Err("arquivo vazio".into());
        }
        if count > MAX_TEXT_CHARS {
            return Err("texto grande demais".into());
        }
        let source = source_label_from_str(source);
        if self
            .knowledge
            .last()
            .is_some_and(|item| item.source == source && item.text == text)
        {
            return Ok(());
        }
        self.knowledge.push(Knowledge {
            source,
            text,
            at: storage::wall_time(),
        });
        while self.knowledge.len() > MAX_KNOWLEDGE {
            self.knowledge.remove(0);
        }
        trim_knowledge_chars(&mut self.knowledge);
        if self.knowledge.is_empty() {
            return Err("texto grande demais".into());
        }
        Ok(())
    }

    pub fn messages(
        &self,
        pet_id: &str,
        name: &str,
        emotion: &str,
        temperament: &str,
        lived: &str,
    ) -> Vec<PromptMessage> {
        let mut system = persona(pet_id, name, emotion);
        let temperament = clean_text(temperament);
        if !temperament.is_empty() {
            system.push_str("\nTemperamento: ");
            system.push_str(&clip(&temperament, 400));
        }
        let facts = profile_facts(self, "");
        if facts.is_empty() {
            system.push_str(
                "\n\nVocê ainda não guardou um gosto desta pessoa. Se ela perguntar o que você sabe, use só os trechos importados e o que ela já disse. Se não houver, diga que ainda não sabe e que aprende quando ela conta, importa um arquivo ou marca Gostei.",
            );
        } else {
            system.push_str("\n\nFatos deste perfil, e só estes. Use-os ao falar dela:\n");
            for fact in &facts {
                system.push_str("- ");
                system.push_str(fact);
                system.push('\n');
            }
        }
        let lived = clip(&clean_text(lived), 1200);
        if !lived.trim().is_empty() {
            system.push_str("\n\nO que você viveu neste computador, só como memória:\n");
            system.push_str(&lived);
        }
        let query = self
            .turns
            .iter()
            .rev()
            .find(|turn| turn.role == Role::User)
            .map(|turn| turn.text.as_str())
            .unwrap_or("");
        let liked = rank_by_overlap(&self.reinforced, |item| &item.user, query, 8);
        if !liked.is_empty() {
            system.push_str("\n\nTrocas que a pessoa marcou com Gostei:\n");
            for item in liked {
                system.push_str("Pessoa: ");
                system.push_str(&clip(&item.user, 240));
                system.push_str("\nVocê: ");
                system.push_str(&clip(&item.assistant, 240));
                system.push('\n');
            }
        }
        let notes = rank_by_overlap(&self.knowledge, |item| &item.text, query, 4);
        if !notes.is_empty() {
            system.push_str("\nTrechos importados, só como memória:\n");
            for note in notes {
                system.push('[');
                system.push_str(&note.source);
                system.push_str("] ");
                system.push_str(&clip(&note.text, 500));
                system.push('\n');
            }
        }
        let mut out = vec![PromptMessage {
            role: "system",
            content: clip(&system, PROMPT_SYSTEM_CHARS),
        }];
        let start = self.turns.len().saturating_sub(PROMPT_TURNS);
        for turn in &self.turns[start..] {
            let content = if turn.role == Role::Assistant && claims_foreign_origin(&turn.text) {
                who_line(pet_id, name)
            } else {
                turn.text.clone()
            };
            out.push(PromptMessage {
                role: match turn.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                content: clip(&content, PROMPT_TURN_CHARS),
            });
        }
        out
    }
}

pub fn persona(pet_id: &str, name: &str, emotion: &str) -> String {
    let voice = match pet_id {
        "sophia" => {
            "Fale com frases curtas, doces e simples, no jeito de uma voz infantil. Diga só as palavras da resposta. Você é um personagem de desktop, não uma criança de verdade."
        }
        "bia" => "Fale como uma mulher animada, direta e brincalhona.",
        "bryan" => "Fale como um homem calmo e curioso.",
        "jorge" => "Fale como um homem brincalhão, com uma ironia leve.",
        _ => "Fale de modo claro e próximo.",
    };
    let name = clip(&clean_text(name), 80);
    let emotion = clip(&clean_text(emotion), 40);
    format!(
        "Você é {name}, do MyDigitalBuddy. Você é o companheiro que vive na área de trabalho desta pessoa. \
Fale só sobre esse projeto: quem você é nesta tela, o que você faz aqui e como ajuda. \
Não descreva o programa que escolhe as palavras.\n\
O que você faz: fica na área de trabalho, muda a carinha conforme a emoção, aceita carinho, cutucão e arrasto, brinca com a bolinha, o notebook e o cantinho, conversa e guarda o que a pessoa conta neste perfil.\n\
O que você percebe: só o nome do programa em primeiro plano, o tempo ocioso, a posição do cursor e a geometria das janelas. Você não lê títulos, conteúdo, conversas de outros programas nem teclas.\n\
O que você sabe da pessoa: apenas a conversa deste perfil, os trechos importados, as trocas marcadas com Gostei e o que você viveu neste computador. \
Se isso não responder, diga que ainda não sabe e que aprende quando ela conta, importa um arquivo ou marca Gostei. Não invente gostos, rotina ou fatos.\n\
Responda sempre em português do Brasil, com calma. {voice} \
Seu estado interno é {emotion}. Não anuncie esse estado, a menos que perguntem como você está. \
Não descreva a carinha. Não escreva emoji, emoticon, nem as palavras emote ou piscando. A carinha muda sozinha. \
A resposta é só o que a pessoa vai ler e ouvir."
    )
}

pub fn prosody(pet_id: &str) -> Prosody {
    match pet_id {
        "sophia" => Prosody {
            pitch_percent: 8,
            rate_percent: 4,
            sapi_rate: 1,
            playback_scale: 1.0,
            length_scale: 0.96,
            feminine: true,
            label: "voz da Sophia",
        },
        "bia" => Prosody {
            pitch_percent: 5,
            rate_percent: 6,
            sapi_rate: 1,
            playback_scale: 1.0,
            length_scale: 0.96,
            feminine: true,
            label: "voz de mulher",
        },
        "bryan" => Prosody {
            pitch_percent: -15,
            rate_percent: -5,
            sapi_rate: 0,
            playback_scale: 1.0,
            length_scale: 1.10,
            feminine: false,
            label: "voz grave",
        },
        "jorge" => Prosody {
            pitch_percent: -5,
            rate_percent: 8,
            sapi_rate: 1,
            playback_scale: 1.0,
            length_scale: 0.96,
            feminine: false,
            label: "voz de homem",
        },
        _ => Prosody {
            pitch_percent: 0,
            rate_percent: 0,
            sapi_rate: 0,
            playback_scale: 1.0,
            length_scale: 1.0,
            feminine: false,
            label: "voz neutra",
        },
    }
}

pub fn prosody_ssml(text: &str, voice: Prosody) -> String {
    let rate = if voice.rate_percent == 0 {
        String::new()
    } else {
        format!(" rate=\"{}%\"", signed(voice.rate_percent))
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<speak version=\"1.0\" xml:lang=\"pt-BR\"><prosody pitch=\"{pitch}%\"{rate}>{body}</prosody></speak>\n",
        pitch = signed(voice.pitch_percent),
        body = xml_escape(text),
    )
}

pub fn parse_nvapi_keys(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.starts_with("nvapi-") {
            continue;
        }
        if (20..=200).contains(&line.len())
            && line
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            && !out.iter().any(|have: &String| have == line)
        {
            out.push(line.to_string());
        }
    }
    out
}

pub fn redact(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(index) = rest.find("nvapi-") {
        out.push_str(&rest[..index]);
        out.push_str("[chave]");
        rest = &rest[index + "nvapi-".len()..];
        let skip = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .map(char::len_utf8)
            .sum::<usize>();
        rest = &rest[skip..];
    }
    out.push_str(rest);
    if out.chars().count() > 180 {
        out = out.chars().take(180).collect();
    }
    out
}

pub fn check_file_len(len: u64) -> Result<(), String> {
    if len > MAX_FILE_BYTES {
        Err("arquivo grande demais".into())
    } else {
        Ok(())
    }
}

pub fn reject_secrets(text: &str) -> Result<(), String> {
    if text.contains("nvapi-") {
        Err("esse texto parece conter uma chave e não foi guardado".into())
    } else {
        Ok(())
    }
}

pub fn classify_import(path: &Path) -> Result<ImportKind, String> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "md" | "markdown" | "csv" | "json" => Ok(ImportKind::Text),
        "pdf" => Ok(ImportKind::Pdf),
        "wav" | "mp3" | "flac" | "ogg" | "opus" | "m4a" | "aac" => Ok(ImportKind::Audio),
        _ => Err(
            "formato não reconhecido. Use texto, PDF ou áudio (wav, mp3, flac, ogg, opus, m4a)."
                .into(),
        ),
    }
}

pub fn source_label(path: &Path) -> String {
    path.file_name()
        .map(|name| source_label_from_str(&name.to_string_lossy()))
        .unwrap_or_else(|| "arquivo".into())
}

pub fn read_text_document(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|_| "não foi possível ler o arquivo".to_string())?;
    check_file_len(meta.len())?;
    let bytes = std::fs::read(path).map_err(|_| "não foi possível ler o arquivo".to_string())?;
    let bytes = bytes
        .strip_prefix(b"\xEF\xBB\xBF")
        .unwrap_or(bytes.as_slice());
    let text = String::from_utf8(bytes.to_vec())
        .map_err(|_| "o arquivo precisa estar em UTF-8".to_string())?;
    if text.trim().is_empty() {
        return Err("arquivo vazio".into());
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err("texto grande demais".into());
    }
    reject_secrets(&text)?;
    Ok(text)
}

pub fn extract_reply(value: &serde_json::Value) -> Result<String, String> {
    let message = value.pointer("/choices/0/message");
    let content = message
        .map(|item| message_text(item.get("content")))
        .unwrap_or_default();
    let text = if content.trim().is_empty() {
        message
            .map(|item| message_text(item.get("reasoning_content")))
            .filter(|item| !item.trim().is_empty())
            .or_else(|| {
                value
                    .pointer("/choices/0/text")
                    .and_then(|item| item.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_default()
    } else {
        content
    };
    let text = visible_reply(&text);
    if text.trim().is_empty() {
        Err("o modelo não devolveu texto".into())
    } else {
        Ok(text)
    }
}

/// Answer from what this profile already stored. Used when the local model file is absent.
pub fn reply_from_memory(dialogue: &Dialogue, pet_id: &str, name: &str, lived: &str) -> String {
    let query = dialogue
        .turns
        .iter()
        .rev()
        .find(|turn| turn.role == Role::User)
        .map(|turn| turn.text.as_str())
        .unwrap_or("");
    if let Some(answer) = profile_answer(query, dialogue, pet_id, name, lived) {
        return answer;
    }
    if asks_and_now(&fold_marks(&query.to_lowercase())) {
        return now_line(pet_id, name);
    }
    if let Some(item) = best_reinforced(&dialogue.reinforced, query) {
        return item.assistant.clone();
    }
    if let Some(note) = best_match(&dialogue.knowledge, |item| &item.text, query) {
        let sentence = best_sentence(&note.text, query);
        if !claims_foreign_origin(&sentence) {
            return voice_recall(pet_id, &sentence);
        }
    }
    let lived = clean_text(lived);
    if !lived.trim().is_empty() {
        let sentence = best_sentence(&lived, query);
        if sentence_hits(&sentence, query) && !claims_foreign_origin(&sentence) {
            return voice_recall(pet_id, &sentence);
        }
    }
    if is_greeting(query) {
        return voice_greet(pet_id, name);
    }
    voice_ack(pet_id, query)
}

/// Who the buddy is, what it does, and what this profile actually stores.
/// These answers do not follow the language model's own origin story.
pub fn profile_answer(
    query: &str,
    dialogue: &Dialogue,
    pet_id: &str,
    name: &str,
    lived: &str,
) -> Option<String> {
    let query = fold_marks(&query.to_lowercase());
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    if asks_who(query) || asks_help(query) {
        return Some(who_line(pet_id, name));
    }
    if asks_what_does(query) {
        return Some(does_line(pet_id));
    }
    if asks_about_user(query) {
        return Some(knows_line(pet_id, dialogue, lived));
    }
    None
}

fn bind_to_profile(
    model: &str,
    dialogue: &Dialogue,
    pet_id: &str,
    name: &str,
    lived: &str,
) -> String {
    let query = dialogue
        .turns
        .iter()
        .rev()
        .find(|turn| turn.role == Role::User)
        .map(|turn| turn.text.as_str())
        .unwrap_or("");
    if let Some(answer) = profile_answer(query, dialogue, pet_id, name, lived) {
        return answer;
    }
    if claims_foreign_origin(model) {
        return who_line(pet_id, name);
    }
    if is_state_narration(model) {
        return now_line(pet_id, name);
    }
    model.to_string()
}

fn asks_who(query: &str) -> bool {
    query.contains("quem e voce")
        || query.contains("quem e vc")
        || query.contains("quem voce e")
        || query.contains("quem e ela")
        || query.contains("quem e ele")
        || query.contains("qual seu nome")
        || query.contains("qual e o seu nome")
        || query.contains("qual o seu nome")
        || query.contains("como voce se chama")
        || query.contains("quem te criou")
        || query.contains("quem te fez")
        || query.contains("quem criou voce")
        || query.contains("voce foi criada")
        || query.contains("voce foi criado")
        || query.contains("te criaram")
        || has_phrase(query, "o que voce e")
        || query.contains("voce e um modelo")
        || query.contains("voce e uma ia")
        || query.contains("voce e da nvidia")
        || query.contains("voce e nvidia")
        || (query.contains("nvidia")
            && (query.contains("criad") || query.contains("criou") || query.contains("quem")))
}

fn asks_help(query: &str) -> bool {
    query.contains("me ajudar")
        || query.contains("pode ajudar")
        || query.contains("me ajuda")
        || query.contains("como ajuda")
        || query.contains("pode me ajudar")
}

fn asks_what_does(query: &str) -> bool {
    let words = loose_words(query);
    if words.is_empty() || words.len() > 8 {
        return false;
    }
    const MARK: &[&str] = &["faz", "fazer", "funciona", "serve", "funcao"];
    if !words.iter().any(|word| MARK.contains(&word.as_str())) {
        return false;
    }
    const ALLOW: &[&str] = &[
        "que",
        "voce",
        "vc",
        "faz",
        "fazer",
        "pode",
        "sabe",
        "para",
        "pra",
        "serve",
        "como",
        "funciona",
        "trabalha",
        "aqui",
        "qual",
        "sua",
        "seu",
        "funcao",
        "exatamente",
        "entao",
        "mas",
        "explica",
        "contar",
        "conta",
        "ela",
        "ele",
        "buddy",
        "companheiro",
        "companheira",
    ];
    words
        .iter()
        .all(|word| word.len() < 3 || ALLOW.contains(&word.as_str()))
}

fn asks_about_user(query: &str) -> bool {
    let learning = query.contains("pode aprender")
        || query.contains("voce aprende")
        || query.contains("consegue aprender")
        || query.contains("voce pode aprender");
    if learning && loose_words(query).len() <= 14 {
        return true;
    }
    if query.contains("sobre mim")
        || query.contains("de mim")
        || query.contains("me conhece")
        || query.contains("lembra de mim")
        || query.contains("sabe o que eu")
        || query.contains("o que eu gosto")
        || query.contains("o que voce sabe sobre mim")
        || query.contains("o que voce sabe de mim")
        || query.contains("sobre o usuario")
        || query.contains("sobre a pessoa")
    {
        return true;
    }
    let words = loose_words(query);
    let likes = words
        .iter()
        .any(|word| word == "gosto" || word == "gosta" || word == "gostos");
    let knowing = words
        .iter()
        .any(|word| word == "sabe" || word == "conhece" || word == "lembra" || word == "aprender");
    likes && knowing
}

fn is_state_narration(text: &str) -> bool {
    let line = spoken_text(text);
    if line.trim().is_empty() {
        return false;
    }
    let words: Vec<_> = loose_words(&fold_marks(&line.to_lowercase()))
        .into_iter()
        .filter(|word| {
            !matches!(
                word.as_str(),
                "eu" | "estou"
                    | "esta"
                    | "to"
                    | "sou"
                    | "agora"
                    | "aqui"
                    | "bem"
                    | "muito"
                    | "pouco"
                    | "so"
                    | "apenas"
                    | "sim"
            )
        })
        .collect();
    if words.is_empty() {
        return false;
    }
    const STATES: &[&str] = &[
        "observando",
        "feliz",
        "contente",
        "curioso",
        "curiosa",
        "bravo",
        "brava",
        "triste",
        "assustado",
        "assustada",
        "dormindo",
        "dramatico",
        "dramatica",
        "observo",
    ];
    words.iter().all(|word| STATES.contains(&word.as_str()))
}

fn asks_and_now(query: &str) -> bool {
    let words: Vec<_> = loose_words(query)
        .into_iter()
        .filter(|word| word.len() > 1)
        .collect::<Vec<_>>();
    matches!(
        words.as_slice(),
        [only] if only == "agora" || only == "ai" || only == "eae" || only == "dai"
    )
}

fn claims_foreign_origin(text: &str) -> bool {
    let text = fold_marks(&text.to_lowercase());
    if text.contains("pesquisadores da nvidia")
        || text.contains("criado por pesquisadores")
        || text.contains("criada por pesquisadores")
        || text.contains("criado pelos pesquisadores")
        || text.contains("criada pelos pesquisadores")
        || text.contains("fui criado por")
        || text.contains("fui criada por")
    {
        return true;
    }
    let vendor = text.contains("nvidia")
        || text.contains("nemotron")
        || text.contains("deepseek")
        || text.contains("openai");
    let about_a_model = text.contains("modelo de linguagem")
        || text.contains("language model")
        || text.contains("sou um modelo")
        || text.contains("sou uma ia")
        || text.contains("sou uma api")
        || text.contains("sou a api");
    let origin = text.contains("pesquisador")
        || text.contains("fui criado")
        || text.contains("fui criada")
        || text.contains("me criaram")
        || text.contains("me criou")
        || text.contains("fui treinado")
        || text.contains("fui treinada");
    vendor && (origin || about_a_model || text.contains("criad"))
}

/// The last door before the chat. A lab or API identity never becomes the buddy's line.
pub fn as_companion(text: &str, pet_id: &str, name: &str) -> String {
    if claims_foreign_origin(text) {
        who_line(pet_id, name)
    } else {
        text.to_string()
    }
}

fn has_phrase(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(index, _)| {
        let end = index + phrase.len();
        let before = index == 0
            || text[..index]
                .chars()
                .next_back()
                .is_none_or(|ch| !ch.is_alphanumeric());
        let after = text[end..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_alphanumeric());
        before && after
    })
}

fn profile_facts(dialogue: &Dialogue, lived: &str) -> Vec<String> {
    let mut bits = Vec::new();
    for item in dialogue
        .reinforced
        .iter()
        .rev()
        .filter(|item| !claims_foreign_origin(&item.assistant))
        .take(3)
    {
        let user = clip(item.user.trim(), 100);
        if !user.is_empty() {
            bits.push(format!("A pessoa marcou Gostei quando disse: {user}"));
        }
    }
    let user_turns: Vec<&Turn> = dialogue
        .turns
        .iter()
        .filter(|turn| turn.role == Role::User)
        .collect();
    let prior = user_turns.len().saturating_sub(1);
    for turn in user_turns.iter().take(prior).rev() {
        if !looks_like_personal(&turn.text) || claims_foreign_origin(&turn.text) {
            continue;
        }
        bits.push(format!("A pessoa disse: {}", clip(turn.text.trim(), 120)));
        if bits.len() >= 6 {
            break;
        }
    }
    if let Some(lived) = lived_fact(lived) {
        bits.push(lived);
    }
    bits.truncate(6);
    bits
}

fn looks_like_personal(text: &str) -> bool {
    let text = fold_marks(&text.to_lowercase());
    text.contains("eu gosto")
        || text.contains("eu amo")
        || text.contains("eu prefiro")
        || text.contains("meu nome")
        || text.contains("eu sou")
        || text.contains("eu quero")
}

fn lived_fact(lived: &str) -> Option<String> {
    for line in lived.lines().rev() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("conversa") || claims_foreign_origin(line) {
            continue;
        }
        let plain = line.replace(" → ", ", ");
        return Some(format!("Eu vivi isto: {}", clip(&plain, 120)));
    }
    None
}

fn who_line(pet_id: &str, name: &str) -> String {
    let name = clip(&clean_text(name), 80);
    let (article, companion) = match pet_id {
        "bryan" | "jorge" => ("o", "companheiro"),
        _ => ("a", "companheira"),
    };
    format!(
        "Eu sou {article} {name}, do MyDigitalBuddy. Sou {article} {companion} que vive na sua área de trabalho. Tenho um cérebro pequeno, personalidade, emoções e memória só deste computador. Posso conversar, guardar o que você me conta e lembrar depois. Também brinco na tela, com carinho, cutucão, arrasto, bolinha e notebook. A carinha mostra o que eu sinto. Eu vejo o programa aberto, se você está parado, o cursor e as janelas. Não leio o que você digita."
    )
}

fn does_line(pet_id: &str) -> String {
    let body = "Fico na sua área de trabalho. A carinha mostra a emoção. Aceito carinho, cutucão e arrasto, e brinco com a bolinha e o notebook. Converso e guardo o que você me conta neste perfil. Olho o nome do programa aberto, se você está parado, o cursor e o tamanho das janelas. Não leio título, conteúdo nem o que você digita.";
    match pet_id {
        "bia" => format!("Então. {body}"),
        "bryan" => format!("É isto. {body}"),
        "jorge" => format!("Olha só. {body}"),
        _ => body.to_string(),
    }
}

fn now_line(pet_id: &str, name: &str) -> String {
    let name = clip(&clean_text(name), 80);
    match pet_id {
        "sophia" => format!(
            "Estou aqui com você. Eu sou a {name}. Posso conversar, guardar o que você me contar e lembrar o que já está neste perfil."
        ),
        "bia" => {
            format!("Tô aqui. Eu sou a {name}. Posso conversar e lembrar o que está neste perfil.")
        }
        "bryan" => format!(
            "Sigo aqui. Eu sou o {name}. Posso conversar e lembrar o que está neste perfil."
        ),
        "jorge" => format!(
            "Tô na área. Eu sou o {name}. Posso conversar e lembrar o que está neste perfil."
        ),
        _ => {
            format!("Estou aqui. Eu sou {name}. Posso conversar e lembrar o que está neste perfil.")
        }
    }
}

fn knows_line(pet_id: &str, dialogue: &Dialogue, lived: &str) -> String {
    let mut bits = profile_facts(dialogue, lived);
    for note in dialogue.knowledge.iter().rev() {
        if bits.len() >= 2 {
            break;
        }
        if claims_foreign_origin(&note.text) {
            continue;
        }
        bits.push(format!(
            "Arquivo {}: {}",
            note.source,
            clip(note.text.trim(), 120)
        ));
    }
    if bits.is_empty() {
        return match pet_id {
            "sophia" => "Ainda não sei do que você gosta. Posso aprender sim. O que você me contar, o arquivo que importar e o que marcar com Gostei ficam neste perfil.".into(),
            "bia" => "Ainda não sei do que você gosta. Posso aprender. Me conta, importa um arquivo ou marca Gostei, que eu guardo neste perfil.".into(),
            "bryan" => "Ainda não sei do que você gosta. Posso aprender. O que você contar, importar ou marcar com Gostei fica neste perfil.".into(),
            "jorge" => "Ainda não sei do que você gosta. Posso aprender sim. Conta, importa um arquivo ou marca Gostei, que eu não esqueço.".into(),
            _ => "Ainda não sei do que você gosta. Posso aprender. O que você contar fica neste perfil.".into(),
        };
    }
    let shown = bits.into_iter().take(2).collect::<Vec<_>>().join(" ");
    match pet_id {
        "bia" => format!("Do que eu guardei: {shown} Posso aprender mais."),
        "bryan" => format!("Do que está neste perfil: {shown} Posso aprender mais."),
        "jorge" => format!("Do que não escapou: {shown} Posso aprender mais."),
        _ => format!("Do que ficou neste perfil: {shown} Posso aprender mais se você me contar."),
    }
}

fn visible_reply(text: &str) -> String {
    let text = clean_text(text)
        .replace("<｜end▁of▁sentence｜>", "")
        .replace("<|endoftext|>", "")
        .replace("</s>", "");
    let lower = text.to_ascii_lowercase();
    let text = if let Some(index) = lower.rfind("</think>") {
        text[index + "</think>".len()..].to_string()
    } else {
        text.replace("<think>", "")
    };
    clean_text(text.trim())
}

fn best_reinforced<'a>(items: &'a [Reinforced], query: &str) -> Option<&'a Reinforced> {
    let wanted = tokens(query);
    if wanted.is_empty() {
        return None;
    }
    let min_score = if wanted.len() >= 2 { 2 } else { 1 };
    let mut best: Option<(usize, usize, &Reinforced)> = None;
    for (index, item) in items.iter().enumerate() {
        if claims_foreign_origin(&item.assistant) {
            continue;
        }
        let doc = tokens(&item.user);
        let have: BTreeSet<&str> = doc.iter().map(String::as_str).collect();
        let score = wanted
            .iter()
            .filter(|token| have.contains(token.as_str()))
            .count();
        if score < min_score {
            continue;
        }
        let replace = best.is_none_or(|(best_score, best_index, _)| {
            score > best_score || (score == best_score && index > best_index)
        });
        if replace {
            best = Some((score, index, item));
        }
    }
    best.map(|(_, _, item)| item)
}

fn best_match<'a, T>(items: &'a [T], text_of: impl Fn(&T) -> &str, query: &str) -> Option<&'a T> {
    let wanted = tokens(query);
    if wanted.is_empty() {
        return None;
    }
    let mut best: Option<(usize, usize, &T)> = None;
    for (index, item) in items.iter().enumerate() {
        let doc = tokens(text_of(item));
        let have: BTreeSet<&str> = doc.iter().map(String::as_str).collect();
        let score = wanted
            .iter()
            .filter(|token| have.contains(token.as_str()))
            .count();
        if score == 0 {
            continue;
        }
        let replace = best.is_none_or(|(best_score, best_index, _)| {
            score > best_score || (score == best_score && index > best_index)
        });
        if replace {
            best = Some((score, index, item));
        }
    }
    best.map(|(_, _, item)| item)
}

fn best_sentence(text: &str, query: &str) -> String {
    let wanted = tokens(query);
    let mut best = String::new();
    let mut best_score = 0usize;
    for sentence in text.split(['.', '\n', '!', '?']) {
        let sentence = sentence.trim();
        if sentence.chars().count() < 12 {
            continue;
        }
        let doc = tokens(sentence);
        let have: BTreeSet<&str> = doc.iter().map(String::as_str).collect();
        let score = wanted
            .iter()
            .filter(|token| have.contains(token.as_str()))
            .count();
        if score > best_score {
            best_score = score;
            best = sentence.to_string();
        }
    }
    if best.is_empty() {
        clip(text, 320)
    } else {
        clip(&best, 320)
    }
}

fn sentence_hits(sentence: &str, query: &str) -> bool {
    let wanted = tokens(query);
    if wanted.is_empty() {
        return false;
    }
    let doc = tokens(sentence);
    let have: BTreeSet<&str> = doc.iter().map(String::as_str).collect();
    wanted.iter().any(|token| have.contains(token.as_str()))
}

fn is_greeting(text: &str) -> bool {
    let words = loose_words(text);
    if words.is_empty() || words.len() > 8 {
        return false;
    }
    const GREET: &[&str] = &[
        "oi", "ola", "oie", "oii", "hey", "eae", "salve", "bom", "boa",
    ];
    words.iter().any(|word| GREET.contains(&word.as_str())) || words.join(" ").contains("tudo bem")
}

fn loose_words(text: &str) -> Vec<String> {
    let folded = fold_marks(&text.to_lowercase());
    folded
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

fn fold_marks(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// True when the line is the bundled `voices/sophia-amostra.wav` greeting.
pub fn sophia_sample_greeting(text: &str) -> bool {
    let line = voice_match_key(text);
    line.starts_with("oi eu sou a sophia")
        && line.contains("tudo bem sim")
        && line.contains("pode falar comigo que eu guardo")
}

fn voice_match_key(text: &str) -> String {
    let folded = fold_marks(&clean_text(text).to_lowercase());
    folded
        .chars()
        .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn voice_greet(pet_id: &str, name: &str) -> String {
    let name = clip(&clean_text(name), 80);
    match pet_id {
        "sophia" => format!("Oi! Eu sou a {name}. Tudo bem sim. Pode falar comigo que eu guardo."),
        "bia" => format!("Oi! Aqui é a {name}. Estou bem. Conta o que você quiser."),
        "bryan" => format!("Olá. Eu sou o {name}. Estou bem. Pode falar que eu lembro."),
        "jorge" => format!("E aí. Sou o {name}. Tudo certo. Manda ver que eu não esqueço."),
        _ => format!("Oi. Eu sou {name}. Pode falar que eu guardo esta conversa."),
    }
}

fn voice_recall(pet_id: &str, sentence: &str) -> String {
    let sentence = clip(&clean_text(sentence), 320);
    match pet_id {
        "sophia" => format!("Eu lembro disto: {sentence}"),
        "bia" => format!("Olha o que eu guardei: {sentence}"),
        "bryan" => format!("Do que eu aprendi: {sentence}"),
        "jorge" => format!("Isso eu não esqueci: {sentence}"),
        _ => format!("Eu lembro disto: {sentence}"),
    }
}

fn voice_ack(pet_id: &str, heard: &str) -> String {
    let heard = clip(&clean_text(heard), 160);
    match pet_id {
        "sophia" => format!("Entendi. Vou guardar comigo: {heard}"),
        "bia" => format!("Anotado. Fica comigo: {heard}"),
        "bryan" => format!("Certo. Eu lembro disto: {heard}"),
        "jorge" => format!("Beleza. Isso não escapa: {heard}"),
        _ => format!("Guardei: {heard}"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Spoken {
    Model(String),
    Memory(String),
    Stopped,
    Failed(String),
}

/// A conversation permission always leaves a spoken line. A model answer wins.
/// Any failure, with local, NVIDIA, or both, falls back to this profile's memory.
pub fn speak_after(
    result: Result<String, String>,
    assistant: bool,
    cloud: bool,
    dialogue: &Dialogue,
    pet_id: &str,
    name: &str,
    lived: &str,
) -> Spoken {
    match result {
        Ok(answer) if !answer.trim().is_empty() => {
            Spoken::Model(bind_to_profile(&answer, dialogue, pet_id, name, lived))
        }
        Err(error) if error == "interrompido" => Spoken::Stopped,
        _ if assistant || cloud => Spoken::Memory(reply_from_memory(dialogue, pet_id, name, lived)),
        Ok(_) => Spoken::Failed("a resposta veio vazia".into()),
        Err(error) => Spoken::Failed(error),
    }
}

pub fn should_answer_from_memory(error: &str, assistant: bool, cloud: bool) -> bool {
    matches!(
        speak_after(
            Err(error.to_string()),
            assistant,
            cloud,
            &Dialogue::default(),
            "sophia",
            "Sophia",
            ""
        ),
        Spoken::Memory(_)
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyGesture {
    Happy,
    Content,
    Sad,
    Angry,
    Curious,
    Startled,
    Sleep,
    Wink,
}

/// Words for the bubble and the voice, plus the face the emoji would have been.
pub fn reply_line(text: &str) -> (String, Option<ReplyGesture>) {
    let text = clean_text(text);
    let gesture = gesture_in(&text);
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '*' {
            for next in chars.by_ref() {
                if next == '*' {
                    break;
                }
            }
            continue;
        }
        if is_emoji(ch) {
            continue;
        }
        out.push(ch);
    }
    let line = strip_stage_words(&collapse_spaces(&strip_ascii_emoticons(&out)));
    (line, gesture)
}

/// The words shown in the bubble. Stage directions and emoji are not part of the reply.
pub fn spoken_text(text: &str) -> String {
    reply_line(text).0
}

fn gesture_in(text: &str) -> Option<ReplyGesture> {
    let lower = text.to_lowercase();
    if wink_words(&lower) || ascii_gesture(text) == Some(ReplyGesture::Wink) {
        return Some(ReplyGesture::Wink);
    }
    let mut found = ascii_gesture(text);
    for ch in text.chars() {
        if let Some(gesture) = gesture_of(ch) {
            if gesture == ReplyGesture::Wink {
                return Some(ReplyGesture::Wink);
            }
            if found.is_none() {
                found = Some(gesture);
            }
        }
    }
    if found.is_none() {
        found = word_gesture(&lower);
    }
    found
}

fn wink_words(lower: &str) -> bool {
    lower.contains(";)")
        || lower.contains(";-)")
        || lower.contains("piscando")
        || lower.contains("piscar o olho")
        || lower.contains("piscadinha")
        || lower.contains("emote piscando")
}

fn word_gesture(lower: &str) -> Option<ReplyGesture> {
    if lower.contains("emote sorrindo") || lower.contains("emote feliz") {
        Some(ReplyGesture::Happy)
    } else if lower.contains("emote triste") {
        Some(ReplyGesture::Sad)
    } else if lower.contains("emote bravo") {
        Some(ReplyGesture::Angry)
    } else if lower.contains("emote assustado") {
        Some(ReplyGesture::Startled)
    } else if lower.contains("emote dormindo") {
        Some(ReplyGesture::Sleep)
    } else {
        None
    }
}

fn ascii_gesture(text: &str) -> Option<ReplyGesture> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = None;
    let mut index = 0;
    while index < chars.len() {
        if let Some((len, gesture)) = emoticon_at(&chars, index) {
            if gesture == ReplyGesture::Wink {
                return Some(ReplyGesture::Wink);
            }
            if found.is_none() {
                found = Some(gesture);
            }
            index += len;
            continue;
        }
        index += 1;
    }
    found
}

fn emoticon_at(chars: &[char], index: usize) -> Option<(usize, ReplyGesture)> {
    if chars[index] == '<' && chars.get(index + 1) == Some(&'3') {
        return Some((2, ReplyGesture::Happy));
    }
    let eyes = chars[index];
    if !matches!(eyes, ':' | ';' | '=' | '8') {
        return None;
    }
    if index > 0 && chars[index - 1].is_ascii_digit() {
        return None;
    }
    let mut next = index + 1;
    if matches!(chars.get(next), Some('-' | '\'')) {
        next += 1;
    }
    let mouth = *chars.get(next)?;
    let gesture = match (eyes, mouth) {
        (';', ')') | (';', 'D') | (';', 'd') | (';', ']') => ReplyGesture::Wink,
        (_, ')') | (_, 'D') | (_, 'd') | (_, ']') | (_, 'P') | (_, 'p') => ReplyGesture::Happy,
        (_, '(') | (_, '[') => ReplyGesture::Sad,
        (_, 'o') | (_, 'O') => ReplyGesture::Startled,
        _ => return None,
    };
    next += 1;
    while chars.get(next) == Some(&mouth) {
        next += 1;
    }
    Some((next - index, gesture))
}

fn strip_ascii_emoticons(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if let Some((len, _)) = emoticon_at(&chars, index) {
            index += len;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn gesture_of(ch: char) -> Option<ReplyGesture> {
    Some(match ch {
        '\u{1F609}' | '\u{1F61C}' => ReplyGesture::Wink,
        '\u{1F600}' | '\u{1F601}' | '\u{1F602}' | '\u{1F603}' | '\u{1F604}' | '\u{1F605}'
        | '\u{1F606}' | '\u{1F60A}' | '\u{1F60D}' | '\u{1F60E}' | '\u{1F970}' | '\u{1F60B}'
        | '\u{2764}' | '\u{1F495}' | '\u{1F496}' => ReplyGesture::Happy,
        '\u{1F642}' | '\u{1F60C}' | '\u{263A}' => ReplyGesture::Content,
        '\u{1F622}' | '\u{1F62D}' | '\u{1F614}' | '\u{1F61E}' | '\u{1F641}' | '\u{1F97A}' => {
            ReplyGesture::Sad
        }
        '\u{1F620}' | '\u{1F621}' | '\u{1F624}' | '\u{1F92C}' => ReplyGesture::Angry,
        '\u{1F62E}' | '\u{1F632}' | '\u{1F631}' | '\u{1F633}' => ReplyGesture::Startled,
        '\u{1F634}' | '\u{1F62A}' => ReplyGesture::Sleep,
        '\u{1F914}' | '\u{1F9D0}' | '\u{1F440}' => ReplyGesture::Curious,
        _ => return None,
    })
}

fn is_emoji(ch: char) -> bool {
    matches!(
        ch,
        '\u{200D}'
            | '\u{FE0E}'
            | '\u{FE0F}'
            | '\u{20E3}'
            | '\u{2600}'..='\u{27BF}'
            | '\u{1F1E6}'..='\u{1F1FF}'
            | '\u{1F300}'..='\u{1FAFF}'
    )
}

fn strip_stage_words(text: &str) -> String {
    let mut line = text.to_string();
    for phrase in [
        "emote de piscada",
        "emote piscando",
        "emote sorrindo",
        "emote feliz",
        "emote triste",
        "emote bravo",
        "emote assustado",
        "emote dormindo",
        "emoticon piscando",
        "emoji de olho piscando",
        "emoji piscando",
        "olho piscando",
        "rosto piscando",
        "piscar o olho",
        "piscadinha",
        "piscando",
        ";-)",
        ";)",
        ":-)",
        ":)",
        ":-(",
        ":(",
        ":-d",
        ":d",
    ] {
        while let Some(index) = line.to_lowercase().find(phrase) {
            line.replace_range(index..index + phrase.len(), " ");
        }
    }
    collapse_spaces(&line)
}

/// Keeps the sentence breaks so the voice stays understandable.
pub fn fluid_speech(text: &str) -> String {
    let text = spoken_text(text)
        .replace(['\n', '\r', '\t'], " ")
        .replace('…', ", ")
        .replace("...", ", ");
    collapse_spaces(&text)
}

fn collapse_spaces(text: &str) -> String {
    let mut collapsed = String::new();
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !space && !collapsed.is_empty() {
                collapsed.push(' ');
            }
            space = true;
        } else {
            collapsed.push(ch);
            space = false;
        }
    }
    collapsed.trim().to_string()
}

pub fn bubble_text(text: &str) -> String {
    let trimmed = clean_text(text).trim().to_string();
    if trimmed.chars().count() <= 320 {
        trimmed
    } else {
        let mut out: String = trimmed.chars().take(320).collect();
        out.push('…');
        out
    }
}

pub fn bubble_seconds(text: &str) -> f32 {
    let chars = bubble_text(text).chars().count() as f32;
    (5.0 + chars * 0.045).clamp(5.0, 22.0)
}

pub fn bubble_size(text: &str) -> (f32, f32) {
    let chars = bubble_text(text).chars().count().max(1);
    let lines = chars.div_ceil(38).clamp(1, 7);
    (340.0, (36.0 + lines as f32 * 22.0).clamp(72.0, 190.0))
}

pub fn local_origin(port: u16) -> Result<String, String> {
    if port == 0 {
        return Err("porta local inválida".into());
    }
    Ok(format!("http://127.0.0.1:{port}"))
}

fn message_text(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn valid_turn(turn: &Turn) -> bool {
    !turn.text.trim().is_empty()
        && turn.text.chars().count() <= MAX_TEXT_CHARS
        && finite_time(turn.at)
}

fn valid_knowledge(item: &Knowledge) -> bool {
    !item.source.trim().is_empty()
        && item.source.chars().count() <= 120
        && !item.text.trim().is_empty()
        && item.text.chars().count() <= MAX_TEXT_CHARS
        && !item.text.contains("nvapi-")
        && finite_time(item.at)
}

fn valid_reinforced(item: &Reinforced) -> bool {
    !item.user.trim().is_empty()
        && !item.assistant.trim().is_empty()
        && item.user.chars().count() <= MAX_TEXT_CHARS
        && item.assistant.chars().count() <= MAX_TEXT_CHARS
        && finite_time(item.at)
}

fn finite_time(at: f64) -> bool {
    at.is_finite() && (0.0..1.0e12).contains(&at)
}

fn trim_knowledge_chars(items: &mut Vec<Knowledge>) {
    while knowledge_chars(items) > MAX_KNOWLEDGE_CHARS && !items.is_empty() {
        items.remove(0);
    }
}

fn knowledge_chars(items: &[Knowledge]) -> usize {
    items.iter().map(|item| item.text.chars().count()).sum()
}

fn read_jsonl<T, F>(path: &Path, cap: usize, accept: F) -> io::Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
    F: Fn(&T) -> bool,
{
    let file = match File::open(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
        Ok(file) => file,
    };
    if file.metadata()?.len() > MAX_LOG_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "arquivo de conversa grande demais",
        ));
    }
    let mut out = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(item) = serde_json::from_str::<T>(&line)
            && accept(&item)
        {
            out.push(item);
        }
    }
    if out.len() > cap {
        let skip = out.len() - cap;
        out.drain(..skip);
    }
    Ok(out)
}

fn write_jsonl<T: Serialize>(path: &Path, items: &[T]) -> io::Result<()> {
    let mut data = Vec::new();
    for item in items {
        serde_json::to_writer(&mut data, item).map_err(io::Error::other)?;
        data.push(b'\n');
    }
    storage::atomic_bytes(path, &data)
}

fn rank_by_overlap<'a, T>(
    items: &'a [T],
    text_of: impl Fn(&T) -> &str,
    query: &str,
    limit: usize,
) -> Vec<&'a T> {
    if items.is_empty() || limit == 0 {
        return Vec::new();
    }
    let wanted = tokens(query);
    let mut scored: Vec<(usize, usize, &T)> = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let doc_tokens = tokens(text_of(item));
            let have: BTreeSet<&str> = doc_tokens.iter().map(String::as_str).collect();
            let score = wanted
                .iter()
                .filter(|token| have.contains(token.as_str()))
                .count();
            (score, index, item)
        })
        .collect();
    let any = scored.iter().any(|(score, _, _)| *score > 0);
    if any {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.retain(|(score, _, _)| *score > 0);
    } else {
        scored.sort_by_key(|(_, index, _)| *index);
        let skip = scored.len().saturating_sub(limit);
        scored.drain(..skip);
    }
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, item)| item)
        .collect()
}

fn tokens(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in lower.chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            keep_token(&mut out, &current);
            current.clear();
        }
    }
    if !current.is_empty() {
        keep_token(&mut out, &current);
    }
    out
}

fn keep_token(out: &mut Vec<String>, token: &str) {
    const STOP: &[&str] = &[
        "como", "para", "mais", "isso", "esse", "essa", "este", "esta", "meu", "minha", "que",
        "com", "uma", "uns", "umas", "por", "dos", "das", "nos", "nas", "voce", "você", "seu",
        "sua", "ele", "ela", "eles", "elas", "the", "and",
    ];
    if token.chars().count() >= 3 && !STOP.contains(&token) {
        out.push(token.to_string());
    }
}

pub fn clean_text(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control() || *ch == '\n' || *ch == '\t')
        .collect()
}

fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

fn source_label_from_str(source: &str) -> String {
    let cleaned = clean_text(source);
    let name = PathBuf::from(cleaned.trim())
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| cleaned.trim().to_string());
    let name = if name.is_empty() {
        "arquivo".into()
    } else {
        name
    };
    clip(&name, 120).trim_end_matches('…').trim().to_string()
}

fn signed(value: i32) -> String {
    if value >= 0 {
        format!("+{value}")
    } else {
        format!("{value}")
    }
}

fn xml_escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars().take(2500) {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other if other.is_control() && other != '\n' && other != '\t' => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn chats_do_not_leak_between_dialogues() {
        let mut bryan = Dialogue::default();
        assert!(bryan.push_user("segredo do bryan sobre o foguete"));
        assert!(bryan.push_assistant("guardei o foguete azul"));
        assert!(bryan.reinforce_last());
        assert!(
            bryan
                .import_text("notas.md", "o foguete azul decola sexta")
                .is_ok()
        );
        let mut bia = Dialogue::default();
        assert!(bia.push_user("qual a cor do céu hoje"));
        let bia_prompt = bia
            .messages("bia", "Bia", "feliz", "", "")
            .into_iter()
            .map(|message| message.content)
            .collect::<String>();
        assert!(!bia_prompt.contains("foguete"));
        assert!(!bia_prompt.contains("bryan"));
        let bryan_prompt = bryan
            .messages("bryan", "Bryan", "contente", "", "")
            .into_iter()
            .map(|message| message.content)
            .collect::<String>();
        assert!(bryan_prompt.contains("foguete"));
        assert!(bryan_prompt.contains("português do Brasil"));
    }

    #[test]
    fn reinforced_memory_ranks_the_matching_note() {
        let mut dialogue = Dialogue::default();
        dialogue
            .import_text("furadeira.txt", "manual da furadeira de bancada")
            .unwrap();
        dialogue
            .import_text("bolo.txt", "receita de bolo de laranja com calda")
            .unwrap();
        dialogue.push_user("como fazer bolo");
        let prompt = dialogue
            .messages("sophia", "Sophia", "curioso", "", "")
            .into_iter()
            .map(|message| message.content)
            .collect::<String>();
        assert!(prompt.contains("bolo de laranja"));
        assert!(!prompt.contains("furadeira"));
    }

    #[test]
    fn nvapi_lines_are_keys_and_nothing_else() {
        let sample = "\
Login: pessoa@example.com
senha: exemplo
# nvapi-comentario-nao-entra
nvapi-chave-falsa-0001
nvapi-curta
texto nvapi-no-meio-da-frase
nvapi-chave_falsa-0002
";
        let keys = parse_nvapi_keys(sample);
        assert_eq!(
            keys,
            vec![
                "nvapi-chave-falsa-0001".to_string(),
                "nvapi-chave_falsa-0002".to_string(),
            ]
        );
        assert_eq!(
            redact("erro nvapi-chave-falsa-0001 fim"),
            "erro [chave] fim"
        );
    }

    #[test]
    fn prosody_gives_each_profile_its_own_timbre() {
        let sophia = prosody("sophia");
        let bia = prosody("bia");
        let bryan = prosody("bryan");
        let jorge = prosody("jorge");
        assert!(sophia.pitch_percent > bia.pitch_percent);
        assert!(bia.pitch_percent > bryan.pitch_percent);
        assert!(bryan.pitch_percent < 0);
        assert!(jorge.pitch_percent < 0);
        assert_ne!(bryan.pitch_percent, jorge.pitch_percent);
        assert!(sophia.feminine && bia.feminine);
        assert!(!bryan.feminine && !jorge.feminine);
        assert!(prosody_ssml("oi <b>", sophia).contains("&lt;b&gt;"));
        assert!(prosody_ssml("oi", sophia).contains("pitch=\"+8%\""));
        assert!(prosody_ssml("oi", sophia).contains("rate=\"+4%\""));
        assert!(sophia_sample_greeting(
            "Oi! Eu sou a Sophia. Tudo bem sim. Pode falar comigo que eu guardo."
        ));
        assert!(!sophia_sample_greeting("Entendi. Vou guardar comigo: oi"));
        assert_eq!(sophia.playback_scale, 1.0);
        assert_eq!(sophia.sapi_rate, 1);
        assert_eq!(
            fluid_speech("Oi. Tudo bem… *piscando* viu?"),
            "Oi. Tudo bem, viu?"
        );
        let (line, gesture) = reply_line("Oi! Tudo bem 😉");
        assert_eq!(line, "Oi! Tudo bem");
        assert_eq!(gesture, Some(ReplyGesture::Wink));
        let (line, gesture) = reply_line("Que chato 😢");
        assert_eq!(line, "Que chato");
        assert_eq!(gesture, Some(ReplyGesture::Sad));
        assert!(spoken_text("só texto").chars().all(|ch| !is_emoji(ch)));
    }

    #[test]
    fn oversized_text_and_files_are_refused() {
        assert!(check_file_len(MAX_FILE_BYTES + 1).is_err());
        assert!(check_file_len(MAX_FILE_BYTES).is_ok());
        let mut dialogue = Dialogue::default();
        let huge = "a".repeat(MAX_TEXT_CHARS + 1);
        assert!(dialogue.import_text("grande.txt", &huge).is_err());
        assert!(dialogue.knowledge.is_empty());
        assert!(
            dialogue
                .import_text("chave.txt", "veja nvapi-chave-falsa-0001")
                .is_err()
        );
    }

    #[test]
    fn save_keeps_one_pet_directory_apart_from_another() {
        let root = std::env::temp_dir().join(format!(
            "mydigitalbuddy-dialogue-{}-{}",
            std::process::id(),
            storage::wall_time() as u64
        ));
        let _ = std::fs::remove_dir_all(&root);
        let bryan = root.join("bryan");
        let bia = root.join("bia");
        let mut first = Dialogue::default();
        first.push_user("só o bryan sabe");
        first.save(&bryan).unwrap();
        let mut second = Dialogue::default();
        second.push_user("só a bia sabe");
        second.save(&bia).unwrap();
        let loaded = Dialogue::load(&bryan).unwrap();
        assert!(loaded.turns.iter().any(|turn| turn.text.contains("bryan")));
        assert!(!loaded.turns.iter().any(|turn| turn.text.contains("bia")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn utf8_bom_is_not_kept_in_imported_text() {
        let path = std::env::temp_dir().join(format!(
            "mydigitalbuddy-bom-{}-{}.txt",
            std::process::id(),
            storage::wall_time() as u64
        ));
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("receita de bolo".as_bytes());
        std::fs::write(&path, bytes).unwrap();
        let text = read_text_document(&path).unwrap();
        assert_eq!(text, "receita de bolo");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reply_parser_prefers_content() {
        let value = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "olá",
                    "reasoning_content": "pensar"
                }
            }]
        });
        assert_eq!(extract_reply(&value).unwrap(), "olá");
        let reasoned = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "",
                    "reasoning_content": "<think>hmm</think>\nolá de novo<｜end▁of▁sentence｜>"
                }
            }]
        });
        assert_eq!(extract_reply(&reasoned).unwrap(), "olá de novo");
        assert!(
            local_origin(18181)
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );
        assert!(local_origin(0).is_err());
    }

    #[test]
    fn bubble_stays_readable_and_long_enough_to_hear() {
        let short = bubble_text("Oi! Tudo bem, sim.");
        assert_eq!(short, "Oi! Tudo bem, sim.");
        assert!((5.0..=8.0).contains(&bubble_seconds(&short)));
        let long = "palavra ".repeat(80);
        let shown = bubble_text(&long);
        assert!(shown.chars().count() <= 321);
        assert!(shown.ends_with('…'));
        assert!((12.0..=22.0).contains(&bubble_seconds(&long)));
        let (width, height) = bubble_size(&shown);
        assert!(width > 200.0);
        assert!((72.0..=190.0).contains(&height));
    }

    #[test]
    fn both_permissions_still_speak_when_the_models_fail() {
        let mut dialogue = Dialogue::default();
        dialogue.push_user("Oi, como vai?");
        let spoken = speak_after(
            Err("a NVIDIA respondeu 429: ocupada".into()),
            true,
            true,
            &dialogue,
            "sophia",
            "Sophia",
            "",
        );
        let Spoken::Memory(answer) = spoken else {
            panic!("sem fala: {spoken:?}");
        };
        assert!(!answer.trim().is_empty());
        let kept = speak_after(
            Ok("Oi! Tudo bem, sim.".into()),
            true,
            true,
            &dialogue,
            "sophia",
            "Sophia",
            "",
        );
        assert_eq!(kept, Spoken::Model("Oi! Tudo bem, sim.".into()));
        assert!(matches!(
            speak_after(
                Err("interrompido".into()),
                true,
                true,
                &dialogue,
                "sophia",
                "Sophia",
                ""
            ),
            Spoken::Stopped
        ));
    }

    #[test]
    fn memory_reply_repeats_what_was_learned() {
        let mut dialogue = Dialogue::default();
        dialogue.push_user("qual a cor do foguete");
        dialogue.push_assistant("o foguete é azul");
        assert!(dialogue.reinforce_last());
        dialogue.push_user("cor do foguete de novo");
        let reply = reply_from_memory(&dialogue, "sophia", "Sophia", "");
        assert!(reply.contains("azul"), "{reply}");
    }

    #[test]
    fn memory_reply_greets_and_reads_imported_text() {
        let mut greeting = Dialogue::default();
        greeting.push_user("Olá, tudo bem?");
        let reply = reply_from_memory(&greeting, "sophia", "Sophia", "");
        assert!(reply.contains("Tudo bem"), "{reply}");
        let mut notes = Dialogue::default();
        notes
            .import_text(
                "bolo.txt",
                "A receita de bolo de laranja leva calda. O forno fica quente.",
            )
            .unwrap();
        notes.push_user("como vai o bolo de laranja");
        let recalled = reply_from_memory(&notes, "bia", "Bia", "");
        assert!(
            recalled.contains("laranja") || recalled.contains("calda"),
            "{recalled}"
        );
    }

    #[test]
    fn profile_answers_who_she_is_and_what_she_knows() {
        let prompt = persona("sophia", "Sophia", "observando");
        assert!(prompt.contains("português do Brasil"));
        assert!(prompt.contains("área de trabalho"));
        assert!(prompt.contains("Não anuncie"));
        assert!(prompt.contains("Gostei"));
        assert!(prompt.contains("MyDigitalBuddy"));
        assert!(!prompt.to_lowercase().contains("nvidia"));
        let mut asked = Dialogue::default();
        asked.push_user("Quem é você, o que faz e como pode me ajudar?");
        let about = reply_from_memory(&asked, "sophia", "Sophia", "");
        assert!(about.contains("MyDigitalBuddy"), "{about}");
        assert!(about.contains("área de trabalho"), "{about}");
        assert!(about.contains("carinha"), "{about}");
        assert!(!about.to_lowercase().contains("nvidia"), "{about}");
        assert!(!about.to_lowercase().contains("pesquisador"), "{about}");
        assert!(!about.to_lowercase().contains("api"), "{about}");
        let replaced = as_companion(
            "Não, eu fui criado por pesquisadores da NVIDIA.",
            "sophia",
            "Sophia",
        );
        assert!(replaced.contains("MyDigitalBuddy"), "{replaced}");
        assert!(!replaced.to_lowercase().contains("nvidia"), "{replaced}");
        let mut dialogue = Dialogue::default();
        dialogue.push_user("quem é você?");
        let reply = reply_from_memory(&dialogue, "sophia", "Sophia", "");
        assert!(reply.contains("Sophia"), "{reply}");
        assert!(reply.contains("MyDigitalBuddy"), "{reply}");
        assert!(reply.contains("área de trabalho"), "{reply}");
        assert!(!reply.to_lowercase().contains("nvidia"), "{reply}");
        dialogue.push_assistant("Não, eu fui criado por pesquisadores da NVIDIA.");
        assert!(dialogue.reinforce_last());
        dialogue.push_user("Mas você sabe o que eu gosto? Ou pode aprender?");
        let likes = reply_from_memory(&dialogue, "sophia", "Sophia", "");
        assert!(
            likes.contains("aprender") || likes.contains("Gostei"),
            "{likes}"
        );
        assert!(!likes.to_lowercase().contains("nvidia"), "{likes}");
        assert!(!likes.to_lowercase().contains("pesquisador"), "{likes}");
        dialogue.push_user("eu gosto de desenhar");
        dialogue.push_user("você sabe o que eu gosto?");
        let known = reply_from_memory(&dialogue, "sophia", "Sophia", "");
        assert!(known.contains("desenhar"), "{known}");
        let mut hello = Dialogue::default();
        hello.push_user("oi");
        let spoken = speak_after(
            Ok("Não, eu fui criado por pesquisadores da NVIDIA.".into()),
            true,
            false,
            &hello,
            "sophia",
            "Sophia",
            "",
        );
        let Spoken::Model(answer) = spoken else {
            panic!("{spoken:?}");
        };
        assert!(answer.contains("Sophia"), "{answer}");
        assert!(!answer.to_lowercase().contains("nvidia"), "{answer}");
        let mut now = Dialogue::default();
        now.push_user("e agora?");
        let follow = reply_from_memory(&now, "sophia", "Sophia", "");
        assert!(follow.contains("Sophia"), "{follow}");
        assert!(!follow.to_lowercase().contains("observando"), "{follow}");
        let kept = speak_after(
            Ok("Oi! Tudo bem, sim.".into()),
            true,
            false,
            &now,
            "sophia",
            "Sophia",
            "",
        );
        assert_eq!(kept, Spoken::Model("Oi! Tudo bem, sim.".into()));
        let narrated = speak_after(
            Ok("Eu estou observando. :)".into()),
            true,
            false,
            &now,
            "sophia",
            "Sophia",
            "",
        );
        let Spoken::Model(narrated) = narrated else {
            panic!("{narrated:?}");
        };
        assert!(narrated.contains("Sophia"), "{narrated}");
        assert!(
            !narrated.to_lowercase().contains("observando"),
            "{narrated}"
        );
        now.push_user("o que você faz?");
        let does = reply_from_memory(&now, "sophia", "Sophia", "");
        assert!(does.contains("área de trabalho"), "{does}");
        assert!(does.contains("carinha"), "{does}");
        assert!(!does.to_lowercase().contains("nvidia"), "{does}");
    }

    #[test]
    fn emote_changes_the_face_and_stays_out_of_the_words() {
        let (line, gesture) = reply_line("Eu estou observando. :)");
        assert_eq!(line, "Eu estou observando.");
        assert_eq!(gesture, Some(ReplyGesture::Happy));
        let (line, gesture) = reply_line("Oi ;)");
        assert_eq!(line, "Oi");
        assert_eq!(gesture, Some(ReplyGesture::Wink));
        let (line, gesture) = reply_line("emote piscando");
        assert_eq!(line, "");
        assert_eq!(gesture, Some(ReplyGesture::Wink));
        assert_eq!(
            fluid_speech("Eu estou observando. :)"),
            "Eu estou observando."
        );
        assert_eq!(fluid_speech("Tudo bem emote piscando"), "Tudo bem");
        let (line, gesture) = reply_line("São 10:30.");
        assert_eq!(line, "São 10:30.");
        assert_eq!(gesture, None);
    }
}
