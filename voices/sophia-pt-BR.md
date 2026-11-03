# Sophia — amostra de voz (pt-BR)

A amostra `voices/sophia-amostra.wav` fala: «Oi! Eu sou a Sophia. Tudo bem sim. Pode falar comigo que eu guardo.» A voz é `pt-BR-ThalitaMultilingualNeural`, ritmo `+4%` (cerca de 10% mais rápido que o take anterior em `-6%`), volume `+10%`, tom de serviço `+8Hz`. O centro medido fica em 211 Hz e o topo da frase fica em 279 Hz.

Os pulsos não são deslocados. O corpo ganha calor em 220 Hz e 950 Hz; a articulação abre em 1,85 kHz; a faixa metálica estreita em 3,15 kHz desce 2 dB; a presença larga em 5,2 kHz sobe 2,4 dB. O chiado acima de 8 kHz desce 7 dB. A pausa entre as orações é a da própria frase.

Com o perfil **Sophia** selecionado, **toda** fala do buddy usa `voices/sophia-speak.py` e `sophia.voice.json` (Thalita + pós-processamento igual ao da amostra). Requisitos: Python 3, `edge-tts`, `numpy`, `ffmpeg` no PATH. Se a síntese falhar, cai na voz do Windows com o tom de reserva em `prosody("sophia")`. A amostra continua sendo a referência auditiva da saudação.
