# Status da AUDITORIA.md (primeira auditoria) contra o código atual

Conferido em leitura (sem compilar nem abrir o app) no HEAD `9fbea5f` mais a árvore de trabalho de 2026-10-26. Base anterior: `sprints/registro/S23-AUDITORIA-BACKLOG.md`, que foi relida e **reconferida no código**; onde o backlog dizia "feito" e o código confirmou, a evidência é do código. `AUDITORIA_V2*` e `auditoria-v2/` ignorados.

Legenda: **FEITO**, **PARCIAL**, **NÃO FEITO**, **VERIFICAR** (precisa rodar o app ou ter mesa/hardware). "Em andamento" = código existe só na árvore de trabalho (arquivos novos ainda não commitados da S25: `figure.rs`, `gradient_tool.rs`, `line_fix.rs`), não conta como FEITO.

## Resumo

Itens contados: 121 (cada afirmação/pedido da auditoria, abrindo os itens do Plano de Ação da Parte 5 e os achados das Partes 1 a 4).

| Status | Qtde |
|---|---|
| FEITO | 56 |
| PARCIAL | 15 |
| NÃO FEITO | 47 |
| VERIFICAR | 3 |

Os 5 itens P0 do plano da auditoria estão feitos (um deles com decisão mantida: "fechar ao voltar" continua ligado). Do P1 (6 a 10) falta pouco: WinTab/inclinação/borracha da caneta, botão de BGM, Importar PSD, seleção por faixa de cor, mover com perspectiva/malha. O P2 (11 a 16) está quase todo aberto, exceto onion skin, réguas, 3D, criador/importador de pincéis, mais o que a S24/S25 entregou por outro caminho.

## Tabela

### Parte 1 (motor)

| # | Item | Status | Evidência | O que falta |
|---|---|---|---|---|
| 1.1 | Balde: região estreita fechada sem rótulo; `label_near` pula a linha | FEITO | `aiai-paint/src/fill.rs:143` (BFS só por pixels de linha), teste `fill.rs:407`; commit 5b68146 | nada |
| 1.2a | Lint ignora regiões que tocam a borda | FEITO | `aiai-tools/src/fill.rs:300-438` (`border_unpainted`, `include_border`) | o botão "Verificar" da UI não mostra `border_unpainted` (ver 2.7) |
| 1.2b | Lint testa um pixel só | FEITO | `aiai-tools/src/fill.rs:424` (cobertura ≥ 0,95 por pixel) | nada |
| 1.3 | Vetor rasterizado antes da câmera/escala | FEITO | `aiai-render/src/compose.rs:786-790`; teste `golden.rs:292`; commit 2f548b6 | nada |
| 1.4 | Pincel/balde O(W×H) | FEITO | tiles tocados (0de0ec5, 54→16 ms), cache de regiões do balde (2º clique 1007→6 ms), `FrameCache` (2e492d2, 450-950→40-70 ms) | metas da auditoria (<16 ms, <8 ms) não medidas (ver 4.4) |
| 1.5a | `inbetween`: sem grupos, traços sem par aparecem/somem em t=0,5 | NÃO FEITO | `aiai-anim/src/lib.rs:245,309-314`; `LACUNAS.md` nº 14 e 15 abertos | correspondência por grupo/tag, 1-para-N, suavizar sem par |
| 1.5b | Manequim `guide3d` sem plano anatômico nem projeção das juntas no JSON | PARCIAL | S24 trouxe manequim posável novo (`aiai-3d-charbase`, df4da70) | `GUIDE3D` continua sem `project` de pontos (`LACUNAS.md` nº 10) |
| 1.6 | Mover seleção parcial vetorial apaga os outros traços | FEITO | não era bug (`ReplaceStrokes` troca por id, `aiai-core/src/command.rs:509`); critério unificado em `transform.rs:145` (maioria dos pontos); teste `aiai-tools/tests/suite.rs:246` | nada |
| 1.7A | Prévia do arrasto diferente do traço final | FEITO | prévia do motor: `live.rs` (`StrokeBuffer`), `canvas.rs:1526-1549`; U7 do backlog | sobra uma `polyline` só como reserva antes da 1ª prévia chegar |
| 1.7B | Cauda do estabilizador salta ao soltar | PARCIAL | mantido por decisão (backlog 1.7B) | decisão do usuário se quer rever |
| 1.7C | Spline do pincel uniforme (overshoot) | FEITO | `aiai-tools/src/paint.rs:186-242` (centrípeta), teste `:288` | nada |
| 1.7C2 | Desligar `pen_autoclose` por padrão | NÃO FEITO | `editor.rs:568` (`true`); backlog "mantido" (o quadrado à mão vazava no balde) | decisão do usuário |
| 1.7D | `Ghost` congelado 170-336 ms | FEITO | prévia fica até o render chegar; render de 40-70 ms (2e492d2) | nada |
| 1.8a | Aquarela: pigmento na borda de fora | FEITO | `aiai-paint/src/brush.rs:1920-1940`; commit 5e55710, prancha `evidencias/S23/auditoria/pinceis_prancha.png` | conferir a prancha a olho se o usuário ainda achar estranho |
| 1.8b | Borrador sem pegada 2D e sem ver o traço atual | FEITO | `brush.rs:1769` (`smudge_carry`); commit 5e55710 | nada |
| 1.8c | Guache seco sem suavização do ângulo | FEITO | commit 5e55710 (passa-baixa na direção) | nada |
| 1.8d | Rosas (1 cor), Nuvem (escamas), Folhagem/Grama sem duas cores | FEITO | `TwoTone`, `brush.rs:184,683-725`; commit 92dc6bb | nada |
| 1.8e | Pressão fixa 1,0 no mouse, sem simulação por velocidade | PARCIAL | `brush_dyn.rs` tem dinâmica por velocidade/pressão/inclinação e as canetas S25 usam (2472df3, 63210d9, mudanças ainda não commitadas em `brush_dyn.rs`/`brush.rs`) | `Marker`/`Pencil` vetoriais seguem tubo uniforme com mouse; curva de entrada/saída para mouse no vetor |

### Parte 2 (UI/UX e ferramentas)

| # | Item | Status | Evidência | O que falta |
|---|---|---|---|---|
| 2.1 | "Começo rápido" sem camadas | FEITO | `commands.rs:320` (Fundo, Cor ligada à Linha, Linha, cel 1) | nada |
| 2.2 | Roda de cor fixa; popup cobrindo Camadas | FEITO | `panels/colors.rs` (`ColorTab::Wheel`, seletor próprio, U8) | nada |
| 2.3 | Painel de pincel só com Preset, Tamanho, Opacidade | FEITO | `panels/tool_props.rs`, `properties.rs:155`, `brush_maker.rs`; "Mostrar todos os ajustes" (U14) | nada |
| 2.4 | Miniaturas de traço dos pincéis | FEITO | `panels/brushes.rs:429` (traço do motor), painel Ferramenta em grade (U13, U20, U24) | nada |
| 2.5 | Caneta: reta, curva por cliques, retângulo, elipse | FEITO | commit 3289b64; `figure.rs` ampliando (em andamento) | polígono, Bézier, laço (em andamento) |
| 2.6 | Caneta: edição de nós e largura | FEITO | ferramenta Editar pontos (`ToolId::Points`, `nodes.rs`), alça U25, ids U27 | nada |
| 2.7 | Caneta: `color_trace` fixo `false` na UI | NÃO FEITO | `pen.rs:230`, `editor.rs:1208`, `anim.rs:781` | opção/caneta Iro-Tresse |
| 2.8 | Borracha vetorial apaga o traço inteiro | FEITO | modos trecho tocado, até a interseção, traço inteiro (`shelf.rs` `EraserVector(EraseMode)`); commit c572538, `aiai_erase` | nada |
| 2.9 | "Verificar" só dá um número, não destaca onde | NÃO FEITO | `tools.rs:750-764` (`run_lint` só guarda a contagem; ignora `border_unpainted`) | marcas piscantes sobre `LintRegion.point`, navegar de buraco em buraco, laço de preencher buracos |
| 2.10 | Onion skin / mesa de luz temporal | FEITO | `panels/light_table.rs`, `anim.rs` (`OnionMode`), commit 27a1e09 | nada |
| 2.11 | Seleção: Varinha | FEITO | commit 3a3d923; `selection.rs` | nada |
| 2.12 | Seleção: faixa de cor (`ColorRange`) | NÃO FEITO | nenhum uso em `aiai-studio` | expor na Seleção |
| 2.13 | Seleção: somar/subtrair/interseção, feather, inverter, expandir | FEITO | `selection.rs:65-195`, `sel_ops.rs` | nada |
| 2.14 | Seleção limita pincel e borracha raster | FEITO | commit 8316477 | caneta vetorial não respeita; balde não respeita (`FillRequest` não tem máscara) |
| 2.15 | Seleção limita balde e caneta | NÃO FEITO | `aiai-tools/src/fill.rs:18-48` sem seleção; S24 lista "balde ainda não respeita" | máscara de seleção no balde e na caneta |
| 2.16 | Mover: alças de escala/rotação, preview raster | FEITO | commits 3a3d923, 72f10e2, 3a6c235 (Ctrl+T 8 alças, prévia ao vivo `move_live.rs`) | nada |
| 2.17 | Mover: inclinar, perspectiva, malha, liquify, puppet warp | NÃO FEITO | `selection.rs:645` só `Deform::Affine`; motor tem `aiai-paint/src/warp.rs` | expor `Perspective`, `MeshWarp`, `Liquify` na UI |
| 2.18 | Conta-gotas: cor da camada/composta, lupa, preservar paleta | NÃO FEITO | `tools.rs:768-773` só `shown.rendered`, grava cor direta | opção "só camada atual", lupa, manter `ColorRef::Palette` |
| 2.19 | Rotação livre do canvas (não só saltos de 15°) | PARCIAL | `panels/navigator.rs:116-128` (controle deslizante + passo configurável nas Preferências) | gesto contínuo Shift+Espaço+arrastar não confirmado |
| 2.20 | `Layout`, `Pintura`, `Cenário` com painéis idênticos | FEITO | teste `layout.rs:1066` (`drawing_workspaces_no_longer_look_the_same`); réguas e Sub View nesses workspaces | Céu e Texturas em perspectiva ainda sem painel (ver 4.5A) |
| 2.21 | Personagens: desenhar na folha dentro da aba | FEITO | commit 7e5aba0 (pasta do personagem com canvas) | nada |
| 2.22 | `CONSISTENCY` digitando Y em pixels | NÃO FEITO | `panels/characters.rs:620-656` ainda `DragValue` | régua arrastável no canvas ou medição automática pela caixa dos traços |
| 2.23 | Cor: monitor de áudio sob "Navegador"; coluna central desperdiçada | FEITO | o workspace "Cor" virou Biblioteca (`workspace.rs:19`); monitor só no Áudio (`app.rs:1437`) | nada |
| 2.24 | Cor: Lift/Gamma/Gain em caixas numéricas, sem rodas/curvas/escopos | NÃO FEITO | `panels/satsuei.rs:104` (`DragValue`) | rodas 3-way, curvas RGB, escopos (S24 deixou "Escopos" para depois) |
| 2.25 | Áudio: timeline mostrando camadas de desenho | VERIFICAR | `layout.rs:1056` põe `Timeline` no Áudio; só abrindo o app se ela mostra trilhas de áudio | olhar o workspace Áudio |
| 2.26 | Edição sem Play/Pause | FEITO | `panels/edit_panels.rs:44,206` (Play/Pausa com som); commit 085c524, 8c624e9 | nada |
| 2.27 | Composição: parâmetros compostos "Editável via MCP" | PARCIAL | `satsuei.rs:117` ainda mostra para o que não é número/cor/vetor | editor de curvas/listas |
| 2.28 | Miniaturas em branco dos cortes C005-C007 na Produção/Edição | VERIFICAR | S24 reescreveu Produção e Edição (d1d34fb, 8c624e9) | abrir o projeto e olhar |

### Parte 3 (micro-anime)

| # | Item | Status | Evidência | O que falta |
|---|---|---|---|---|
| 3.1 | Model sheet: manequim 90° com desenho de costas | PARCIAL | `tools/curta/c02_sheets.py:5` `VIEWS=[0,30,180]` agora coerente com o desenho | vista de perfil (90°) e 3/4 costas (135°) ainda sem desenho (rig sem perfil) |
| 3.2 | `hair_ren` com `rot=False` descola do rosto em 3/4 | NÃO FEITO | `tools/curta/rig.py:284` todos `False`; backlog "pendente" | girar a silhueta lateral com o rosto |
| 3.3 | `body_bust` não gira em 3/4 | NÃO FEITO | `rig.py` comentário "não gira com a cabeça"; backlog "pendente (rig)" | ombros/avental/bolso assimétricos, tronco que gira |
| 3.4 | Gambiarra de `curta.py`/`c21_sobras.py` que pinta frestas com cor vizinha | FEITO | `tools/curta/curta.py:70-99` usa `skip_seeds_on_line`/`only_unpainted`/`close_leftovers`; `c21_sobras.py` removido; repintura `c22_repintura.py` | nada |
| 3.5 | Teste de consistência comparando 0,60 com 0,60 | PARCIAL | `c20_revisao.py` agora mede o render do corte e do sheet (backlog "Curta") | a ferramenta `CONSISTENCY` do motor ainda recebe números digitados (ver 2.22) |
| 3.6 | Corte C999/Ensaios no projeto de produção | FEITO | sem `C999` em `producoes/micro-anime/ultima-rosa.aiai/project.json` | nada |
| 3.7 | Atmosfera inicial em 3 cortes (jardim, fuurin, ninho) | NÃO FEITO | só `C001` | roteiro/produção, não ferramenta |
| 3.8 | Cena C002: braços rígidos como tábuas, sem flare no toque | NÃO FEITO | `c13_c002.py` ainda `offset_x` | poses-chave de mão e `LensFlare` no contato |
| 3.9 | Satsuei nos personagens (RimLight, LightWrap, Para) e sombras faciais | NÃO FEITO | nenhum uso nos `c0x`/`c1x` (só no C007) | aplicar o que o `aiai-fx` já tem |
| 3.10 | Blush, brilho de olho, respiração | NÃO FEITO | backlog "pendente" | camada de blush no rig, partículas de brilho |
| 3.11 | Dissolve do C005 para C006 com dupla exposição | NÃO FEITO | `c17_edicao.py:12` | fade só do fundo (Edição v2 pode ajudar) |

### Parte 4 (engenharia)

| # | Item | Status | Evidência | O que falta |
|---|---|---|---|---|
| 4.1a | Pressão: 1º ponto do traço começa em 1,0 | FEITO | commit 63b8cd2 (Touch Start do mesmo quadro), `canvas.rs` | nada |
| 4.1b | Curva global de pressão | FEITO | `prefs.rs:32-33`, `canvas.rs:533`; commit c1857cb | nada |
| 4.1c | WinTab / `WM_POINTER` nativo | NÃO FEITO | `Cargo.toml` do studio sem `Win32_UI_Input_Pointer`; só o Windows Ink que o winit entrega | WinTab para quem desliga o Windows Ink; precisa de mesa para validar |
| 4.1d | Inclinação da caneta (`tilt`) | NÃO FEITO | `tools.rs:229,371` `tilt: None`; o motor (`brush_dyn.rs:100`) já tem a dinâmica | ler a inclinação do evento e passar |
| 4.1e | Borracha traseira e botões da caneta | NÃO FEITO | nenhum `eraser`/`barrel` | precisa de mesa para validar |
| 4.1f | Coalescência > 60 Hz | NÃO FEITO | nenhum | `GetPointerPenInfoHistory` ou equivalente |
| 4.2a | Importar imagem (PNG, JPEG, WebP, TIFF, BMP) + arrastar e soltar | FEITO | `app/Cargo.toml:24`; `menu.file.import_image` (`app.rs:630`); commit 9d3e5b2 | nada |
| 4.2b | Painel Sub View | FEITO | `panels/subview.rs`; commit b557a6a | nada |
| 4.2c | Exportar PSD, SVG, XDTS, OTIO, quadro | FEITO | `app.rs:634-651`; commits 9273374 e seguintes | nada |
| 4.2d | Importar PSD/ORA, exportar ORA | NÃO FEITO | nenhum | leitor de PSD/ORA |
| 4.2e | Importar 3D (.obj, .gltf, .glb) | FEITO | `aiai-3d-model/src/import.rs:37` | `.fbx` não; `.vrm` responde "em breve" (`import.rs:79`) |
| 4.3a | Autosave antes do primeiro Salvar | FEITO | `app/autosave.rs` (recovery a cada 2 min, intervalo nas Preferências) | nada |
| 4.3b | `sync_all` por tile travando 10 s | FEITO | `persist.rs:278` (blobs em paralelo, WAL sem sync por tile); commit fc17a74, eeadec5 | nada |
| 4.3c | Recuperação de queda: undo perdido, `mark_saved` mentiroso, WAL ignorado | FEITO | `doc.rs:183-196` (avisa `wal_replayed`, fica "não salvo") | nada |
| 4.3d | Undo fixo em 500 e `log` ilimitado | FEITO | `session.rs:72` (`LOG_KEEP_COMMANDS` 5000); `behavior_prefs.rs:124` (`undo_levels`) | nada |
| 4.3e | `collect_garbage` nunca chamada | FEITO | `session.rs:174` (automática) e `persist.rs:372` (ao salvar) | nada |
| 4.4a | GPU ociosa, tudo na CPU | NÃO FEITO | nenhum crate do motor usa wgpu para compor | decisão: compositor na GPU |
| 4.4b | Sem cache por camada | FEITO | `FrameCache` em `aiai-render` (2e492d2, 0de0ec5) | nada |
| 4.4c | Tempestade de alocações 132 MB por camada | PARCIAL | caixa do alfa, tiles tocados, LRU de tiles (2e492d2) | ainda `Image` f32 de quadro inteiro no composto |
| 4.4d | Disputa de `rayon` entre ferramenta e render | NÃO FEITO | sem pools separados | pool próprio para o render |
| 4.4e | Máscara de preenchimento do tamanho do quadro | FEITO | não reproduziu: `mask_of` já recorta (backlog) | nada |
| 4.4f | Tamanho do canvas do corte editável; Começo rápido ignorava o tamanho | FEITO | `canvas.rs:141`; `props.canvas_4k` (commit 9e5626d) | nada |
| 4.4g | Metas de 4K (render < 8 ms, balde < 15 ms, 60 fps) | VERIFICAR | `evidencias/S23/desempenho/bench4k.json`: re-render 1890→339 ms, 1º clique do balde ~650 ms | rodar o bench de novo; as metas não foram atingidas |
| 4.5a | `PAINT_SHAPES` / laço de preenchimento na GUI | PARCIAL | `app/figure.rs` (em andamento, não commitado) | commitar e conferir |
| 4.5b | `GRADIENT` na GUI | PARCIAL | `app/gradient_tool.rs` (em andamento, não commitado), `ToolId::Gradient` | commitar e conferir |
| 4.5c | Gerador de céu `SKY` e `PATTERN_FILL` em perspectiva na GUI | NÃO FEITO | só predefinições de degradê de céu (`grad_tool::sky_presets`) | ferramentas de céu e de textura em perspectiva |
| 4.5d | `LINES` (velocidade/foco) na GUI | FEITO | `workbar.rs:339-356` (menu Filtro); `figure.rs` com as abas (em andamento) | nada |
| 4.5e | MeshWarp / Liquify / Puppet Warp na GUI | NÃO FEITO | ver 2.17 | |
| 4.5f | Camadas Ajuste e Texto no menu `+` | PARCIAL | `layers.rs:18` tem Texto; Ajuste fora | incluir `Adjustment` |
| 4.5g | Editor de partículas na GUI | NÃO FEITO | nada de `particles` fora de `layers.rs`/`commands.rs` | presets pétala/chuva/neve/faísca e parâmetros |
| 4.5h | Efeitos `Para`, `RadialBlur`, `ColorAwareBlur`, `Adjust`, `Lut` no menu | FEITO | `panels/effects_browser.rs:22-29` | nada |
| 4.5i | 3D cel-shading, importação, câmera | FEITO | S24: spike 4,8 ms (6ea984e), `aiai-3d-render` (outline, toon) | extração de linhas do modelo (S24 "ficou para depois") |
| 4.5j | Fumaça/fogo/raios procedurais 2D | NÃO FEITO | nenhum | gerador estilo anime |
| 4.5k | Camera shake paramétrico | PARCIAL | bridge `CameraRequest::Shake` (`satsuei_tools.rs:150`) | botão/painel na GUI (não confirmado) |
| 4.5l | `lock_alpha` na camada raster | NÃO FEITO | nenhum | |
| 4.5m | Anti-overflow (não atravessar linhas da referência) no pincel | NÃO FEITO | nenhum | |
| 4.6a | `INBETWEEN` e `PROPAGATE_FILLS` na Linha do tempo | NÃO FEITO | nenhum botão em `timeline.rs`/`anim.rs` | botões de gerar intercalação e propagar pintura |
| 4.6b | Dope sheet / graph editor, trilhas de Câmera/Efeito | FEITO | `panels/comp_timeline.rs:235,855` (Composição, commit 0680c5d) | na Linha do tempo de Animação continuam invisíveis (por desenho) |
| 4.6c | `propagate_fills` com semente linear em arco | NÃO FEITO | `aiai-tools/src/anim.rs:196-200` | usar Procrustes/bend do `inbetween`; `LACUNAS.md` nº 14 |
| 4.6d | Rigging 2D nativo (ossos, IK, pai-filho, deformadores) | NÃO FEITO | só âncora e chaves; S24 "parenting com herança" ficou para depois | |
| 4.6e | ProRes 10-bit com alfa (`rgba64le`) | NÃO FEITO | `aiai-media/src/lib.rs:147` `rgb24`; `:182` `yuv422p10le` | 4444 com alfa |
| 4.6f | Fila de entrega com progresso por quadro e Cancelar | FEITO | `panels/deliver.rs:230` (cancelar), fila v2 (f6ce0d7) | nada |
| 4.6g | Kokoro via `uv run`; `FFmpeg`/VOICEVOX sem verificação de saúde | NÃO FEITO | `aiai-audio/src/kokoro.rs:74` | ONNX embutido ou sidecar, verificação na UI |
| 4.6h | Cor linear / HDR | NÃO FEITO | `aiai-paint/src/tile.rs:16` `clamp(0,1)` | |
| 4.6i | Prefs: recentes, atalhos, layouts, biblioteca de pincéis | FEITO | `prefs.rs:22,26`, `layout_store.rs`, biblioteca em Documentos (e1444bc) | nada |
| 4.6j | Prefs: geometria da janela principal | NÃO FEITO | `main.rs:104` `persist_window: false` | |
| 4.6k | Projeto em arquivos por corte com trava por corte | NÃO FEITO | `persist.rs` só `project.lock` do projeto todo | S24 deixou o `.aiaiproj` SQLite para depois |
| 4.7a | Áudio ao vivo, scrubbing, waveform nos clipes e na timeline | FEITO | `audio_out.rs`, `audio.rs:233`, `timeline.rs:530-557`; commits 4e8967c, 3904dba | nada |
| 4.7b | Botões de áudio na GUI: importar WAV, TTS, lip-sync, SFX | FEITO | `panels/audio.rs:378-419`, `audio_ws/tts.rs` | nada |
| 4.7c | Botão de BGM | NÃO FEITO | backlog "menos BGM"; só pela `aiai_bgm` | |
| 4.7d | Iro-Tresse (linhas R/G/B no balde e no compositor) | NÃO FEITO | `boundary_from_alpha` só lê alfa | |
| 4.7e | Variantes de iluminação na paleta (Dia/Entardecer/Noite) | PARCIAL | modelo: `PaletteData::active_variant`, `NamedColor::variants` (`model.rs:908`) | escolha por corte e interface |
| 4.7f | `.XDTS` e `.OTIO` | FEITO | commit 9273374 | importar `.xdts` não |
| 4.7g | Botão Folha de e-conte | FEITO | `panels/board.rs:148` | nada |
| 4.7h | Puppet Warp; modificadores de traço (vento, line boil, profundidade, espelho); colorização por dicas | NÃO FEITO | nenhum | |
| 4.8a | Ajuste automático de curva (RDP + Schneider) ao soltar a caneta | NÃO FEITO | `pen.rs`/`tools.rs` gravam as amostras brutas | |
| 4.8b | Editar nós de controle | FEITO | ferramenta Editar pontos, atalhos U26 | nada |
| 4.8c | Corrigir linha (Beliscar, Simplificar, Conectar, Largura, Redesenhar, poeira) | PARCIAL | `app/line_fix.rs` + `aiai-tools/src/line_fix.rs` + bridge/MCP (em andamento, não commitado) | commitar e conferir |
| 4.8d | QuickArc / Perspective Snap | NÃO FEITO | nenhum | |
| 4.8e | IA pontual de traço (ghost lines, micro-patches) | NÃO FEITO | proposta de pesquisa | |
| 4.8f | Casamento de inclusão (BasicPBC) no `propagate_fills` | NÃO FEITO | nenhum | |
| 4.9a | Duplo afinamento `taper × profile` | FEITO | commit 5b68146 (vale o mais fino); backlog 4.9.1 | nada |
| 4.9b | Soldagem automática de pontas próximas; fim do remendo "Fecho do balde" | NÃO FEITO | `tools/calib/c131_lines.py:48`, `a138_char.py:323` ainda usam | |
| 4.9c | Catálogo de canetas (10 do plano) | PARCIAL | 11 canetas/marcadores raster de fábrica (`brush.rs:976-1039`: Pena G, G real, Mapping, Nabo, Caligrafia...) | `VectorTip` segue com 4 variantes (`stroke.rs:74`); faltam School, Fude, Genga com grão no vetor, Iro-Tresse, ponta quadrada |
| 4.9d | Multi-Tip Stamp e Ribbon Brush | NÃO FEITO | só `TwoTone` de duas cores; sem `MultiStamp`/`Ribbon` | biblioteca de cenário e adereços |
| 4.9e | Sete réguas interativas com snap | FEITO | `RulerKind` (`ruler.rs:59`: perspectiva 1/2/3, paralela, radial, simetria, grade, linear, curva, figura, concentrica, guia); commit 27cd5fe, cb2c8f6 | nada |
| 4.9f | Criador de pincel e importador `.abr` | FEITO | U29 (`brush_import.rs`, `brush_maker.rs`), `.aiaibrush` (e1444bc) | `.sut` é recusado com motivo (backlog) |
| 4.9g | Plugins `.aiaiplugin` (WASM, shaders WGSL) | NÃO FEITO | nenhum | |
| 4.9h | Bancada de calibração (Steger, `w(s)`, junções V/T/L) em lugar do Canny | PARCIAL | `tools/csp/calib_compare.py`, `examples/calib_pens.rs` (S25, árvore de trabalho) calibram contra o Clip Studio | `EDGE_TOLERANCE_PX=4` ainda em `compare.rs:178`; sem Steger nem teste de vedação |

## Pendências priorizadas para despachar

Os grupos não se sobrepõem em arquivos; a ordem sugerida é a de impacto para o artista. Os agentes da S25 ainda editam `tools.rs`, `canvas.rs`, `selection.rs` e `brush*.rs`, então os grupos A e B esperam o commit deles.

### A. Verificador de buracos e balde (pequeno, alto impacto)
Arquivos: `aiai-studio/src/app/tools.rs`, `canvas.rs`, `aiai-tools/src/fill.rs`.
1. "Verificar" guarda as regiões (inclusive `border_unpainted`) e pinta marcas piscantes no canvas, com avançar de buraco em buraco (2.9).
2. Balde e caneta vetorial respeitam a seleção (2.15).
3. Laço de preencher buracos (parte do 2.9).

### B. Seleção, mover e conta-gotas
Arquivos: `selection.rs`, `sel_ops.rs`, `tools.rs`, `aiai-tools/src/transform.rs`.
1. Faixa de cor na Seleção (2.12).
2. Mover com inclinar, perspectiva, malha e liquify (2.17, 4.5e); depois Puppet Warp (4.7h).
3. Conta-gotas "só camada", lupa, manter paleta (2.18).

### C. Mesa digitalizadora (precisa de mesa para validar)
Arquivos: `canvas.rs`, `input_queue.rs`, `Cargo.toml` do studio, `drawing_prefs.rs`.
1. Ler a inclinação e passar `tilt` (4.1d).
2. Borracha traseira e botões (4.1e).
3. Pontos coalescidos (4.1f) e WinTab (4.1c).

### D. Entrada vetorial
Arquivos: `pen.rs`, `aiai-core/src/stroke.rs`, `aiai-paint/src/curve.rs`, `stroke_raster.rs`.
1. Ajuste automático de curva ao soltar (RDP + Schneider) (4.8a).
2. Soldagem de pontas e remover os "Fecho do balde" (4.9b).
3. `VectorTip` novas (School, Fude, Genga com grão, ponta quadrada) e caneta Iro-Tresse com `color_trace` na UI (2.7, 4.9c).
4. QuickArc (4.8d). Decidir antes: `pen_autoclose` desligado por padrão (1.7C2).
5. Commitar e conferir o Corrigir linha (4.8c).

### E. Pintura de cel (Shiage)
Arquivos: `aiai-tools/src/fill.rs`, `aiai-paint/src/fill.rs`, `colors.rs`, `characters.rs`.
1. Iro-Tresse no balde e no compositor (4.7d).
2. Variantes de iluminação por corte (4.7e).
3. `lock_alpha` e anti-overflow no pincel (4.5l, 4.5m).
4. Colorização por dicas (4.7h).

### F. Animação (intercalação)
Arquivos: `aiai-anim/src/lib.rs`, `aiai-tools/src/anim.rs`, `panels/timeline.rs`.
1. Botões Intercalar e Propagar pintura na Linha do tempo (4.6a).
2. Sem par suavizado, grupos/tags, 1-para-N (1.5a).
3. Semente do `propagate_fills` transformada como o `inbetween` (4.6c); depois inclusão BasicPBC (4.8f).
4. Rigging 2D e modificadores de traço (4.6d, 4.7h) — item grande, precisa de spike.

### G. Exposição na GUI do que já existe no motor
Arquivos: `panels/layers.rs`, `panels/satsuei.rs`, nova ferramenta de fundo.
1. `Adjustment` no menu `+` (4.5f).
2. Editor de partículas com presets (4.5g).
3. Painéis Céu (`SKY`) e Textura em perspectiva (`PATTERN_FILL`) no Cenário (4.5c).
4. Rodas Lift/Gamma/Gain, curvas e escopos (2.24).
5. Botão de BGM (4.7c).
6. Commitar e conferir `figure.rs` e `gradient_tool.rs` (4.5a, 4.5b).

### H. Entrega e distribuição
Arquivos: `aiai-media/src/lib.rs`, `aiai-audio/src/kokoro.rs`, `main.rs`.
1. ProRes 4444 com alfa, entrada `rgba64le` (4.6e).
2. TTS sem `uv`, verificação de FFmpeg/VOICEVOX (4.6g).
3. Geometria da janela principal (4.6j).
4. Importar PSD/ORA (4.2d), `.vrm` (4.2e).

### I. Desempenho 4K
Arquivos: `aiai-render`, `aiai-tools/src/paint.rs`, `render_service.rs`.
1. Rodar o bench 4K de novo e decidir se as metas ficam (4.4g).
2. Pool de `rayon` separado para o render (4.4d); composição sem `Image` f32 de quadro inteiro (4.4c). Compositor na GPU só depois da decisão (4.4a).

### J. Curta (micro-anime, scripts em `tools/curta/`)
1. `hair_ren` e `body_bust` que giram (3.2, 3.3), vista de perfil no sheet (3.1).
2. Satsuei nos personagens, blush, flare no toque, dissolve do fundo (3.8 a 3.11).
3. Cortes de ambientação (3.7).

### K. Grandes e de longo prazo
Biblioteca de pincéis Multi-Stamp/Ribbon (4.9d), plugins (4.9g), bancada de calibração com Steger (4.9h), cor linear/HDR (4.6h), projeto particionado por corte (4.6k), fumaça/fogo/raios (4.5j), IA pontual de traço (4.8e).

## Observações

- O backlog `S23-AUDITORIA-BACKLOG.md` ainda diz "P2 registrado, não começado" para onion skin, réguas com snap, importador `.abr` e 3D; isso está desatualizado (feito na S24/S23 rodada do usuário, tabela acima).
- A string `audio.no_live` (`i18n.rs:2579`) ficou órfã (a reprodução ao vivo existe); pode ser apagada.
- Nada foi compilado nem executado; os itens VERIFICAR dependem de abrir o app.
