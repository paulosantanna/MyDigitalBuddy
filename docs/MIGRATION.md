# Migração Python → Rust

Fonte fixa: Pestinha `5063be916e68ebeaaf3723d49b387cfe2c362549`, 60 arquivos no pacote principal e 8.961 linhas. O repositório original foi baixado para análise e permaneceu intacto. O novo repositório não incorpora o histórico Git, o runtime Python, o executável pré-compilado ou os recursos visuais originais.

Reversa 1.3.6 foi instalado com a API Writer oficial, a partir do checkout identificado em `PROVENANCE.json`. As instruções de descoberta e migração orientaram inventário, mapeamento por módulo, extração de contratos, arquitetura e critérios de paridade. A execução ocorreu sequencialmente no contexto do agente de código. As entrevistas/pausas do assistente foram adaptadas à autorização de execução de ponta a ponta; não foram registradas aprovações humanas inexistentes.

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
- **Interface:** egui substitui Qt. A central volta para a bandeja ao minimizar ou fechar; o pet continua. Abrir o executável de novo, com essa instância já viva, pede para a janela voltar e o segundo processo encerra — antes o lock rejeitava o processo em silêncio. No Windows, "Iniciar com o Windows" (On/Off) registra o executável em `HKCU\...\Run` com `--tray`. Clicar na área de trabalho não é tela cheia: as janelas de shell (`Progman`, `WorkerW`, bandeja) ficam de fora, como no Pestinha. O ícone do executável e da bandeja é o rosto da Sophia, com as cores de `pets/sophia.json`. Layout e rótulos foram reorganizados. Não há promessa de comparação pixel a pixel.
- **Mídia:** elementos básicos são desenhados em código. Memes/fotos são opcionais e fornecidos pelo usuário. GIF externo usa primeiro quadro.
- **Dados:** uma pasta própria do usuário substitui gravações ao lado do programa. Importação preserva os originais e exige destino vazio.
- **Plataformas:** a migração inicial verificou Windows e macOS só por checagem estática cruzada. Em 2026-11-02, um Windows 11 sem `link.exe` gerou e abriu o executável com o host Rust GNU e o GCC do MinGW: central, pet, bolinha e notebook ficaram visíveis e o processo respondeu. Isso ainda não comprova foco, DPI, transparência, atalho ou paridade de gestos. macOS usa a primeira janela normal visível como aproximação da atividade ativa e depende das permissões do SO. Wayland tem restrições de posicionamento/sensores.
- **Renderização:** brilhos, sinais, balística e a UI foram reexpressos em Rust. A equivalência de todos os gestos, tempos e detalhes visuais ainda exige teste de aceitação nativo.
- **Personagens:** Bia (vermelho-púrpura) e Sophia (roxo) são definições embutidas, com seed, temperamento e aparência próprios. A memória continua isolada por ID. A conversa também: cada perfil tem o próprio histórico, os próprios trechos importados e as próprias respostas marcadas com Gostei. Sophia, Bia, Bryan e Jorge usam timbres diferentes na voz local.
- **Linguagem:** a aba Conversa é opcional e não altera a rede pulsada. O modo local sobe o `llama-server` do llama.cpp apenas em `127.0.0.1`, com contexto 4096 e lote 256 para não reservar cache de GPU além do necessário. Ollama não é usado. O modo DeepSeek na NVIDIA, desligado por padrão, envia a conversa para a API hospedada `deepseek-ai/deepseek-v4.1-flash`. O checkpoint grande em disco não é carregado. A chave `nvapi-` é lida na hora e não é gravada.
- **Rede:** os grupos da referência de 470 neurônios crescem em 75%, com arredondamento para cima nas metades (845 neurônios). A simulação e os testes usam essa rede. A densidade de cada projeção permanece a da referência, então as sinapses acompanham o aumento dos grupos. Os pesos ficam numa lista esparsa; a matriz cheia não é alocada. Buffers de ruído e de corrente são reutilizados a cada passo. A habituação salva por neurônio deixa de caber na rede nova: um `nervous.json` antigo é rejeitado e o personagem recomeça a habituação, sem misturar tamanhos. O arquivo congelado de 470 neurônios não descreve mais a simulação.
- **Créditos da central:** o rodapé aponta a origem e os créditos para paulosantanna e o repositório MyDigitalBuddy. A atribuição do Pestinha permanece em NOTICE e em `--license`.

Os arquivos em `_reversa_sdd/` descrevem a extração e a intenção da migração. O relatório `VALIDATION.md` limita as afirmações à evidência realmente obtida.
