# Migração Python → Rust

Fonte fixa: Pestinha `5063be916e68ebeaaf3723d49b387cfe2c362549`, 60 arquivos no pacote principal e 8.961 linhas. O repositório original foi baixado para análise e permaneceu intacto. O novo repositório não incorpora o histórico Git, o runtime Python, o executável pré-compilado ou os recursos visuais originais.

Reversa 1.3.6 foi instalado com a API Writer oficial, a partir do checkout Sandeco identificado em `PROVENANCE.json`. As instruções de descoberta e migração orientaram inventário, mapeamento por módulo, extração de contratos, arquitetura e critérios de paridade. A execução ocorreu sequencialmente no contexto do agente de código. As entrevistas/pausas do assistente foram adaptadas à autorização de execução de ponta a ponta; não foram registradas aprovações humanas inexistentes.

| Origem | Destino | Tratamento |
|---|---|---|
| core/nervous_system.py | core/brain.rs, topology.rs | Fórmulas e 135 projeções portadas; armazenamento esparso por spike |
| core/emotion.py, motor.py | core/emotion.rs, motor.rs | Vetores, suavização, prioridades e decodificador |
| core/homeostasis.py, conditioning.py | módulos correspondentes | Equações e limites preservados |
| core/episodic.py | core/memory.rs | Bandit contextual, episódios, revisão por castigo e consolidação |
| core/physics.py, body/toy.py | core/physics.rs, engine.rs | Colisões, giro, plataformas, tracker e brincadeira |
| mind/ e body/behavior.py | core/mind.rs, engine.rs | Estados puros e composição por relógio explícito |
| world/ e pinturas body/ | desktop.rs, desktop/drawing.rs | Viewports egui e vetores em Rust |
| sense/ | senses.rs, timeline.rs | Metadados convertidos em canais e categorias agregadas |
| plataforma/ | platform/{windows,linux,macos}.rs | APIs nativas; sem intérpretes ou subprocessos de sensores |
| controle/, app.py | engine.rs, desktop.rs | Comandos explícitos e um proprietário do estado |
| config, paths, store, settings, migração | config.rs, storage.rs, engine.rs | Tipos validados, dados por usuário, lock e importação por staging |
| ui/ | desktop.rs | Onboarding, central, permissões, monitor e painel |
| ferramentas de docs/PyInstaller | Markdown e Cargo/CI | Ferramentas legadas substituídas |

## Diferenças declaradas

- **RNG:** SplitMix64 + Box-Muller substitui NumPy PCG64. Mesma seed não cria os mesmos pesos aleatórios. Os oráculos neurais usam pesos e draws fixos do original para verificar as equações separadamente.
- **Interface:** egui substitui Qt; a central minimizável substitui o tray. Layout e rótulos foram reorganizados. Não há promessa de comparação pixel a pixel.
- **Mídia:** elementos básicos são desenhados em código. Memes/fotos são opcionais e fornecidos pelo usuário. GIF externo usa primeiro quadro.
- **Dados:** uma pasta própria do usuário substitui gravações ao lado do programa. Importação preserva os originais e exige destino vazio.
- **Plataformas:** Windows e macOS tiveram verificação estática cruzada neste host. Isso não comprova foco, DPI, transparência ou hotkey em uma sessão nativa desses sistemas. macOS usa a primeira janela normal visível como aproximação da atividade ativa e depende das permissões do SO. Wayland tem restrições de posicionamento/sensores.
- **Renderização:** brilhos, sinais, balística e a UI foram reexpressos em Rust. A equivalência de todos os gestos, tempos e detalhes visuais ainda exige teste de aceitação nativo.

Os arquivos em `_reversa_sdd/` descrevem a extração e a intenção da migração. O relatório `VALIDATION.md` limita as afirmações à evidência realmente obtida.
