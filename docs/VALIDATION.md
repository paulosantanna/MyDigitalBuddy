# Validação executada

Ambiente: Linux x86-64, Rust 1.99.1, eframe 0.37.0. A data da entrega é 2026-11-01 no fuso America/Sao_Paulo. Nenhum resultado abaixo implica que a execução foi validada no computador do usuário.

| Verificação | Resultado |
|---|---|
| `cargo fmt --all --check` | Passou |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passou sem avisos no Linux |
| Testes com desktop | 27 passaram |
| Testes sem a feature desktop | 27 passaram |
| `cargo build --release --locked` | Executável Linux gerado |
| `cargo check --target x86_64-pc-windows-gnu --all-targets` | Verificação estática cruzada passou |
| `cargo check --target aarch64-apple-darwin --all-targets` | Verificação estática cruzada passou |
| Clippy cruzado para Windows GNU | Passou sem avisos |
| Duas simulações headless de 60s, seed 42, diretórios novos | Resumos idênticos |
| Aplicativo Linux em Xvfb/Openbox/Mesa software | Janelas e central renderizaram |
| Minimizar a central no teste gráfico | Três viewports permaneceram visíveis; processo continuou ativo |
| Segurar Ctrl+Alt+Shift+B por 350ms no teste gráfico | `active=false` foi persistido |
| Fechar normalmente com Alt+F4 | Processo encerrou com código 0 |

Os registros resumidos estão em `ui-smoke.json` e `headless-smoke.json`. As capturas são de uma sessão de teste isolada, sem dados pessoais. Xvfb sem compositor não demonstra transparência final em um desktop real.

## Cobertura dos 27 testes

- 3 testes da rede: equações contra modelo/draws do Python, restauração atômica e reprodução/habituação/limites plásticos.
- 7 testes de caracterização: limites de classificação emocional, homeostase, trajetórias físicas, emoção temporal com prioridades, motores, condicionamento e protesto.
- 17 testes de integração: pânico e reinício, pausa/fullscreen, lock de escritor, isolamento entre pets, IDs, dados inválidos, punição de um episódio sem duplicação, seleção limitada por permissões, importação válida/inválida, plataformas, bolinha, sequência aleatória de interações, escrita atômica, definições personalizadas, seleção para importação e posição do pet no notebook.

O oráculo neural contém um modelo original de 470 neurônios e verifica 150 passos de 1ms com entradas fixas. Tolerância absoluta: 1e-10 nos estados contínuos e igualdade em disparos/refratários. Os demais oráculos numéricos usam 1e-9. Não se está comparando streams aleatórios nativos dos dois runtimes.

## Execução nativa no Windows

Ambiente observado em 2026-11-02: Windows 11, Rust 1.99.1 host `x86_64-pc-windows-gnu`, GCC do MinGW-w64. O host MSVC não linkou porque `link.exe` não está instalado. O binário de debug não importa DLLs do MinGW; as dependências nativas são do Windows, incluindo `OPENGL32.dll`.

| Verificação | Resultado |
|---|---|
| `cargo build --locked` com o host GNU | Executável gerado |
| `--headless 1 --seed 42` | Código 0; 470 neurônios, emoção Watching, sem avisos |
| Processo gráfico com `--direto` | Permaneceu respondendo; janelas visíveis: central, pet, bolinha e notebook |

Essa sessão não mede DPI, múltiplos monitores, transparência final, atalho global nem paridade com o Pestinha.

## Limites desta evidência

macOS **não foi executado nativamente neste host**. No Windows, a sessão acima confirma link e abertura das janelas; não cobre foco, permissões do sistema, atalho, escala/DPI ou todos os gestos. `cargo check` continua sem substituir essa execução. O workflow repete build e testes em runners nativos quando o repositório estiver publicado; os resultados remotos só podem ser declarados depois da execução.

Não há comprovação de paridade global ou pixel a pixel com todos os caminhos do legado. A central substitui o tray e há diferenças de RNG e de mídia descritas em `MIGRATION.md`. Recomenda-se aceite de gestos, fullscreen, múltiplos monitores, hotkey e importação com uma cópia dos dados no sistema de destino.

Uma tentativa inicial de destruir diretamente a janela com uma ferramenta de teste provocou `BadDrawable` dentro do winit. O teste de encerramento foi corrigido para o fluxo normal do gerenciador de janelas (Alt+F4), que passou; destruição externa forçada de uma janela não foi corrigida no backend de terceiros.
