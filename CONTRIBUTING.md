# Contribuir

Use a toolchain indicada em `rust-toolchain.toml`. Antes de enviar mudanças, rode fmt, Clippy e os testes, com e sem a feature `desktop`. Alterações numéricas devem preservar os testes de caracterização ou declarar uma mudança de comportamento com evidência.

`src/core/` não deve depender de egui, APIs do SO ou acesso a disco. `Engine` compõe estados e comandos; `platform/` é a borda de leitura nativa e `storage` é a borda de persistência. Testes têm diretórios temporários próprios e nunca usam memória pessoal.

Não adicione coleta de títulos, conteúdo de tela, texto digitado, chamadas de rede ou telemetria como efeito colateral de uma mudança. Preserve os limites de permissões antes de decisões aprendidas e a persistência do pânico.

Inclua o sistema, escala/DPI, backend X11/Wayland e passos de reprodução em bugs visuais. Não anexe sua pasta de memória, capturas privadas ou logs contendo informações pessoais.

Contribuições ao código derivado mantêm GPL-3.0-or-later. Conteúdo opcional deve ter origem e licença compatíveis documentadas.
