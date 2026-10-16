# S02 — Bridge, servidor MCP, harness de mouse, render headless

## Objetivo
A partir daqui o agente opera o app só por ferramentas próprias. Implementar o ADR-004: bridge dentro do app, `aiai-mcp` (stdio + CLI + modo headless), harness SendInput, cliente Python, registro do MCP na sessão do Claude Code.

## Tarefas
- [ ] Copiar/adaptar do ANIGO: `anigo-bridge` (servidor, auth por token, limites, descoberta, protocolo NDJSON + HTTP mínimo) → `aiai-bridge`; `anigo-mcp` (stdio JSON-RPC, cliente da bridge, CLI `bridge-call`, sandbox de pastas) → `aiai-mcp`. Renomear, remover o que é 3D, manter os testes.
- [ ] `StudioApp::bridge_tick()` no início de cada quadro; ações do ADR-004 §2: PING, GET_STATUS, GET_METRICS, APPLY_COMMAND, QUERY, UNDO, REDO, GET_LOG, SCREENSHOT, INPUT, GET_UI_STATE, UI_ACTION, NEW/OPEN/SAVE_PROJECT. RENDER e COMPARE entram com stub honesto ("não implementado ainda", erro claro) e são completados na S03/S04.
- [ ] SCREENSHOT: lê o framebuffer do egui (janela inteira ou retângulo de painel) e grava PNG em pasta permitida.
- [ ] INPUT: eventos de mouse (move, down/up, wheel), teclado (tecla, texto), caneta simulada (pressão) injetados no `raw_input_hook` do egui, em pontos lógicos.
- [ ] GET_UI_STATE: workspace, painéis com retângulos (lógicos e físicos), ferramenta ativa, foco, diálogos, escala de DPI, tamanho da janela.
- [ ] `aiai-mcp`: ferramentas genéricas (`aiai_apply`, `aiai_query`, `aiai_render`, `aiai_screenshot`, `aiai_input`, `aiai_ui`, `aiai_compare`, `aiai_undo`, `aiai_redo`, `aiai_log`, `aiai_project`, `aiai_status`); imagens devolvidas como conteúdo de imagem MCP (redimensionadas) e gravadas em disco quando pedido.
- [ ] Modo headless do `aiai-mcp`: abrir projeto, aplicar comandos e renderizar sem app (usado nos testes e quando o app estiver recompilando).
- [ ] CLI: `aiai-mcp call <AÇÃO> '<json>' [--png out.png]` para uso pela Bash.
- [ ] `.mcp.json` na raiz do projeto registrando `aiai-mcp` (caminho do binário release em `app/target/release/` para não travar com o build debug do app).
- [ ] `tools/harness/input.ps1` + `watch_input.ps1` + `screen.ps1` (copiar o harness do Smart Canvas `09-parede-sprints/harness-input/` e adaptar o título da janela; aplicar todas as lições 15–23).
- [ ] `tools/aiai.py`: cliente Python da bridge (lotes, scripts de calibração).
- [ ] Script `tools/relanca.ps1`: fecha a árvore do app, compila, abre com `--bridge --no-session`, espera o `bridge.json` e o PING.

## Testes
- Suíte de integração da bridge (um binário `tests/suite.rs`): auth, ação desconhecida, parâmetros inválidos, apply+undo, screenshot com tamanho certo, INPUT muda o estado esperado (ex.: trocar workspace clicando no seletor).
- Teste de fumaça por mouse real: focar app, clicar no seletor de workspace, conferir por GET_UI_STATE; watch_input sem eventos físicos.
- `aiai-mcp` stdio: initialize → tools/list → tools/call de cada genérica.

## Critério de pronto
- [ ] Nesta sessão (ou numa nova, se exigir reinício) o Claude chama `aiai_status`, cria um corte com `aiai_apply`, desfaz, e tira screenshot pela ferramenta MCP.
- [ ] Mesmo fluxo pela CLI `aiai-mcp call` com o app recompilando no meio (sobrevive ao reinício).
- [ ] Clique real por SendInput trocando workspace, com captura antes/depois.
- [ ] Evidências em `registro/evidencias/S02/`.
