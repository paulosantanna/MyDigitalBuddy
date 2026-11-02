# Personagens e conteúdo local

Uma definição `pets/<id>.json` contém nome, seed, temperamento, pesos de brincadeira, reação ao notebook e aparência opcional. O ID aceita letras, números, hífen e sublinhado, sem caminhos. Sementes e memória são separadas por personagem.

Bryan, Jorge, Bia e Sophia estão embutidos no binário. Bia usa vermelho-púrpura; Sophia usa roxo. Definições ao lado do executável ou em `--content-dir` podem substituí-las; a validação exige ganhos e pesos finitos entre 0 e 10. Para um novo personagem, copie uma definição com outro ID, nome e seed.

Conteúdo opcional:

```text
pets/<id>/recadinhos.txt
pets/<id>/recadinhos_tristes.txt
pets/<id>/cartazes.txt
pets/<id>/memes/<emocao>/*.png
assets/comum/fotos/*.png
```

Bilhetes têm uma frase por linha; linhas vazias e comentários `#` são ignorados. Cartazes usam seções `[reclamacao]`, `[manifestacao]`, `[greve]`, `[revolucao]`, `[saudade]`, `[chorando]`, `[abandonado]` e `[acordo]`. Arquivos de texto são relidos durante o uso.

Imagens aceitas: PNG, JPEG, WebP, GIF e BMP, até 20 MiB por arquivo e dimensões limitadas antes de decodificar. O primeiro quadro de GIFs é exibido. Imagens não são distribuídas neste projeto; use conteúdo próprio ou com licença apropriada.
