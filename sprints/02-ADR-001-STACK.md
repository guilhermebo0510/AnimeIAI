# ADR-001 — Stack

Status: aceito (hipótese de trabalho; pode ser revisto durante a execução com registro em `registro/DECISOES.md`).

## Contexto

A pesquisa inicial (`anime-studio-pesquisa/00-RELATORIO-INICIAL.md` §6) recomendou **núcleo Rust + canvas nativo wgpu + casca web Tauri v2/WebView2 + TypeScript**. Desde então surgiram três fatos:

1. **O ANIGO, outro projeto do usuário, aposentou a versão Tauri/Svelte** e migrou para nativo egui 0.36 + wgpu 30 (~70 mil linhas no `anigo-studio`), com bridge de teste por INPUT/SCREENSHOT que funciona bem para o agente.
2. **O Smart Canvas (Tauri + viewport nativo)** acumulou problemas que vêm justamente da mistura WebView2 + superfície nativa: entrada por Raw Input fora do DOM, retângulo do viewport velho após minimizar, screenshot da WebView que não vê a superfície nativa, MCP que cai a cada recompilação.
3. **O operador principal é a IA, não um animador com caneta.** O risco da pesquisa (latência de caneta, texto vertical japonês com ruby) pesa menos; testabilidade por agente e reaproveitamento de código pesam mais. O usuário quer reaproveitar ANIGO e Smart Canvas e possivelmente unir com o ANIGO no futuro.

## Decisão

**Rust nativo de ponta a ponta na aplicação, com processos auxiliares para o que não é Rust:**

| Camada | Escolha | Observação |
|---|---|---|
| Linguagem | Rust (edição 2024, toolchain estável 1.96+) | Mesma do ANIGO/Smart Canvas |
| Janela e UI | `eframe`/`egui` (versão do ANIGO como base, 0.36; subir se houver release estável) + `egui_tiles` (docking) + ícones vetoriais próprios (SVG → textura) | Painéis, propriedades, timeline e timesheet desenhados em egui; canvas e viewports são texturas wgpu dentro do egui (sem "airspace") |
| GPU | `wgpu` 30.x (DX12/Vulkan), `PowerPreference::HighPerformance` | Compositor, pincéis, efeitos em WGSL |
| Vetor | `kurbo` (curvas, offset, simplificação) + tesselação própria de traço com largura variável; `vello`/`vello_cpu` avaliado na S03 como render de vetor e referência CPU | Decisão final do rasterizador vetorial na S03 com medição |
| Raster | Camadas em tiles (256×256, RGBA16F na GPU, RGBA8/16 no disco), motor de pincel por "dabs" em compute/fragment shader | Ver ADR-003 |
| Render de referência | Compositor CPU determinístico (mesmo grafo, em Rust puro) para golden tests e para o "olho" do agente | Pixel idêntico entre execuções |
| Texto | `egui` + fontes Noto Sans/Noto Sans JP embutidas; roteiro com formatação própria | Texto vertical japonês adiado (não é requisito do usuário) |
| Áudio | `cpal` (saída), `symphonia` (decodificação), `rustysynth` (SoundFont para BGM), DSP próprio (mixer, EQ, compressor, loudness BS.1770) | |
| Vozes | **VOICEVOX Engine** (processo local HTTP, gratuito, LGPL; vozes com termos por personagem) para japonês; **Kokoro ONNX** (via `ort` ou Python) para PT/EN | Sem PyTorch |
| Vídeo | FFmpeg como processo externo (já instalado; empacotar build LGPL só se for distribuir) | Leitura de vídeo de referência e exportação |
| 3D de guia | Renderer e utilidades do ANIGO (`anigo-renderer`, manequim/blocos) copiados/adaptados | Guias de ângulo e perspectiva, cenário em blocos |
| MCP | Binário `aiai-mcp` (stdio, JSON-RPC MCP) + bridge HTTP/NDJSON dentro do app; modo CLI `aiai-mcp call` | Padrão do ANIGO; ver ADR-004 |
| QA/análise | Python 3.13 via `uv` em `tools/` (numpy, scikit-image, opencv-python-headless, scipy); métricas críticas também em Rust (FLIP, SSIM, ΔE2000) | Reaproveitar `C:\ANIGO\scripts\look_calibration` |
| Persistência | Pasta `.aiai/` com `project.json` canônico (chaves ordenadas) + `blobs/` por BLAKE3 + WAL de recuperação | Ver ADR-002 |
| Testes | `cargo test` por crate, golden images CPU, `naga` para WGSL, suíte de integração via bridge, harness SendInput | |

### Crates do workspace (`app/crates/`)

| Crate | Papel |
|---|---|
| `aiai-core` | Documento, Command Bus, undo/redo, ids, validação, serialização canônica. Sem GPU, sem UI |
| `aiai-paint` | Motor de desenho: traço vetorial, pincéis raster, tiles, preenchimento, seleção, transformação (CPU, determinístico) |
| `aiai-render` | Compositor e efeitos: caminho GPU (wgpu) e caminho CPU de referência com o mesmo grafo |
| `aiai-anim` | Timesheet, exposição, interpolação, intercalação vetorial, câmera, keyframes |
| `aiai-fx` | Biblioteca de efeitos de satsuei, partículas, LUT/cor |
| `aiai-audio` | Mixer, loudness, TTS (cliente VOICEVOX/Kokoro), sintetizador SFX, sequenciador BGM, lip-sync |
| `aiai-media` | FFmpeg (import/export), PNG/EXR/PSD básico, sequências |
| `aiai-guide3d` | Guias 3D (manequim, blocos, câmera) com código do ANIGO |
| `aiai-metrics` | Métricas de imagem/áudio para o laço de calibração |
| `aiai-bridge` | Servidor local no processo do app (ações, auth por token, descoberta) |
| `aiai-mcp` | Servidor MCP stdio + CLI, fala com a bridge e também roda headless (render/verify sem app aberto) |
| `aiai-studio` | O aplicativo (eframe/egui), workspaces, painéis, ferramentas interativas |

## Alternativas

| Opção | Situação | Motivo |
|---|---|---|
| Tauri v2 + TS + canvas nativo (pesquisa) | **Descartada para este produto** | Problemas de airspace/entrada/captura vividos no Smart Canvas; ANIGO já saiu dela; duas linguagens e dois processos de build num PC com 8 GB |
| Rust + CEF em textura (estilo Graphite) | Adiada | Só faria sentido se precisássemos de UI web rica (ex.: editor de texto vertical japonês). Não é requisito agora |
| egui substituído por Xilem/Slint | Adiada | Reavaliar só se egui virar limitador (texto rico do roteiro é o candidato). A camada `aiai-core` não depende de UI, então trocar a casca é possível |
| Fork do OpenToonz | Descartada | C++/Qt 5.15, difícil de testar por agente; vale como oráculo de domínio (timesheet, paletas) |
| Blender como host | Descartada como host; **permitida como ferramenta externa opcional** | GPL e 3D-cêntrico. O 3D de guia vem do ANIGO |

## Consequências

- Uma linguagem, um build, um processo principal: menos RAM e disco.
- Toda a lógica de desenho fica em `aiai-paint`/`aiai-render`, testável sem janela, o que o agente precisa.
- Risco: egui para timeline/timesheet densos e editor de roteiro. Mitigação: widgets próprios pintados direto (como o ANIGO fez); medir na S12/S16.
- Risco: caneta real (pressão) no Windows via winit tem bug conhecido; o operador principal é o MCP, e o retoque humano usa mouse ou caneta via `WM_POINTER` próprio (adiado para depois do MVP, com registro).
- O que esta decisão **não** cobre: distribuição/instalador, licença de FFmpeg empacotado, texto vertical japonês. Ficam registrados como adiados.
