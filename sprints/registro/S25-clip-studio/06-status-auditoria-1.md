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
| 1.5a | `inbetween`: sem grupos, traços sem par aparecem/somem em t=0,5 | FEITO (27/10) | traço sem par se apaga/desenha andando com o corpo; agora o traço tem grupo de intercalação (`Stroke.group`, campo opcional): só casa com o mesmo grupo e o traço a mais de um grupo nasce do par do grupo (1 para N; e N para 1 ao contrário) (`aiai-anim` `same_group`, `inbetween`); na interface, Seleção/Mover com traços escolhidos mostra "Grupo de intercalação"; `aiai_draw_curves` aceita `group`; testes no `aiai-anim` e de ponta a ponta pela bridge | |
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
| 1.8e | Pressão fixa 1,0 no mouse, sem simulação por velocidade | FEITO (27/10) | pincéis: dinâmica por velocidade/pressão/inclinação (`brush_dyn.rs`); caneta vetorial: "Início (afinar)" e "Fim (afinar)" em px, como o Início e fim do Clip Studio, valem para qualquer ponta e sem pressão (`tools::taper_profile` grava o perfil de largura do traço); conferido pela bridge com a G-pen de 16 px e no teste do perfil | |

### Parte 2 (UI/UX e ferramentas)

| # | Item | Status | Evidência | O que falta |
|---|---|---|---|---|
| 2.1 | "Começo rápido" sem camadas | FEITO | `commands.rs:320` (Fundo, Cor ligada à Linha, Linha, cel 1) | nada |
| 2.2 | Roda de cor fixa; popup cobrindo Camadas | FEITO | `panels/colors.rs` (`ColorTab::Wheel`, seletor próprio, U8) | nada |
| 2.3 | Painel de pincel só com Preset, Tamanho, Opacidade | FEITO | `panels/tool_props.rs`, `properties.rs:155`, `brush_maker.rs`; "Mostrar todos os ajustes" (U14) | nada |
| 2.4 | Miniaturas de traço dos pincéis | FEITO | `panels/brushes.rs:429` (traço do motor), painel Ferramenta em grade (U13, U20, U24) | nada |
| 2.5 | Caneta: reta, curva por cliques, retângulo, elipse | FEITO | commit 3289b64; `figure.rs` ampliando (em andamento) | polígono, Bézier, laço (em andamento) |
| 2.6 | Caneta: edição de nós e largura | FEITO | ferramenta Editar pontos (`ToolId::Points`, `nodes.rs`), alça U25, ids U27 | nada |
| 2.7 | Caneta: `color_trace` fixo `false` na UI | FEITO (26/10) | opção "Linha de cor (Iro-Tresse)" nas opções da caneta (`tools.rs` `commit_pen`) | o balde/compositor: ver 4.7d |
| 2.8 | Borracha vetorial apaga o traço inteiro | FEITO | modos trecho tocado, até a interseção, traço inteiro (`shelf.rs` `EraserVector(EraseMode)`); commit c572538, `aiai_erase` | nada |
| 2.9 | "Verificar" só dá um número, não destaca onde | FEITO (82cc089) | marcas piscantes, ‹ › e Pintar no painel; bridge `lint_next/prev/fill/clear` | nada |
| 2.10 | Onion skin / mesa de luz temporal | FEITO | `panels/light_table.rs`, `anim.rs` (`OnionMode`), commit 27a1e09 | nada |
| 2.11 | Seleção: Varinha | FEITO | commit 3a3d923; `selection.rs` | nada |
| 2.12 | Seleção: faixa de cor (`ColorRange`) | FEITO (b0ee21b) | menu Selecionar > Selecionar a cor atual | nada |
| 2.13 | Seleção: somar/subtrair/interseção, feather, inverter, expandir | FEITO | `selection.rs:65-195`, `sel_ops.rs` | nada |
| 2.14 | Seleção limita pincel e borracha raster | FEITO | commit 8316477 | caneta vetorial não respeita; balde não respeita (`FillRequest` não tem máscara) |
| 2.15 | Seleção limita balde e caneta | FEITO (82cc089) | a caneta vetorial sai em pedaços dentro da seleção; o balde já respeitava | nada |
| 2.16 | Mover: alças de escala/rotação, preview raster | FEITO | commits 3a3d923, 72f10e2, 3a6c235 (Ctrl+T 8 alças, prévia ao vivo `move_live.rs`) | nada |
| 2.17 | Mover: inclinar, perspectiva, malha, liquify, puppet warp | FEITO, menos Puppet Warp | Ctrl+canto = distorção (a659cbd); Malha (bc36073, gravação por triângulos); Liquefazer empurrar/expandir/contrair/girar (`Deform::LiquifyPath`) | Puppet Warp (4.7h) |
| 2.18 | Conta-gotas: cor da camada/composta, lupa, preservar paleta | FEITO (bc36073 + 26/10) | "Cor da camada"/"Cor exibida", lupa 11×11 que vira perto da borda, cor da paleta mantida, pixel vazio não troca a cor | nada |
| 2.19 | Rotação livre do canvas (não só saltos de 15°) | FEITO (27/10) | girar livre com R + arrastar, Mão > Girar e agora Shift+Espaço+arrastar (como no Clip Studio; `canvas.rs` `on_press`), além do deslizante do Navegador e do passo configurável; conferido pela bridge: arco de 90° = visão a 90°, sem traço | |
| 2.20 | `Layout`, `Pintura`, `Cenário` com painéis idênticos | FEITO | teste `layout.rs:1066` (`drawing_workspaces_no_longer_look_the_same`); réguas e Sub View nesses workspaces | Céu e Texturas em perspectiva ainda sem painel (ver 4.5A) |
| 2.21 | Personagens: desenhar na folha dentro da aba | FEITO | commit 7e5aba0 (pasta do personagem com canvas) | nada |
| 2.22 | `CONSISTENCY` digitando Y em pixels | FEITO (27/10) | Personagens > Consistência: "Marcar" ao lado de cada medida e o próximo clique no canvas dá o y (o clique não desenha); as linhas aparecem sobre o desenho com o nome enquanto a aba está aberta; "Do desenho: topo e pés" pega a caixa do que o corte mostra no quadro (`characters::cons_pick`, `paint_cons_guides`, `drawing_box`); os campos continuam editáveis | |
| 2.23 | Cor: monitor de áudio sob "Navegador"; coluna central desperdiçada | FEITO | o workspace "Cor" virou Biblioteca (`workspace.rs:19`); monitor só no Áudio (`app.rs:1437`) | nada |
| 2.24 | Cor: Lift/Gamma/Gain em caixas numéricas, sem rodas/curvas/escopos | FEITO, menos escopos (26/10) | rodas de cor no Lift/Gamma/Gain e no Equilíbrio de cor; editor de Curvas (RGB/R/G/B) e dos outros ajustes na janela de filtros (antes "só pelo MCP") | escopos (forma de onda/vetorscópio) |
| 2.25 | Áudio: timeline mostrando camadas de desenho | FEITO (verificado 27/10) | o Áudio abre na aba Multipista (trilhas de áudio); a aba Linha do tempo do Áudio é a dos desenhos com a faixa de áudio por cima, para sincronia (`calibracao/tmp/vis_verif/audio_timeline.png`) | |
| 2.26 | Edição sem Play/Pause | FEITO | `panels/edit_panels.rs:44,206` (Play/Pausa com som); commit 085c524, 8c624e9 | nada |
| 2.27 | Composição: parâmetros compostos "Editável via MCP" | FEITO (27/10) | curvas e listas já tinham editor (2.24); agora os campos opcionais (cor do Brilho e da Vinheta, centro da Aberração) têm "automático" que liga/desliga e, com valor, viram linha animável; o modo de mistura tem lista também no painel do corte; a LUT mostra o nome do arquivo com a dica de como trocar (`satsuei::optional_field`, `effect_controls::other_fields`) | |
| 2.28 | Miniaturas em branco dos cortes C005-C007 na Produção/Edição | FEITO (verificado 27/10) | numa cópia do projeto real, Produção (lista) e Edição (Mídia e linha do tempo) mostram as miniaturas dos 7 cortes, C005 a C007 inclusive | |

### Parte 3 (micro-anime)

| # | Item | Status | Evidência | O que falta |
|---|---|---|---|---|
| 3.1 | Model sheet: manequim 90° com desenho de costas | PARCIAL | `tools/curta/c02_sheets.py:5` `VIEWS=[0,30,180]` agora coerente com o desenho | vista de perfil (90°) e 3/4 costas (135°) ainda sem desenho (rig sem perfil) |
| 3.2 | `hair_ren` com `rot=False` descola do rosto em 3/4 | NÃO FEITO | `tools/curta/rig.py:284` todos `False`; backlog "pendente" | girar a silhueta lateral com o rosto |
| 3.3 | `body_bust` não gira em 3/4 | NÃO FEITO | `rig.py` comentário "não gira com a cabeça"; backlog "pendente (rig)" | ombros/avental/bolso assimétricos, tronco que gira |
| 3.4 | Gambiarra de `curta.py`/`c21_sobras.py` que pinta frestas com cor vizinha | FEITO | `tools/curta/curta.py:70-99` usa `skip_seeds_on_line`/`only_unpainted`/`close_leftovers`; `c21_sobras.py` removido; repintura `c22_repintura.py` | nada |
| 3.5 | Teste de consistência comparando 0,60 com 0,60 | FEITO (27/10) | `c20_revisao.py` mede o render do corte e do sheet; no app as medidas vêm do canvas (ver 2.22), não de números digitados | |
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
| 4.2d | Importar PSD/ORA, exportar ORA | FEITO (27/10) | Arquivo > Importar aceita .psd (RGB 8 bits, cru ou RLE) e .ora (ZIP guardado/deflate): uma camada raster por camada, com nome, visibilidade, opacidade e mistura; bridge `IMPORT_LAYERS` e MCP `aiai_import_layers`. Arquivo > Exportar ORA em camadas (e `EXPORT_PSD`/`aiai_export_psd` com `out` .ora): `export::export_ora`, mimetype guardado primeiro, `stack.xml`, um PNG por camada recortado, composto e miniatura; o teste lê de volta pela importação | |
| 4.2e | Importar 3D (.obj, .gltf, .glb) | FEITO | `aiai-3d-model/src/import.rs:37` | `.fbx` não; `.vrm` responde "em breve" (`import.rs:79`) |
| 4.3a | Autosave antes do primeiro Salvar | FEITO | `app/autosave.rs` (recovery a cada 2 min, intervalo nas Preferências) | nada |
| 4.3b | `sync_all` por tile travando 10 s | FEITO | `persist.rs:278` (blobs em paralelo, WAL sem sync por tile); commit fc17a74, eeadec5 | nada |
| 4.3c | Recuperação de queda: undo perdido, `mark_saved` mentiroso, WAL ignorado | FEITO | `doc.rs:183-196` (avisa `wal_replayed`, fica "não salvo") | nada |
| 4.3d | Undo fixo em 500 e `log` ilimitado | FEITO | `session.rs:72` (`LOG_KEEP_COMMANDS` 5000); `behavior_prefs.rs:124` (`undo_levels`) | nada |
| 4.3e | `collect_garbage` nunca chamada | FEITO | `session.rs:174` (automática) e `persist.rs:372` (ao salvar) | nada |
| 4.4a | GPU ociosa, tudo na CPU | NÃO FEITO | nenhum crate do motor usa wgpu para compor | decisão: compositor na GPU |
| 4.4b | Sem cache por camada | FEITO | `FrameCache` em `aiai-render` (2e492d2, 0de0ec5) | nada |
| 4.4c | Tempestade de alocações 132 MB por camada | PARCIAL | caixa do alfa, tiles tocados, LRU de tiles (2e492d2) | ainda `Image` f32 de quadro inteiro no composto |
| 4.4d | Disputa de `rayon` entre ferramenta e render | FEITO (26/10) | pool próprio do render do canvas (metade dos núcleos) em `render_service.rs` | medir no bench 4K |
| 4.4e | Máscara de preenchimento do tamanho do quadro | FEITO | não reproduziu: `mask_of` já recorta (backlog) | nada |
| 4.4f | Tamanho do canvas do corte editável; Começo rápido ignorava o tamanho | FEITO | `canvas.rs:141`; `props.canvas_4k` (commit 9e5626d) | nada |
| 4.4g | Metas de 4K (render < 8 ms, balde < 15 ms, 60 fps) | PARCIAL (medido 27/10) | `evidencias/S25/desempenho/bench4k.json` (3 rodadas, release): render inteiro 616–640 ms (S23: 1437), re-render depois de um retoque 85–118 ms (S23: 375), 1º clique do balde 507–548 ms (S23: 703), 2º clique 5–10 ms; a pincelada de Aquarela foi de 16 para 40–45 ms porque o preset passou a misturar com a camada (`mix` 0,15, S25) | as metas de 8 ms/15 ms não foram atingidas na CPU; dependem do compositor na GPU (4.4a) |
| 4.5a | `PAINT_SHAPES` / laço de preenchimento na GUI | FEITO (cdc0668, 1d3f2b2) | Figura (desenho direto) | nada |
| 4.5b | `GRADIENT` na GUI | FEITO (cdc0668, 1d3f2b2) | Degradê por arrasto, camada de degradê, degradê livre | nada |
| 4.5c | Gerador de céu `SKY` e `PATTERN_FILL` em perspectiva na GUI | FEITO (26/10) | Filtro > Céu (dia/manhã/pôr do sol/crepúsculo/noite) e Filtro > Textura (8 tipos; dentro da seleção; laço poligonal de 4 cantos = plano em perspectiva), cada um numa camada nova | nada |
| 4.5d | `LINES` (velocidade/foco) na GUI | FEITO | `workbar.rs:339-356` (menu Filtro); `figure.rs` com as abas (em andamento) | nada |
| 4.5e | MeshWarp / Liquify / Puppet Warp na GUI | FEITO, menos Puppet Warp | ver 2.17 | Puppet Warp |
| 4.5f | Camadas Ajuste e Texto no menu `+` | FEITO (26/10) | "+" > Camada de ajuste > Níveis/Curvas/Matiz/... (camada de efeito com a correção, abre os parâmetros) | nada |
| 4.5g | Editor de partículas na GUI | FEITO (26/10) | Propriedades da camada de partículas: presets (pétalas, brilhos, neve, chuva, poeira) e 16 parâmetros + cores (`panels/particles_ed.rs`) | faísca como preset |
| 4.5h | Efeitos `Para`, `RadialBlur`, `ColorAwareBlur`, `Adjust`, `Lut` no menu | FEITO | `panels/effects_browser.rs:22-29` | nada |
| 4.5i | 3D cel-shading, importação, câmera | FEITO | S24: spike 4,8 ms (6ea984e), `aiai-3d-render` (outline, toon) | extração de linhas do modelo (S24 "ficou para depois") |
| 4.5j | Fumaça/fogo/raios procedurais 2D | NÃO FEITO | nenhum | gerador estilo anime |
| 4.5k | Camera shake paramétrico | FEITO (27/10) | Composição > Propriedades > Câmera do corte: Tremer com força (px), duração (quadros) e Amortecer, a partir do quadro atual (vira chaves `shake` da câmera); bridge `CameraRequest::Shake` | |
| 4.5l | `lock_alpha` na camada raster | FEITO (26/10) | botão "Bloquear transparência" na barra das Camadas (`params.lock_alpha`); commit e prévia (`paint::lock_alpha_region`) | nada |
| 4.5m | Anti-overflow (não atravessar linhas da referência) no pincel | FEITO (26/10) | "Não passar da linha" no pincel: a região das linhas do balde onde o traço começou limita o pincel (`tools::stroke_clip`) | nada |
| 4.6a | `INBETWEEN` e `PROPAGATE_FILLS` na Linha do tempo | FEITO (26/10) | menu da célula: "Intercalar até o próximo desenho" (2s quando cabe), timesheet e pintura propagada das chaves (`anim::run_inbetween`) | nada |
| 4.6b | Dope sheet / graph editor, trilhas de Câmera/Efeito | FEITO | `panels/comp_timeline.rs:235,855` (Composição, commit 0680c5d) | na Linha do tempo de Animação continuam invisíveis (por desenho) |
| 4.6c | `propagate_fills` com semente linear em arco | FEITO (26/10) | semente vai pelo `GlobalMotion` (o mesmo movimento das intercalações) medido nas linhas de referência | BasicPBC (4.8f) |
| 4.6d | Rigging 2D nativo (ossos, IK, pai-filho, deformadores) | NÃO FEITO | só âncora e chaves; S24 "parenting com herança" ficou para depois | |
| 4.6e | ProRes 10-bit com alfa (`rgba64le`) | FEITO (26/10) | `VideoCodec::Prores4444` (yuva444p10le, alfa 16), render sem fundo, preset "ProRes 4444 com transparência" na Entrega; teste com ffprobe | nada |
| 4.6f | Fila de entrega com progresso por quadro e Cancelar | FEITO | `panels/deliver.rs:230` (cancelar), fila v2 (f6ce0d7) | nada |
| 4.6g | Kokoro via `uv run`; `FFmpeg`/VOICEVOX sem verificação de saúde | PARCIAL (26/10) | Áudio > Verificar ferramentas: FFmpeg, ffprobe, VOICEVOX (1,5 s) e Kokoro (script, modelos, uv) com o que falta (`aiai-tools/src/health.rs`) | Kokoro sem `uv` (ONNX embutido) |
| 4.6h | Cor linear / HDR | NÃO FEITO | `aiai-paint/src/tile.rs:16` `clamp(0,1)` | |
| 4.6i | Prefs: recentes, atalhos, layouts, biblioteca de pincéis | FEITO | `prefs.rs:22,26`, `layout_store.rs`, biblioteca em Documentos (e1444bc) | nada |
| 4.6j | Prefs: geometria da janela principal | FEITO (26/10) | posição, tamanho e maximizada gravados em `prefs.window` (1 s depois de parar) e usados ao abrir; `--size` manda mais | nada |
| 4.6k | Projeto em arquivos por corte com trava por corte | NÃO FEITO | `persist.rs` só `project.lock` do projeto todo | S24 deixou o `.aiaiproj` SQLite para depois |
| 4.7a | Áudio ao vivo, scrubbing, waveform nos clipes e na timeline | FEITO | `audio_out.rs`, `audio.rs:233`, `timeline.rs:530-557`; commits 4e8967c, 3904dba | nada |
| 4.7b | Botões de áudio na GUI: importar WAV, TTS, lip-sync, SFX | FEITO | `panels/audio.rs:378-419`, `audio_ws/tts.rs` | nada |
| 4.7c | Botão de BGM | FEITO (26/10) | Áudio > Gerar BGM pelo clima (calmo, alegre, tenso, triste): trilha original em loop do tamanho do corte (`aiai-audio` `mood_song`) | nada |
| 4.7d | Iro-Tresse (linhas R/G/B no balde e no compositor) | FEITO (26/10) | linha de cor (opção da caneta) delimita o balde mesmo escondida; as regiões vão por baixo dela até o meio, então esconder não abre fresta (`fill::compute_regions_uncached`) | nada |
| 4.7e | Variantes de iluminação na paleta (Dia/Entardecer/Noite) | FEITO (26/10) | painel Cores > Conjunto: Iluminação (Normal/variantes, gerar Entardecer/Noite/Sombra, editar cor por variante) e "Neste corte" (`CutData::palette_variant`, render e pincel por corte) | nada |
| 4.7f | `.XDTS` e `.OTIO` | FEITO | commit 9273374 | importar `.xdts` não |
| 4.7g | Botão Folha de e-conte | FEITO | `panels/board.rs:148` | nada |
| 4.7h | Puppet Warp; modificadores de traço (vento, line boil, profundidade, espelho); colorização por dicas | NÃO FEITO | nenhum | |
| 4.8a | Ajuste automático de curva (RDP + Schneider) ao soltar a caneta | FEITO (26/10) | "Simplificar ao soltar" (0..100, padrão 10 = 1 px) usa o Simplificar do Editar pontos (RDP com largura + desvio medido na curva); 121 amostras viram ~28 pontos | nada |
| 4.8b | Editar nós de controle | FEITO | ferramenta Editar pontos, atalhos U26 | nada |
| 4.8c | Corrigir linha (Beliscar, Simplificar, Conectar, Largura, Redesenhar, poeira) | FEITO (7e54e65, 1104dd7) | `app/line_fix.rs` + `aiai-tools/src/line_fix.rs` + bridge/MCP | nada |
| 4.8d | QuickArc / Perspective Snap | FEITO, menos Perspective Snap (26/10) | "Forma rápida": parar 0,35 s no fim vira reta, arco ou elipse (`aiai-tools/src/quick_shape.rs`) | encaixe na grade 3D |
| 4.8e | IA pontual de traço (ghost lines, micro-patches) | NÃO FEITO | proposta de pesquisa | |
| 4.8f | Casamento de inclusão (BasicPBC) no `propagate_fills` | NÃO FEITO | nenhum | |
| 4.9a | Duplo afinamento `taper × profile` | FEITO | commit 5b68146 (vale o mais fino); backlog 4.9.1 | nada |
| 4.9b | Soldagem automática de pontas próximas; fim do remendo "Fecho do balde" | FEITO no app (26/10) | "Soldar pontas": a ponta perto de outra linha entra nela e fica reta (canto na ponta = sem afinar) (`line_fix::weld_endpoints`) | scripts `tools/calib` ainda desenham pelo MCP com "Fecho do balde" |
| 4.9c | Catálogo de canetas (10 do plano) | FEITO (26/10) | `VectorTip` + Pena escolar, Fude, Ponta quadrada (corte reto), Lápis de genga (grão de papel) e a linha de cor | nada |
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
