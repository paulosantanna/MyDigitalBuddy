# Privacidade

MyDigitalBuddy funciona localmente. Não há telemetria nem download automático de modelos. A conversa nasce desligada.

Os adaptadores consultam nome do programa, cursor, tempo ocioso e geometria de janelas. Não consultam títulos, pixels, documentos, abas, sites, mensagens ou conteúdo de janelas. O atalho de pânico consulta a combinação de teclas, sem guardar sequência de entrada. A inferência de digitação vem de input recente com cursor parado.

A rotina é opcional. Quando ativada, grava categorias agregadas por minuto, frações de ociosidade/ausência/digitação, contagem de trocas de aplicativo, tempo de sessão, interações e emoção. O CSV não contém nomes de processos, títulos ou conteúdo. Desativá-la descarta o minuto ainda em RAM; apagar histórico remove o CSV e os acumuladores.

A memória por personagem guarda habituação, aprendizado, necessidades, condicionamento, protesto, fugas e até 500 episódios recentes. A conversa, quando você a usa, guarda o histórico, os trechos importados e as respostas marcadas com Gostei, na pasta daquele perfil. O Gostei não altera a rede pulsada. Os logs locais registram erros de armazenamento e configuração, sem títulos de janelas, sem conteúdo de trabalho e sem chaves. Configurações e painéis são locais.

O assistente local fala só com `127.0.0.1`, pelo llama.cpp. Ollama não entra. Microfone, leitura de PDF, texto e áudio também ficam na máquina. A voz sintetizada não sai do computador. Opus depende do ffmpeg, se ele já estiver instalado; wav, mp3, flac, ogg e m4a são lidos pelo programa.

DeepSeek na NVIDIA é outra permissão, desligada por padrão. Quando ligada, a conversa e o texto importado seguem para `https://integrate.api.nvidia.com`, modelo `deepseek-ai/deepseek-v4.1-flash`. A chave é lida de linhas que começam com `nvapi-` no arquivo configurado, ou da variável `NVIDIA_API_KEY`. Outras credenciais desse arquivo são ignoradas. A chave não é copiada para a memória, para o log nem para a interface.

É possível apagar o histórico e redefinir a memória na central. A redefinição afeta o personagem selecionado, inclusive a conversa dele; os demais permanecem. A pasta pode ser escolhida por `--data-dir`. Diretórios de memória não devem ser commitados ou publicados.

Links de documentação só abrem o navegador quando você clica. O sistema operacional pode exigir permissões próprias para os metadados e para o atalho global. Nenhuma permissão é concedida automaticamente pelo programa.
