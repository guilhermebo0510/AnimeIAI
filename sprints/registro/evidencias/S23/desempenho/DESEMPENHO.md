# S23 — desempenho medido (2026-10-26, retomada automática das 23:27)

## 1. Render por quadro, headless (feito)

- Comando: `python tools/desempenho/render_por_quadro.py <cópia do curta>.aiai sprints/registro/evidencias/S23/desempenho/render_por_quadro.json`
- Projeto: uma cópia do 「最後の一輪」 (`producoes/micro-anime/ultima-rosa.aiai`) na pasta temporária. O projeto versionado não foi aberto.
- Binário: `app/target/release/aiai-mcp.exe` (26/10 11:42), render CPU 1920×1080 com câmera, efeitos e grade. Os PNGs em `_quadros/` foram conferidos à vista (ex.: `c6_f66.png`, céu com pétalas e raios).
- 7 cortes × início/meio/fim = 21 quadros. Cada chamada abre o projeto do zero; a linha de base (QUERY stats, mediana 682 ms) é descontada.

| | ms por quadro (render estimado) |
|---|---|
| mediana | **618** |
| mínimo | 279 (corte 2, quadro 0) |
| máximo | 1137 (corte 7, quadro 66: céu + partículas) |

Leitura: o curta inteiro (33,2 s = 797 quadros) leva da ordem de 8 min só de render na CPU. A linha de base inclui ler o projeto e reaplicar 653 linhas do diário. Medido com outra sessão do Claude e o After Effects abertos na mesma máquina (lição 65), então os números são de pior caso.

Medida anterior (19/10, `bench4k.json`): 4K com 24 camadas, 1437 ms no render inteiro e 375 ms com cache depois de uma edição.

## 2. FPS do canvas e da reprodução no app (NÃO medido; bloqueado)

Tentativas, todas com `aiai-studio.exe` release (recompilado em 26/10 21:53):
- `--bridge --project <cópia>`, `--bridge --no-session`, `--no-splash`: a janela fecha sozinha de 5 a 10 s depois de abrir, com saída 0.
- O log `eframe=debug` mostra `Received WindowEvent::CloseRequested`: o pedido vem de fora (o Windows manda WM_CLOSE). Não é queda nem perda de GPU, e não ficou marcador em `logs/`.
- Prova de que não é o app: uma janela WinForms neutra (`form.ps1`, título "Neutra" e outro "AnimeIAI") também foi fechada uns 10 s depois de abrir, com `CloseReason=UserClosing`.
- Na mesma hora havia outra sessão do Claude Code ativa (pid 25976, com computer-use) trabalhando em `C:\VIDEO\projeto`, com o After Effects aberto. A explicação provável é que essa sessão manda teclas ou cliques para a janela da frente.

Decisão: não disputar mouse e teclado com outra sessão do usuário. A medida de FPS e o ensaio do zero com mouse ficam para uma retomada em que a área de trabalho esteja livre. Antes de abrir o app, conferir com `Get-CimInstance Win32_Process` se há outro `claude.exe` com bash ativo fora de `C:\AnimeIAI`.

Como medir quando estiver livre: abrir com `--bridge --no-session --project <cópia>`. Para a reprodução, dar Play pelo `UI_ACTION` e ler `GET_UI_STATE.playback.fps_real`. Para o canvas, manter algo animando (lição 28) e ler `GET_STATUS.fps`/`frame_ms` a cada segundo por 30 s.
