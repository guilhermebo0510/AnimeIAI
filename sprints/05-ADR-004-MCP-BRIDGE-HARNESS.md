# ADR-004 — Como a IA enxerga e opera o programa (MCP, bridge, harness)

Status: aceito. Implementado na S02 e estendido em todas as sprints seguintes.

## 1. Arquitetura

```
 Claude Code (sessão)
    │ stdio JSON-RPC (MCP)                      │ Bash: aiai-mcp call <AÇÃO> '<json>' [--png out.png]
    ▼                                           ▼
 aiai-mcp  ──────────────────────────────────────┘
    │  modo A: fala com o app aberto  → HTTP/NDJSON 127.0.0.1:<porta>, token (descoberta em %LOCALAPPDATA%\AnimeIAI\bridge\bridge.json, relida a cada chamada)
    │  modo B: headless               → abre o projeto .aiai direto com aiai-core/aiai-render (render, verificação, métricas sem janela)
    ▼
 aiai-studio --bridge
    aiai-bridge (threads de conexão, auth, limites) ─► fila limitada ─► StudioApp::bridge_tick() no início de cada quadro (thread da UI)
        └─ CoreSession.apply/undo/redo · consultas · render · SCREENSHOT do framebuffer · INPUT injetado no egui · estado da UI
```

Padrão copiado/adaptado do ANIGO (`crates/anigo-bridge`, `crates/anigo-mcp`, `docs/native/MCP_BRIDGE_NATIVA.md`). Vantagens comprovadas: o MCP sobrevive a recompilação/reinício do app; tudo que o MCP faz entra no undo e aparece ao vivo na janela.

## 2. Ações da bridge (núcleo; cada sprint acrescenta as suas)

| Ação | Função |
|---|---|
| `PING`, `GET_STATUS`, `GET_METRICS` | Saúde, fps real, GPU, projeto, histórico |
| `APPLY_COMMAND` (`command`, `if_revision`) | Qualquer comando do Command Bus (inclusive `batch`) |
| `QUERY` (`query`) | Qualquer consulta (documento, camada, cel, timesheet, paleta, seleção) |
| `UNDO`, `REDO`, `GET_LOG` | Histórico |
| `RENDER` (`cut`, `frame`, `range`, `size`, `backend: cpu|gpu`, `layers`, `out`) | Render real do corte (PNG/sequência) |
| `SCREENSHOT` (`region`, `panel`, `out`) | Captura da janela/painel (framebuffer do egui) |
| `INPUT` (eventos de mouse/teclado/caneta em pontos lógicos) | Injeção dentro do egui, sem tocar no cursor do SO |
| `GET_UI_STATE` | Workspace, painéis e seus retângulos, ferramenta ativa, diálogos abertos, estouros de layout |
| `UI_ACTION` | Trocar workspace, abrir painel, escolher ferramenta, abrir diálogo |
| `COMPARE` (`a`, `b`, `metrics[]`, `mask`, `out_diff`) | Métricas de imagem (ver protocolo de validação) |
| `OPEN_PROJECT`, `SAVE_PROJECT`, `NEW_PROJECT` (caminho explícito, sem diálogo do SO) | Arquivo |

## 3. Ferramentas MCP

Duas camadas:
1. **Genéricas e estáveis** (não mudam, então não exigem reiniciar a sessão): `aiai_apply` (comando), `aiai_query`, `aiai_render`, `aiai_screenshot`, `aiai_input`, `aiai_ui`, `aiai_compare`, `aiai_undo/redo/log`, `aiai_project`.
2. **Curadas, de alto nível**, para o agente desenhar e produzir com menos tokens: `aiai_draw_curve`, `aiai_draw_shape`, `aiai_fill`, `aiai_paint_stroke`, `aiai_set_exposure`, `aiai_camera_key`, `aiai_add_effect`, `aiai_tts_line`, etc. Cada sprint lista as suas. Uma ferramenta curada é sempre um açúcar sobre comandos do Command Bus (nada de caminho paralelo).

O catálogo de comandos é autodescrito: `aiai_query {"kind":"command_schema"}` devolve o JSON Schema de todos os comandos, para o agente descobrir comandos novos sem reiniciar a sessão.

## 4. Harness de mouse e teclado reais

- `tools/harness/input.ps1` (SendInput, DPI-aware, BOM UTF-8): `foco`, `max`, `cap <arquivo>`, `move x y`, `click x y`, `dclick`, `drag x1 y1 x2 y2 passos`, `wheel n`, `key <combo>`, `type <texto>`. Coordenadas físicas; retângulos dos painéis vêm do `GET_UI_STATE` (convertidos de pontos lógicos para físicos pela escala real).
- `tools/harness/watch_input.ps1`: hook `WH_MOUSE_LL` que marca a corrida como contaminada se aparecer evento físico do usuário.
- Antes de qualquer clique: focar a janela pelo título "AnimeIAI", maximizar, capturar e conferir a barra de título.
- Uso: validar que a ferramenta funciona pelo caminho do usuário (clicar no ícone, arrastar no canvas, mexer no slider, arrastar painel, atalhos). Toda sprint com UI tem um roteiro de teste por mouse.

## 5. Ferramentas de QA em Python (`tools/`)

- `tools/aiai.py`: cliente da bridge (para lotes e scripts de calibração).
- `tools/metrics/`: análise e comparação de imagens (reaproveitar `C:\ANIGO\scripts\look_calibration`), extração de paleta, mapa de bordas, relatório lado a lado em HTML/PNG.
- `tools/ref/`: extração de frames e áudio do episódio de referência para `calibracao/`, detecção de cortes, timing de exposição (quais quadros mudam).

## 6. Segurança e limpeza

- Bridge só com `--bridge` e só em 127.0.0.1, token por sessão.
- Testes abrem o app com `--no-session` ou `--session-file` no scratchpad para não sujar o layout salvo do usuário.
- O MCP só grava arquivos em pastas permitidas (`AIAI_MCP_ALLOWED_DIRS`: projeto, `calibracao/`, `producoes/`, `sprints/registro/evidencias/`, scratchpad).
