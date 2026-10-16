# S00 — Fundação do workspace, janela, tema, i18n, guardas de disco

## Objetivo
Ter o repositório, o workspace Cargo e um app egui que abre com identidade provisória, tema claro/escuro, PT/EN, docking e workspaces vazios, com os perfis de build e as proteções de disco certos desde o primeiro dia.

## Pré-requisitos
Ler `00-*`, `01`, `02`. Disco ≥ 30 GB livres para o primeiro build completo.

## Tarefas
- [x] `git init` em `C:\AnimeIAI` (sem remoto) e `.gitignore` — feito no planejamento (2026-10-15). Conferir e ampliar o `.gitignore` quando surgirem pastas novas.
- [x] Commit inicial com pesquisa e sprints — feito no planejamento.
- [ ] Estrutura de pastas do runbook §7.
- [ ] `app/Cargo.toml` workspace com os crates do ADR-001 (vazios compilando), `resolver = "3"`, dependências comuns em `[workspace.dependencies]` com versões fixas (partir do `Cargo.lock` do ANIGO: egui/eframe 0.36, egui_tiles 0.17, wgpu 30.0.1, glam, serde, image; conferir releases mais novos e registrar a escolha).
- [ ] Perfis: `[profile.dev] opt-level=1, debug="line-tables-only"`; `[profile.dev.package."*"] opt-level=3, debug=false`; `[profile.test] debug=false`; `[profile.release] lto="thin"`. Não mexer de novo sem 30 GB livres.
- [ ] `app/.cargo/config.toml`: `CARGO_INCREMENTAL=0` para test; `jobs = 4` (RAM).
- [ ] `tools/disk_guard.ps1`: mostra espaço livre, tamanho de `target/`, e limpa na ordem das lições (`pdb` → duplicatas → `incremental`). `tools/build.ps1` chama o guard antes e recusa build com < 8 GB.
- [ ] `aiai-studio`: janela eframe (wgpu, HighPerformance), título "AnimeIAI", ícone provisório, barra de menu, barra de status (fps real, projeto, memória), docking com `egui_tiles`, seletor de workspaces (Roteiro, Personagens, Storyboard, Layout, Animação, Pintura, Cenário, Composição, Cor, Áudio, Edição, Entrega, Produção) — cada um com layout de painéis próprio, ainda vazios.
- [ ] Tema: tokens de cor para claro e escuro (inspirado em Blender/DaVinci, sóbrio), tipografia (Noto Sans + Noto Sans JP + mono), alternância em tempo real; persistência das preferências fora do projeto (`%APPDATA%\AnimeIAI`).
- [ ] i18n: chaves PT/EN em arquivos `.ftl` ou tabela Rust (reaproveitar o esquema do ANIGO `i18n*.rs`); troca em tempo real; teste que falha se uma chave faltar em um idioma.
- [ ] Ícones: pipeline SVG → textura (resvg/usvg) com conjunto inicial (ferramentas básicas) desenhado do zero; sem emoji. Phosphor só como provisório se necessário (licença MIT), substituído na S20.
- [ ] CLI: `aiai-studio --no-session`, `--session-file <f>`, `--project <pasta>`, `--bridge` (ligado na S02).
- [ ] `tools/` com `uv` (pyproject, Python 3.13): numpy, pillow, scikit-image, opencv-python-headless, scipy, scikit-learn.

## Testes
- `cargo test -p aiai-studio` (i18n completo, tokens de tema válidos, layout de workspaces carrega).
- Abrir o app, capturar a tela pelos dois temas e dois idiomas (harness de captura da S02 ainda não existe: usar captura PowerShell DPI-aware `tools/harness/screen.ps1`, criada aqui).
- Mouse: trocar workspace, arrastar um divisor de painel, alternar tema pelo menu.

## Critério de pronto
- [ ] Repositório com commit inicial; build limpo medido (tempo e tamanho de `target/` registrados no ESTADO).
- [ ] 4 capturas (claro/escuro × PT/EN) em `registro/evidencias/S00/`.
- [ ] Teste por mouse feito e registrado.

## Riscos
Primeiro build completo de wgpu+egui pode passar de 10 GB e 10 min neste PC. Rodar com `-j 4` e acompanhar o disco.
