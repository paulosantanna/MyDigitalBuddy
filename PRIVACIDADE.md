# Privacidade

MyDigitalBuddy funciona localmente. Não há endpoint de produto, telemetria, download de modelos ou envio de memória a terceiros.

Os adaptadores consultam nome do programa, cursor, tempo ocioso e geometria de janelas. Não consultam títulos, pixels, documentos, abas, sites, mensagens ou conteúdo de janelas. O atalho de pânico consulta a combinação de teclas, sem guardar sequência de entrada. A inferência de digitação vem de input recente com cursor parado.

A rotina é opcional. Quando ativada, grava categorias agregadas por minuto, frações de ociosidade/ausência/digitação, contagem de trocas de aplicativo, tempo de sessão, interações e emoção. O CSV não contém nomes de processos, títulos ou conteúdo. Desativá-la descarta o minuto ainda em RAM; apagar histórico remove o CSV e os acumuladores.

A memória por personagem guarda habituação, aprendizado, necessidades, condicionamento, protesto, fugas e até 500 episódios recentes. Os logs locais registram erros de armazenamento e configuração, sem títulos de janelas ou conteúdo de trabalho. Configurações e painéis são locais.

É possível apagar o histórico e redefinir a memória na central. A redefinição afeta o personagem selecionado; os demais permanecem. A pasta pode ser escolhida por `--data-dir`. Diretórios de memória não devem ser commitados ou publicados.

Links de documentação só abrem o navegador quando você clica. O sistema operacional pode exigir permissões próprias para os metadados e para o atalho global. Nenhuma permissão é concedida automaticamente pelo programa.
