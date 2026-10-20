# S24 — Pesquisa: o que cada workspace deve ser

Pesquisa só de leitura (nenhum código alterado). Data: 2026-10-19. Fontes web citadas em linha; o que veio de conhecimento do ofício (sem página verificada nesta pesquisa) está marcado **[não verificado]**.

Notação de layout (a mesma de `layout.rs`): `Row[...]` = lado a lado, `Col[...]` = empilhado, `Tabs[...]` = abas, o número antes é a proporção. `Canvas` é o viewport central.

---

## 0. Diagnóstico do código atual

### 0.1 Por que Layout, Pintura, Cenário e Composição parecem iguais

`layout.rs::default_node` dá aos quatro **a mesma árvore**: `Row[0.17 Col[Tabs[Projeto, Pincéis], Tabs[Navegador, SubView]], 0.55 Canvas, 0.28 Col[Paleta, Tabs[Camadas, Propriedades]]]` (Composição troca só a coluna da direita por Paleta/Camadas/Propriedades). Animação é a mesma árvore mais a Timeline embaixo. Não existe um único painel que nasça de uma etapa do pipeline. Causas:

1. **Os painéis são genéricos** (`Canvas`, `Layers`, `Properties`, `Palette`, `Brushes`...) e os workspaces só diferem pela combinação deles. Onde há conteúdo próprio, ele está escondido por `if env.ws == ...` dentro do mesmo `PanelKind` (`Timeline` na Edição, `Media`/`Preview` no Áudio, `Navigator` na Cor mostrando o monitor).
2. **A prateleira de ferramentas é a mesma** (`ToolId::ALL`, 10 ferramentas) para os 5 workspaces com `has_canvas()`, inclusive Composição, que deveria ser um *viewer* e não um canvas de pintura.
3. **`Properties` é o balaio** de tudo (camada, ferramenta, efeitos, câmera, grade).
4. **Não existe luz de mesa / onion skin em lugar nenhum**: `grep -ri onion` em todos os crates não acha nada. Isso é o coração da Animação.
5. **Não existe régua de perspectiva interativa**: `aiai_perspective` / `perspective_command` (aiai-tools/src/guide.rs) só **assa** raios de fuga como traços numa camada de guia, e `aiai-guide3d` só monta blocos + manequim (`Shape` = Box/Cylinder/Cone/Sphere/Plane/Stairs); `Mesh` é só `verts + tris`, ou seja, receber modelo importado é barato.
6. **Layout (dock) não é persistido**: `trees: HashMap<WorkspaceId, Tree<PanelKind>>` vive só em memória (`app.rs:63`); "Restaurar layout" = `trees.remove(&ws)`. Exibir > Painéis só lista painéis que **já estão na árvore** (o que foi fechado fica invisível na árvore e volta; o que nunca esteve na árvore não pode ser adicionado). Não há layouts nomeados nem variantes.
7. `Prefs` (prefs.rs) não tem campo de layout; `WorkspaceId::Cor` hoje é *grade de cor do corte* (lift/gamma/gain) com o monitor no lugar do Navegador.

### 0.2 Painéis por workspace hoje (de `layout.rs`)

| Workspace | Painéis (árvore padrão) | Observação |
|---|---|---|
| Roteiro | Estrutura(Outline) \| Roteiro(Script) \| Propriedades | Row 0,22/0,52/0,26 |
| Personagens | Personagens(lista) \| Folha(Sheet) \| Col[Paleta, Propriedades(ficha)] | 0,22/0,50/0,28 |
| Storyboard | Col[Row[Cenas(Shots) \| Prancha(Board) \| Propriedades], Timeline] | 0,72/0,28; sem canvas |
| Layout | Col[Tabs[Projeto,Pincéis], Tabs[Navegador,SubView]] \| Canvas \| Col[Paleta, Tabs[Camadas,Propriedades]] | idêntico a Pintura/Cenário |
| Animação | igual ao Layout + Timeline embaixo (0,34) | Timeline horizontal, linha 22 px |
| Pintura | = Layout | |
| Cenário | = Layout | |
| Composição | Tabs[Projeto,Pincéis]+Navegador \| Canvas \| Col[Paleta, Camadas, Propriedades(efeitos+câmera)] | |
| Cor | Navegador(monitor) \| Paleta \| Propriedades(grade) | |
| Áudio | Col[Row[Mídia \| Monitor \| Mixer], Timeline] | |
| Edição | Col[Row[Mídia \| Prévia \| Propriedades], Timeline(edição)] | 1 faixa de vídeo + legendas + créditos |
| Entrega | Presets \| Fila \| Propriedades | |
| Produção | Episódios \| Tarefas(カット表) \| Propriedades | |

`PanelKind` (22): Canvas, Layers, Properties, Palette, Navigator, Timeline, Script, Outline, Characters, Sheet, Shots, Board, Preview, Media, Mixer, Presets, Queue, Tasks, Episodes, Project, SubView, Brushes. Sem categoria, sem "onde docar por padrão", sem "em quais workspaces faz sentido".

### 0.3 O que o motor já tem e a UI não expõe (reaproveitar)

- `CelRole { Key, Breakdown, Inbetween }` (aiai-core/model.rs:578) e `Stage { Layout, Genga, Douga, Shiage, Background, Satsuei, Color, Sound }` (:266): a estrutura Genga/Douga **já existe no modelo**; falta UI.
- `aiai_inbetween`, `aiai_propagate_fills`, `aiai_lint` (pintura); `aiai_pattern_fill` (homografia), `aiai_sky`, `aiai_gradient`, `aiai_transform` (cenário); `aiai_guide3d`, `aiai_perspective`, `aiai_model_sheet`, `aiai_consistency` (layout/personagem); `aiai_effects` (25 efeitos em `aiai-fx/effects.rs`), `aiai_camera`, `aiai_layer_motion`, `aiai_grade`, `aiai-fx/scopes.rs` (histograma, waveform, parade, vectorscope) para Composição; `aiai_tts_line/regenerate`, `aiai_voice_profile`, `aiai_lipsync`, `aiai_loudness`, `aiai_mix`, `dsp.rs` (biquads, EQ) para Áudio; `EditDoc` com 7 `TransitionKind`; `aiai_export_xdts/otio/psd/svg` para entrega/handoff; `PRODUCTION`/`CORRECTION`/`RETAKE` para Produção.
- `AudioTrack { Dialogue, Sfx, Bgm, Ambience }` já separa as faixas de áudio do corte.
- Restrições do projeto que valem aqui: sem IA generativa de imagem; áudio local e gratuito, vozes originais (regra 2); máquina com 8 GB de RAM e disco C: cheio (cache de miniaturas com orçamento, modelos pesados com pasta de instalação escolhida pelo usuário); tudo precisa ter paridade no MCP/bridge.

---

## 1. O pipeline japonês e o que cada ferramenta de referência ensina

### 1.1 Etapas (e o que cada uma entrega)

Resumo consolidado de [Kanzenshuu, Animation Production Guide](https://kanzenshuu.com/animation-production/positions-and-roles), [Toei/SIGGRAPH 2021 (fluxo 2D: storyboard, layout, key animation, in-betweens, clean-up, compositing, edição, dublagem)](https://history.siggraph.org/wp-content/uploads/2025/09/2021-SA_Courses_Japanese-Animation-in-2021-To-CG-or-not-CG.pdf), [Anime News Network/Animenation, "Ask John"](https://www.animenation.net/blog/?p=1370) e de fontes em japonês (絵コンテ → レイアウト → 原画 → 動画 → 仕上げ → 撮影; ver [animeanime.jp](https://animeanime.jp/article/2024/10/31/87413.html)):

| Etapa | Japonês | O que faz | Entrega | Mapeamento no app |
|---|---|---|---|---|
| Roteiro/Storyboard | 脚本 / 絵コンテ | planeja cena e corte | e-conte com ação, falas, tempo | Roteiro, Storyboard |
| Layout | レイアウト | o animador-chave desenha, por corte, enquadramento, posição dos personagens, perspectiva e as notas de câmera; é o "projeto" para genga e fundo | folha de layout (e 背景原図, o desenho-base do fundo) | **Layout** |
| Fundo | 背景 / 美術 | pintura do fundo a partir do layout; hoje digital (Photoshop etc.) | fundos (muitas vezes mais largos que o quadro, para pan) | **Cenário** |
| Genga | 原画 | desenhos-chave que definem o movimento; podem ser rascunho, depois "limpos" | genga + indicação de timing | **Animação (Genga)** |
| Sakkan | 作画監督 | corrige o genga para ficar on-model | genga corrigido | revisão em Produção (retakes/correções) |
| Douga | 動画 | limpa os genga e desenha as intercalações segundo a folha de tempo | animação completa linha limpa | **Animação (Douga)** |
| Shiage | 仕上げ | tinta e cor; hoje, coloração digital | quadros coloridos | **Pintura** |
| Satsuei | 撮影 | composição final: cel + fundo + efeitos + câmera | quadros finais | **Composição** |
| Edição | 編集 | monta os cortes fotografados em rough/fine cut | vídeo master sem áudio | **Edição** |
| Som | 音響 | vozes, música, SE, mixagem | master de áudio | **Áudio** |

Pontos de desenho tirados disso:

1. **Layout alimenta dois ramos em paralelo** (genga→douga→shiage e fundo). Cenário vem do mesmo layout que Animação; a ordem Layout, Cenário, Animação, Pintura é coerente.
2. **A folha de tempo (タイムシート / X-sheet) é o documento que liga as etapas**: segundo [CGWORLD](https://cgworld.jp/terms/タイムシート.html) e [a busca em japonês](https://www.nii.ac.jp/dsc/idr/userforum/poster/IDR-UF2024_P28.pdf) ela registra quantidade de genga e douga, empilhamento de cels, falas, câmera e efeitos; 1 s = 24 quadros (6 s = 144 quadros). Padrão de página (6 s por página, colunas de cel A/B/C/D, coluna de ação, de fala com letras de boca, de câmera) **[não verificado nesta pesquisa; confirmar com o modelo XDTS exportado por `aiai_export_xdts`]**. O app já exporta XDTS, então a **folha de exposição vertical é obrigatória** para quem pensa em animação japonesa.
3. A animação japonesa é "pose a pose": [poucos quadros-chave e mais intercalações](https://en.wikipedia.org/wiki/Anime). Logo a interface precisa tratar *chave* (K) e *intercalação* (I) como cidadãos de primeira classe, com marcas e atalhos.
4. [RETAS STUDIO](https://en.wikipedia.org/wiki/RETAS): Stylos/TraceMan (desenho/traço) → PaintMan (cor) → CoreRETAS (撮影: pan, multiplano, efeitos, saída). Cada etapa era **um programa com tela própria**; o CoreRETAS foi depois substituído pelo After Effects no papel de câmera/composição. É exatamente a separação Animação / Pintura / Composição que o usuário pede.

### 1.2 Ferramentas de referência: o que copiar de cada

| Ferramenta | O que organiza bem | Copiar |
|---|---|---|
| **Clip Studio Paint EX** ([animação](https://clipstudio.net/en/animation), [dicas](https://tips.clip-studio.com/en-us/articles/9951)) | paleta Timeline + paleta *Animation cels*: linha de cima = cel em edição, **linha de baixo = camadas registradas como mesa de luz**; [onion skin](https://help.clip-studio.com/en-us/manual_en/600_animation/Onion_skin.htm) com nº de quadros antes/depois diferentes, modo Cor/Meia cor/Monocromático, cor "anterior/seguinte" e opacidade com passo; pasta de câmera 2D; [réguas de perspectiva](https://help.clip-studio.com/en-us/manual_en/510_ruler/Perspective_Rulers.htm) 1/2/3 pontos + olho de peixe, linha do horizonte (eye level), "fixar eye level", "fixar ponto de fuga", "infinitizar", guias com alças (azul = move o ponto ao longo da guia, branca = ângulo), snap das ferramentas à régua, régua de 3 pontos casando com a câmera do modelo 3D | luz de mesa + onion com as mesmas opções; régua como objeto editável com alças; snap |
| **Toon Boom Harmony** ([interface](https://docs.toonboom.com/help/harmony-24/premium/getting-started/interface.html)) | views: Camera (+ Drawing em aba), Tool Properties, Colour, **Timeline e Xsheet** (duas vistas do mesmo dado), **Drawing Substitutions** (miniaturas dos desenhos da camada para escolher qual expor), Library (animação, desenhos, símbolos, modelos 3D), Node; views Perspective/Side/Top para multiplano; vários layouts salvos | Xsheet + Timeline como duas vistas; "substituições" = grade de cels da camada; vista lateral/superior para multiplano |
| **Storyboard Pro** ([workspaces](https://docs.toonboom.com/help/storyboard-pro-20/storyboard/reference/workspaces/about-workspaces.html)) | 8 workspaces prontos (Drawing, Timeline, 3D View...); Stage grande + Thumbnails embaixo; Panel view | Storyboard com canvas + faixa de miniaturas |
| **OpenToonz** ([interface](https://opentoonz.readthedocs.io/en/latest/interface_overview.html)) | "rooms" (workspaces) com "Reset to Default Rooms" e "Lock Room Panes"; Xsheet + **Level Strip** (todos os desenhos do nível, inclusive não expostos) + Flipbook + Viewer + Timeline | travar painéis; level strip = lista de cels da camada |
| **TVPaint** | onion skin com opacidade por quadro; folha de exposição ([resumo genérico](https://gitnux.org/best/onion-skin-animation-software/)) — páginas oficiais não achadas **[não verificado]** | opacidade individual por quadro de onion |
| **Photoshop (fundos)** | pincéis com textura, mistura/borrar, gradientes, transformar com perspectiva/warp, referências ao lado, ver em tons de cinza para valores **[não verificado, conhecimento do ofício]** | ferramentas do Cenário; "modo valores" |
| **After Effects** ([workspaces](https://helpx.adobe.com/hu/after-effects/desktop/get-started/get-familiar-with-the-interface/workspaces-panels-viewers.html); [layout padrão](https://blog.fmctraining.com/blog/after-effects-tutorial-understanding-workspaces-panels)) | Project à esquerda, Composition no centro, Effects & Presets à direita, **Timeline embaixo**; Effect Controls ao lado do Project em alguns workspaces; atalhos [A P S R T, U, J/K, F9](https://motiondesign.school/after-effects-keyboard-shortcuts/) | layout e atalhos de AE no workspace Composição |
| **Premiere Pro** ([workspaces](https://helpx.adobe.com/ee/premiere/desktop/get-started/tour-the-workspace/what-are-workspaces.html), [lista](https://videoguys.com/blogs/news-and-sales/premiere-bro-tutorial-working-workspaces)) | workspaces Assembly (Project grande), Editing, Color, Effects, Audio, Captions & Graphics...; Source + Program monitor; faixas V e A separadas; ferramentas [C razor, B ripple, N rolling, Y slip, U slide](https://blog.motionarray.com/learn/premiere-pro/ultimate-list-of-adobe-premiere-keyboard-shortcuts/) | variantes de layout por tarefa; ferramentas e atalhos |
| **Audition / Audacity** ([waveform + espectral](https://helpx.adobe.com/audition/using/displaying-audio-waveform-editor.html); [Audacity: painel de controle da faixa, espectrograma, label tracks](https://manual.audacityteam.org/man/track_control_panel_and_vertical_scale.html)) | Waveform editor com display espectral abaixo (divisória arrastável), Multitrack separado, Effects Rack, Essential Sound; Audacity: cabeçalho de faixa com menu (waveform/spectrogram), faixa de rótulos, ferramentas Seleção/Envelope/Zoom/Time-shift | editor de forma de onda + espectro; rack de efeitos; faixa de rótulos |
| **Kitsu / ShotGrid / ftrack** ([Kitsu](https://kitsu.cg-wire.com/), [estatísticas Kitsu](https://blog.cg-wire.com/lee-film-studio-sponsors-enhanced-statistics-for-kitsu), [Flow Production Tracking](https://www.autodesk.com/eu/products/shotgrid/features), [ftrack](https://www.ftrack.com/en/features)) | lista de planos com miniatura + colunas de status por etapa; estatísticas por status (contagem e quadros, em texto ou gráfico, export CSV); visões salvas; cartões/lista/timeline/kanban; gráficos burndown, status history; planejamento de recursos; tarefas atrasadas e gargalos | views Lista/Miniaturas/Grande/Quadro/Cronograma/Gráficos; burndown e histórico; visões salvas |

---

## (a) Ordem recomendada dos workspaces

Ordem pedida pelo usuário, que **mantenho**:

`Roteiro · Personagens · Storyboard · Layout · Cenário · Animação · Pintura · Composição · Biblioteca · Modelagem · Áudio · Edição · Entrega · Produção` (14)

Comentários e refinamentos:

1. **Está certa quanto ao pipeline** (layout → fundo/animação → pintura → satsuei → áudio/edição → entrega) e põe Produção por último (gestão atravessa tudo e é onde o usuário confere o todo).
2. **Biblioteca e Modelagem não são etapas, são apoio** e quebram a leitura "esquerda → direita = tempo". Em vez de mudar a ordem, **agrupar a barra com separadores visuais** e rótulos de grupo discretos (hover): `Pré-produção [Roteiro, Personagens, Storyboard] | Arte [Layout, Cenário, Animação, Pintura] | Finalização [Composição] | Recursos [Biblioteca, Modelagem] | Som e montagem [Áudio, Edição, Entrega] | Gestão [Produção]`. Custo: uma lista `WORKSPACE_GROUPS` em `workspace.rs`; a barra e o menu Janela só desenham um espaçador. Alternativa (se a barra ficar longa, 14 abas em 1280 px): mostrar só ícones nos grupos Recursos/Gestão. Em um passo futuro Biblioteca/Modelagem podem ir para o fim sem custo (só a ordem do array `ALL`).
3. **Animação é um workspace com duas fases** (Genga, Douga), não dois workspaces: o usuário disse "Animação (Genga + Douga)". Resolver com **variantes de layout de fábrica** dentro do workspace (ver (d)) + seletor `Genga | Douga` no cabeçalho do workspace. A fase também filtra a Timeline/Folha (chaves vs. intercalações) e liga/desliga a onion padrão.
4. **Cor → Biblioteca muda o que "Cor" significava**: hoje Cor = grade do corte (`GRADE`). Essa grade é trabalho de 撮影 e **vai para a Composição** (painel `Cor/Grade`, ao lado dos Scopes) e uma versão enxuta por clipe na Edição (como a aba Lumetri do Premiere). Chave serde: `biblioteca`, com `#[serde(alias = "cor")]` para não quebrar `prefs.json`; `from_key("cor")` deve devolver Biblioteca.
5. **Modelagem alimenta Layout/Cenário/Personagens**. Deixar o atalho de "Enviar ao Layout" visível e fazer o painel `Guia 3D` do Layout abrir a Modelagem com a cena selecionada (ida e volta).
6. **Áudio depois de Composição e antes de Edição** está certo para anime: o som é refeito depois do *picture lock* e a mixagem final precisa da imagem. A Edição mantém só o ajuste simples e o "Abrir no Áudio".
7. Atalhos de troca de workspace: `Alt+1..9,0` (Alt+0 = 10º) e `Alt+Shift+1..4` para os demais. Não usar `Ctrl+0/1`: já são zoom de visão (`ViewFit/ViewActual` em `keymap.rs`).
8. Mudanças em `WorkspaceId`: `Cor`→`Biblioteca`, novo `Modelagem` (+ ícones `WsBiblioteca`, `WsModelagem`), `ALL: [WorkspaceId; 14]`, teste `seen.len() == 14`, `has_canvas()` passa a ser um `profile()` (ver 0.4 abaixo).

### Arquitetura comum (vale para todos)

**Perfil de workspace** (novo, em `workspace.rs`): `WorkspaceProfile { tools: &[ToolId], default_tool, overlays_default: { onion, rulers, safe_frame, grid }, keymap_context }`. Resolve o problema 2 do diagnóstico (mesma prateleira para todos) e o das teclas (Composição não precisa de `B` pincel; Modelagem usa `W/E/R`). A `Keymap` ganha **contextos**: `global`, `canvas:<ws>`, `timeline`, `comp_timeline`, `edit_timeline`, `waveform`, `viewport3d`; o atalho vale quando o ponteiro/foco está no painel ou no canvas daquele workspace; o usuário continua podendo religar tudo (já existe `prefs.shortcuts`).

**Catálogo de painéis** (novo, em `layout.rs`): `PanelKind::meta() -> PanelMeta { key, title_key, category, dock_hint: Left|Right|Bottom|Center, needs_canvas: bool, workspaces: &[WorkspaceId] }`. Serve ao Exibir > Painéis, ao "adicionar painel que não está na árvore" e a testes ("todo painel do catálogo existe em ≥ 1 workspace e tem i18n PT/EN").

**Propriedades contextual**: `Properties` deixa de ser balaio: cada workspace registra um `PropertiesProvider` (o que já existe em `characters::properties`, `script::properties`, `shots::properties`, `deliver::properties`, `production::properties`, `edit::properties` segue o padrão; falta o mesmo para Layout/Cenário/Animação/Pintura/Composição).

**Catálogo dos painéis novos** (chave → título PT; `existe` = já há código a reaproveitar):

| Chave | Título | Categoria | Workspaces | Existe? |
|---|---|---|---|---|
| `scene_info` | Cena (roteiro + storyboard do corte) | Pré-produção | Layout, Cenário, Animação, Pintura, Storyboard | não (lê `script`, `shots`/`board`) |
| `char_ref` | Referências do personagem | Personagem | Layout, Animação, Pintura, Storyboard | não (lê `characters::sheet`) |
| `light_table` | Mesa de luz | Animação | Animação, Pintura | não |
| `timesheet` | Folha de exposição | Animação | Animação, Composição(leitura), Pintura | não (`timeline::exposure_runs` serve de base) |
| `cut_strip` | Faixa de cortes | Navegação | Layout, Cenário | não |
| `rulers` | Réguas e guias | Desenho | Layout, Cenário, Animação | parcial (`aiai_perspective` assa traços) |
| `guide3d` | Guia 3D | Desenho | Layout, Cenário, Animação | parcial (`aiai-guide3d`, `aiai_guide3d`) |
| `bg_variants` | Variantes e câmera do fundo | Cenário | Cenário | não |
| `color_script` | Roteiro de cor | Cenário | Cenário, Biblioteca | não |
| `generator` | Céu e gradiente | Cenário | Cenário | parcial (`SkySpec`, `aiai_gradient`) |
| `settei` | Settei (cores do personagem) | Pintura | Pintura | parcial (`Palette` + `CharacterData.palette`) |
| `lint` | Verificação (regiões abertas) | Pintura | Pintura | parcial (`aiai_lint`) |
| `paint_queue` | Fila de pintura (quadros do corte) | Pintura | Pintura | não |
| `effects_browser` | Efeitos e predefinições | Composição | Composição, Edição | parcial (`satsuei::effect_presets`) |
| `effect_controls` | Controles de efeito | Composição | Composição, Edição | parcial (`satsuei::layer_effects`) |
| `comp_timeline` | Linha do tempo da composição | Composição | Composição | não |
| `camera` | Câmera | Composição | Composição | parcial (`satsuei::camera`) |
| `color_grade` | Cor/Grade | Composição | Composição, Edição | existe (`satsuei::grade`, hoje na Cor) |
| `scopes` | Escopos | Composição | Composição, Edição | motor existe (`aiai-fx/scopes.rs`) |
| `multiplane` | Planos (vista lateral) | Composição | Composição, Cenário | não |
| `lib_tree`/`lib_grid`/`lib_preview` | Biblioteca | Recursos | Biblioteca (+ mini no Cenário/Personagens/Layout) | não |
| `outliner3d`, `viewport3d`, `props3d`, `pose`, `camera3d`, `model_lib` | Modelagem | 3D | Modelagem | motor parcial |
| `waveform`, `rack`, `multitrack`, `loudness`, `spectrum`, `labels`, `dialogue_cues`, `tts` | Áudio | Áudio | Áudio | parcial (`audio::mixer/monitor/media`) |
| `edit_bin`, `source_monitor`, `transitions`, `captions`, `edit_effects` | Edição | Edição | Edição | parcial (`edit::media/preview/timeline`) |
| `destinations`, `deliverables`, `preflight` | Entrega | Entrega | Entrega | não |
| `prod_main`, `prod_charts`, `prod_bottleneck`, `prod_ai`, `prod_schedule` | Produção | Gestão | Produção | parcial (`production::tasks`) |

---

## (b) Cada workspace

### 1. Roteiro (脚本)

- **Propósito**: escrever e estruturar o roteiro por cena; é a fonte das falas (TTS), do tempo estimado e do storyboard.
- **Hoje** (mantidos): Estrutura (`outline`), Roteiro (`script`), Propriedades (`script::properties`).
- **Layout padrão**: `Row[0.20 Tabs[Estrutura, Beats], 0.50 Roteiro, 0.30 Col[0.30 Elenco da cena, 0.40 Propriedades, 0.30 Tabs[Notas, Estatísticas]]]`
- **Painéis novos** (pequenos):
  - *Elenco da cena*: personagens que falam/aparecem na cena selecionada (do `speaker` dos elementos), cada um com avatar (corte da folha de modelo), voz (perfil), nº de falas e segundos; clique abre o personagem. Reusa `characters::char_data`.
  - *Notas*: texto livre por cena (pesquisa, referências), guardado como elemento `nota` já existente.
  - *Estatísticas*: segundos estimados por cena, falas por personagem, palavras por minuto, cenas sem storyboard, falas sem TTS (link para Áudio > Falas).
  - *Beats* como aba ao lado da Estrutura (hoje só leitura embaixo).
- **Atalhos** (contexto `script`): `Tab` troca o tipo do elemento (ação → fala → parentético → transição, como Fountain/Final Draft), `Ctrl+Enter` novo elemento abaixo, `Ctrl+↑/↓` mover elemento, `Ctrl+Shift+N` nova cena. Sem ferramentas de canvas.

### 2. Personagens (キャラクター) — pasta do personagem

- **Propósito**: a "pasta" de cada personagem com **todo** o material: model sheet (turnaround), expressões, tabela de boca, figurinos, olhos, modelo 3D, paleta de settei e voz. Quem anima, pinta e faz layout vai buscar aqui.
- **Hoje** (mantidos): `characters::list`, `characters::sheet` (corte da folha gerado por `MODEL_SHEET`, linhas-guia em cabeças), ficha em `Properties` (nome, perfil, idade, altura em cabeças, paleta, voz, geração da folha, medidor de consistência), Paleta.
- **Modelo de dados** (novo): `CharacterData.materials: Vec<CharMaterial { id, kind, name, outfit: Option<String>, angle: Option<f32>, source: Cut(NodeId) | Blob(hash) | Model3d(hash), tags, note }>` com `kind ∈ { Turnaround, Expression, Mouth, Eyes, Hands, Pose, Outfit, Model3d, Prop, Note }`. Cada material de desenho é um **corte** (reusa o motor de desenho; "Abrir para desenhar" leva ao Pintura/Layout), materiais importados são blobs (`aiai_import_image`). Figurinos: `outfits: Vec<Outfit { name, palette: Id, turnaround: material }>` (paleta de settei por figurino).
- **Layout padrão**: `Row[0.17 Personagens, 0.50 Pasta do personagem, 0.33 Col[0.46 Propriedades (ficha), 0.26 Settei, 0.28 Tabs[Consistência, Voz]]]`
- **Painel novo `Pasta do personagem`** (`char_folder`) — o centro. Barra de seções (segmentada): `Folha | Expressões | Boca | Olhos | Figurinos | Poses | Modelo 3D | Notas`. Conteúdo:
  - *Folha*: o turnaround atual (existente) com vistas (frente, 3/4, perfil, 3/4 costas, costas), linhas-guia de proporção ligáveis, nome da vista; seletor de figurino; botões Gerar (`MODEL_SHEET`), Importar imagem, Abrir para desenhar.
  - *Expressões*: grade de miniaturas (neutro, alegre, triste, raiva, surpresa, vergonha, medo, cansado, + personalizadas), por ângulo; botão "+ expressão"; arrastar uma expressão para a Timeline/canvas coloca como cel; tag de emoção liga ao campo `emotion` das falas (Roteiro) e ao perfil TTS.
  - *Boca (口パク)*: gráfico A I U E O + N/fechada, por ângulo (frente, 3/4, perfil); tabela fonema→cel usada por `aiai_lipsync`; botão "Gerar do lip-sync" e "Testar com fala" (toca trecho do TTS com a boca trocando). Resolve a lacuna 11 (lip-sync por grupo de camadas) como futura extensão.
  - *Olhos*: aberto/meio/fechado/piscada, direções (esq., dir., cima, baixo), brilhos; mostra a sequência de piscada.
  - *Figurinos*: cartões por figurino (miniatura da folha + paleta); "Duplicar figurino" copia folha e paleta.
  - *Poses*: biblioteca de poses de referência (desenhos ou capturas do manequim 3D).
  - *Modelo 3D*: importar `.glb/.gltf/.obj/.vrm` (ver 3D abaixo), miniatura giratória (turntable), proporção em cabeças ligada a `heights`, botão "Usar no Layout" (envia ao `Guia 3D`) e "Abrir na Modelagem". Sem modelo importado mostra o manequim parametrizado por `heads_tall` (`aiai-guide3d::Mannequin`).
  - Rodapé comum: zoom, **comparar A/B** (dois materiais lado a lado com sobreposição por transparência), "Medir consistência" (`aiai_consistency` contra a folha).
- **Painéis novos menores**: `Consistência` e `Voz` saem da ficha para abas próprias (hoje estão dentro de `characters::properties`, 693 linhas).
- **Atalhos**: `1..8` trocam a seção; `A` alterna A/B; `G` liga/desliga guias; `Space` arrasta.

### 3. Storyboard (絵コンテ)

- **Propósito**: planejar cortes: painéis com enquadramento, ação, falas e tempo; gerar animatic e os cortes.
- **Hoje** (mantidos): `shots` (Cenas), `board` (Prancha com miniaturas em cache), `shots::properties`, Timeline (animatic).
- **Layout padrão** (inspirado em [Storyboard Pro: Stage grande + Thumbnails embaixo](https://docs.toonboom.com/help/storyboard-pro-20/storyboard/reference/workspaces/about-workspaces.html)): `Col[0.70 Row[0.17 Cenas, 0.58 Canvas(painel ativo), 0.25 Col[0.50 Propriedades, 0.50 Tabs[Cena(roteiro), Referências do personagem]]], 0.30 Tabs[Prancha(faixa de miniaturas), Animatic(Timeline)]]`
- **Mudança maior**: Storyboard ganha **canvas** (`has_canvas`) com a prateleira mínima (Caneta, Pincel, Borracha, Laço, Mover, Mão, Zoom), em vez de `Board::open_drawing` mandar para outro workspace. Desenho continua sendo o corte do painel (`panel_image`). Tamanho M/L; **fase 3** (não bloqueia o resto).
- **Painel novo**: `Cena` (`scene_info`) com o trecho do roteiro da cena do painel selecionado (falas em destaque).
- **Atalhos**: `Ctrl+←/→` painel anterior/seguinte, `Ctrl+Enter` novo painel após o atual, `D` duplica painel, `Ctrl+Shift+A` gera animatic.

### 4. Layout (レイアウト)

- **Propósito**: traduzir o corte do storyboard em desenho de produção: **enquadramento e área de câmera**, posição e escala dos personagens, **perspectiva**, volumes e plano de fundo (背景原図), movimento de câmera. É a base para genga e fundo ([Kanzenshuu](https://kanzenshuu.com/animation-production/positions-and-roles): "blueprint for the rest of the animation").
- **Hoje**: Canvas, Projeto, Pincéis, Navegador, SubView, Paleta, Camadas, Propriedades (tudo genérico).
- **Layout padrão**: `Col[0.88 Row[0.19 Col[0.38 Cena, 0.62 Tabs[Réguas, Guia 3D, Pincéis]], 0.53 Canvas, 0.28 Col[0.22 Navegador, 0.42 Camadas, 0.36 Tabs[Propriedades, Paleta]]], 0.12 Faixa de cortes]`
  - **Canvas com área de câmera**: o canvas é maior que o quadro (overscan) e mostra o quadro de câmera, margens seguras (ação 93 %/título 90 % **[não verificado; confirmar padrão do app]**), terços, e os **retângulos de início/fim da câmera** ligados por seta (lê/escreve as chaves de `aiai_camera`); o texto "PAN / TU / TB / FIX / FOLLOW" vira predefinição que grava as chaves. O guia de campo japonês (フィールドチャート, 12 campos) pode virar uma grade opcional **[não verificado]**.
  - **Painel Cena** (`scene_info`): abas `Roteiro` (elementos da cena e, em destaque, a fala que cai no tempo do corte) e `Storyboard` (imagem do painel + ação + enquadramento + segundos). Botão "Painel como camada de referência" cria camada `Reference` a 35 % de opacidade, travada, abaixo do desenho.
- **Painéis novos**:
  - **`Réguas e guias`** (`rulers`) — ver especificação completa em (c.2).
  - **`Guia 3D`** (`guide3d`) — especificação em (c.2).
  - **`Faixa de cortes`** (`cut_strip`): miniaturas horizontais dos cortes da cena (cache de `production::cut_thumbnail`), o ativo destacado, clique troca de corte, cada miniatura mostra a etapa Layout (cor de status). Altura 12 %.
  - Propriedades contextuais: dimensões do quadro, overscan em px, margem segura, escala em cabeças (régua de cabeças: seleciona um personagem e mostra linhas de 1 cabeça de altura).
- **Ferramentas do canvas** (`profile.tools`): Mover `V`, Caneta `P`, Pincel/lápis `B`, Borracha `E`, Pontos `A`, Laço `L`, **Régua `R`** (novo: cria/edita réguas), **Câmera `K`** (novo: arrasta quadros inicial/final), **Medir `M`** (novo: distância em px e em cabeças; mostra ângulo), **Orbitar 3D `O`** (novo: arrasta = orbita a câmera do Guia 3D, Shift = pan, roda = dolly), Nota `T` (novo: texto de anotação de layout em camada Guide), Conta-gotas `I`, Mão `H`, Zoom `Z`.
- **Atalhos**: `Ctrl+'` mostra/oculta grade de perspectiva, `Ctrl+;` mostra/oculta réguas e guias, `Ctrl+Shift+;` liga/desliga snap às réguas, `Shift+R` alterna a régua ativa, `Alt+arrastar` sobre um ponto de fuga = mover só o ponto.

### 5. Cenário (背景・美術)

- **Propósito**: pintar o fundo a partir do layout/背景原図: cena grande (mais larga que o quadro para pan), planos de profundidade, variantes de horário (dia/tarde/noite), texturas e céu, valores e cor consistentes entre cortes.
- **Hoje**: iguais aos de Layout. Motor: `aiai_sky`, `aiai_pattern_fill` (textura plana ou **em perspectiva por homografia**), `aiai_gradient`, `aiai_transform`, `aiai_gag_bg`, pincéis (`Brushes`, `brush_maker`, importação de .abr/.brush).
- **Layout padrão**: `Row[0.16 Col[0.40 Tabs[Pincéis, Texturas], 0.60 Tabs[SubView, Navegador]], 0.55 Col[0.80 Canvas, 0.20 Tabs[Variantes e câmera, Roteiro de cor]], 0.29 Col[0.30 Tabs[Paleta, Céu e gradiente], 0.42 Camadas(planos), 0.28 Propriedades]]`
  - **SubView grande** (fotos de referência, conta-gotas) como painel principal da esquerda, porque pintura de fundo vive de referência.
  - **Camadas como planos**: o painel de camadas do Cenário mostra grupos fixos (Céu, Fundo, Meio, Primeiro plano, Overlays) com a **profundidade (multiplano)** por camada visível na linha; atalho para criar a estrutura padrão ("Criar planos do cenário").
- **Painéis novos**:
  - **`Variantes e câmera`** (`bg_variants`): (1) tira de **variantes do mesmo local** (dia, entardecer, noite, chuva...) como cortes irmãos, clique troca, "Duplicar como variante" copia camadas e abre o ajuste de cor; (2) **caminho da câmera sobre o fundo**: o fundo inteiro em miniatura com os retângulos de início/fim da câmera de cada corte que usa este fundo (dos cortes da cena), alertando se a câmera sai da arte pintada (resolve a lacuna 8, "cobrir o quadro").
  - **`Roteiro de cor`** (`color_script`): faixa horizontal com uma cor/paleta dominante por cena ou corte (extraída do render do quadro 0 por k-means pequeno), para checar a progressão de cor do episódio; clique abre o corte.
  - **`Céu e gradiente`** (`generator`): controles do `SkySpec` (hora do dia, nuvens, cores) e do gradiente (linear/radial, paradas), botão "Aplicar na seleção/camada" e prévia ao vivo pequena.
  - **`Texturas`** (aba no painel de pincéis): grade de materiais da Biblioteca (versão mini de `lib_grid` filtrada em texturas/padrões); arrastar para uma região = `pattern_fill` com modo "plano" ou "em perspectiva" (usa a régua do Layout como plano quando houver).
  - **Modo valores** no canvas: botão que mostra o desenho em tons de cinza e botão de comparar com uma referência do SubView; ajuda a checar valores como no Photoshop **[não verificado]**.
- **Ferramentas**: Mover `V`, Pincel `B`, **Mistura/borrar `S`** (novo), Borracha `E`, Balde `G`, **Gradiente `Shift+G`** (novo, `aiai_gradient`), **Preencher com padrão `Shift+F`** (novo, `aiai_pattern_fill`), **Transformar `Ctrl+T`** (livre/perspectiva/warp; `aiai_transform`), Laço `L`, Régua `R`, Conta-gotas `I`, Mão `H`, Zoom `Z`.
- **Atalhos**: `X` troca cor principal/secundária, `Alt+clique` conta-gotas, `Ctrl+Shift+V` alterna o modo valores, `D` volta às cores padrão.

### 6. Animação (原画 + 動画) — duas fases

- **Propósito**: desenhar o movimento. **Genga**: poses-chave em rascunho, com timing; **Douga**: limpar os genga e desenhar as intercalações segundo a folha de tempo ([Kanzenshuu](https://kanzenshuu.com/animation-production/positions-and-roles)). A mesma cena, duas fases, com ferramentas e vistas diferentes.
- **Hoje**: Canvas + Projeto/Pincéis/Camadas/Propriedades + `timeline::show` (blocos de exposição, 1 linha/camada, `ROW_H = 22`, `HEADER_W = 150`, 34 % da altura). Modelo: `CelRole`, `Stage`; motor: `aiai_inbetween`, `aiai_repeat`. **Faltam**: onion skin/mesa de luz, marcas K/B/I na UI, folha de exposição, acesso ao roteiro e à ficha.
- **Seletor de fase** no cabeçalho do workspace: `Genga | Douga | Livre` (troca a variante de fábrica; ver (d)); grava a fase no corte (`cut.phase`) para a Produção ligar `Stage::Genga/Douga` às ações do usuário.
- **Layout de fábrica — Genga**: `Col[0.78 Row[0.17 Col[0.50 Tabs[Cena, Pincéis], 0.50 Referências do personagem], 0.56 Canvas, 0.27 Col[0.20 Navegador, 0.30 Tabs[Mesa de luz, Paleta], 0.50 Tabs[Camadas, Propriedades]]], 0.22 Tabs[Linha do tempo compacta, Folha de exposição]]`
- **Layout de fábrica — Douga**: `Col[0.80 Row[0.19 Folha de exposição(vertical, alta), 0.54 Canvas, 0.27 Col[0.20 Navegador, 0.26 Mesa de luz, 0.54 Tabs[Camadas, Propriedades]]], 0.20 Row[0.72 Linha do tempo compacta, 0.28 Tabs[Referências do personagem, Cena, Pincéis]]]`
- **Painéis novos**:
  - **`Cena`** (`scene_info`) e **`Referências do personagem`** (`char_ref`): acesso à cena do roteiro e às fichas, sem sair do workspace.
    - `Cena`: abas Roteiro (elementos da cena; a fala corrente em destaque, com segundos e emoção), Storyboard (painel + ação + enquadramento), Tempo (anotações de timing do corte: nº de quadros, ritmo; campo de texto salvo no corte).
    - `Referências do personagem`: seletor dos personagens do corte (os que aparecem nas camadas/roteiro); abas `Folha | Expressões | Boca | Figurino | 3D`, mostrando o material da Pasta do personagem (item 2); **clique = fixa a imagem como referência flutuante sobre o canvas** (modo "fixar": arrastável, 40 % de opacidade, `Esc` solta) ou como camada `Reference` bloqueada; arrastar uma expressão/boca para a Linha do tempo cria o cel naquele quadro; mostra o medidor de consistência do desenho atual (`aiai_consistency`) com semáforo (on-model).
  - **`Mesa de luz`** (`light_table`): copia as opções do CSP ([onion skin settings](https://help.clip-studio.com/en-us/manual_en/600_animation/Onion_skin.htm)) e as melhora: ligar/desligar; quadros antes `0..5` e depois `0..5` independentes; modo `Cor | Meia cor | Monocromático`, cor do anterior (padrão azul) e do seguinte (padrão vermelho/verde); opacidade inicial + passo por quadro; **só chaves** (mostra as chaves vizinhas em vez dos quadros vizinhos, essencial no Genga); **por camada** (onion só da camada ativa / de todas / das marcadas); **camadas de mesa de luz** fixadas (como a linha de baixo da paleta *Animation cels* do CSP: a genga fica fixa por baixo ao fazer douga); opacidade individual por quadro (TVPaint **[não verificado]**). Implementação: o render do canvas compõe os cels vizinhos em textura de cache (orçamento: máx. 4 render extras por quadro, igual `RENDER_BUDGET` de `board.rs`).
  - **`Folha de exposição`** (`timesheet`): grade **vertical** (1 linha = 1 quadro, 24 por segundo, numeração e marca de segundo), colunas: `AÇÃO` (texto livre), `FALA` (texto da fala + letras de boca, do TTS/lip-sync), uma coluna por camada de animação (A, B, C, D...), `CÂMERA` (setas e chaves da `aiai_camera`). Células mostram o número do cel e a marca de papel (K ● vermelha, B ◐ amarela, I ○ azul, de `CelRole`); **uma célula que repete o mesmo cel vira traço vertical contínuo** (convenção de folha de tempo). Edição: digitar número no quadro expõe o cel; arrastar a borda estende a exposição (segurar); `Alt+arrastar` copia; clique no cabeçalho da coluna seleciona a camada. Sincroniza com `Linha do tempo` (mesmos dados). Exporta o `XDTS` que já existe. Referência de formato **[confirmar com o `aiai_export_xdts` do app]**.
  - **Linha do tempo compacta**: a `timeline.rs` ganha `density: Compact | Comfortable` (`ROW_H` 14 px compacto / 22 px confortável, `HEADER_W` 110 px, fonte 10 px, nomes truncados com "…" e hover); camadas em **grupos recolhíveis** (cada grupo = uma linha de resumo com a união das exposições); linha **só da camada ativa expandida**, as demais recolhidas; a faixa de áudio (diálogo) vira **uma linha fina com forma de onda** em cima; **trilha de marcas K/B/I** única no topo (resumo de todas as camadas) com os marcadores de chave clicáveis; altura padrão 22 % (era 34 %). Atalho para alternar: `Ctrl+Shift+T`.
  - **Intervalos (中割り)**: botão e painel pequeno dentro da Linha do tempo/Propriedades: selecionar duas chaves → *Inserir N intercalações* (cria cels `Inbetween` vazios, ou **auto** por `aiai_inbetween` quando vetorial), e **gráfico de espaçamento** (a escala de intervalos entre as duas chaves: linear / ease in / ease out / ease in-out, editável por arrasto dos tiques) que define a distribuição temporal; botões de exposição `1 em 1`, `2 em 2`, `3 em 3` (cada desenho dura 1, 2 ou 3 quadros; "em uns/dois/três") **[não verificado: nomenclatura usada nas folhas]**.
  - **Fase e travas**: ao entrar em Douga, o painel Camadas oferece "Iniciar douga a partir do genga": cria camada `Douga (limpa)` com os cels `Key` copiados como guia e baixa a opacidade da camada `Genga` a 30 %; cels de camada com papel Genga ficam **travados em Douga** (cadeado; `Shift+clique` destrava) para ninguém sobrescrever a chave.
- **Ferramentas**: Mover `V`, Caneta `P`, Pincel `B`, Pontos `A`, Borracha `E`, Laço `L`, Conta-gotas `I`, Mão `H`, Zoom `Z`, **Folhear `F`** (novo: segurar = passa os quadros do intervalo sem onion, como folhear papel), **Intervalo `Q`** (novo: arrasta entre duas chaves para criar a cadeia de intercalações).
- **Atalhos** (contexto `timeline`/canvas Animação): `K` marca o cel atual como Chave, `Shift+K` como Breakdown, `Insert` nova intercalação depois do quadro atual, `Ctrl+→/Ctrl+←` próxima/anterior chave, `Alt+→/Alt+←` próximo/anterior desenho, `O` liga/desliga onion, `Shift+O` liga/desliga mesa de luz, `,` / `.` quadro anterior/seguinte (já existem as setas), `Space` reproduz, `Shift+Space` reproduz no intervalo, `Ctrl+Shift+T` densidade da linha do tempo, `Ctrl+Shift+F` fixa a referência selecionada.

### 7. Pintura (仕上げ)

- **Propósito**: dar cor aos quadros limpos: preencher regiões, aplicar sombra de settei, verificar regiões abertas, propagar cores entre quadros. Hoje é coloração digital (a etapa foi o "ink and color" dos cels, [Kanzenshuu](https://kanzenshuu.com/animation-production/positions-and-roles)); no RETAS era o PaintMan, **um programa só para isso** ([RETAS](https://en.wikipedia.org/wiki/RETAS)).
- **Hoje**: Canvas + balde (`ToolId::Fill`, vínculo do balde às camadas de linha em `layers`), Cores (`colors`), `aiai_lint`, `aiai_propagate_fills`.
- **Layout padrão**: `Col[0.83 Row[0.17 Col[0.55 Settei, 0.45 Tabs[Verificação, Pincéis, SubView]], 0.55 Canvas, 0.28 Col[0.24 Navegador, 0.40 Tabs[Camadas, Propriedades], 0.36 Tabs[Mesa de luz, Referências do personagem]]], 0.17 Fila de pintura]`
- **Painéis novos**:
  - **`Settei`** (`settei`): a paleta do personagem do corte grande, em **grade região × tom**: linhas = regiões nomeadas (pele, cabelo, olho, roupa 1, roupa 2...), colunas = `Base | Sombra 1 | Sombra 2 | Realce`; clique seleciona o slot como cor atual (ligada à paleta, então mudar a paleta recolore tudo); `X` troca base ↔ sombra. Os pares vêm de `NamedColor.group` (já existe, "pele", "cabelo"); falta a convenção de tons (sufixo/atributo `tone`).
  - **`Verificação`** (`lint`): lista de regiões abertas/vazadas/não pintadas (`aiai_lint` do quadro e do corte), clique centraliza e pisca a região, botão "fechar lacuna" (aumenta o `gap close` do balde), "Verificar corte" e semáforo por quadro.
  - **`Fila de pintura`** (`paint_queue`): faixa de miniaturas dos **quadros do corte** (um por cel/quadro exposto da camada de linha) com selo (não pintado / pintado / com aviso do lint / propagado) e ações em lote: "Propagar cores do quadro atual para os seguintes" (`aiai_propagate_fills`), "Pintar tudo que casa" e "Verificar todos". Navega com `←/→`. Reusa o cache de miniaturas de `board.rs`.
  - **Mesa de luz** (a do item 6) com o padrão "mostrar o quadro anterior **já pintado** a 35 %" (referência de cor), em vez do onion de linhas.
- **Ferramentas**: Balde `G` (modos: região, toda a camada, por semente, com `tolerância` e `fechar lacuna`), Pincel `B` e Borracha `E` (acertos), Laço de preenchimento `L`, Conta-gotas `I`, **Pegar cor da região `Alt+clique`**, Mão `H`, Zoom `Z`. Pontos e Caneta saem da prateleira (não se desenha linha aqui).
- **Atalhos**: `1/2/3/4` base/sombra1/sombra2/realce da região sob o cursor, `X` troca base ↔ sombra, `Ctrl+Shift+P` propaga cores, `Ctrl+Shift+L` verifica o quadro, `;` / `'` região aberta anterior/seguinte, `←/→` quadro anterior/seguinte.

### 8. Composição (撮影) — estilo After Effects

- **Propósito**: compor, por corte, camadas de linha, cor e fundo com câmera, multiplano, efeitos e correção de cor; a saída é o quadro final. Hoje o workspace é um canvas de pintura com efeitos num painel de Propriedades — o oposto de After Effects.
- **Hoje** (reaproveitados): `satsuei::layer_effects`, `satsuei::camera`, `satsuei::grade` (que vem da Cor), `Layers`, `Project`, `Properties`; motor `aiai_effects`, `aiai_camera`, `aiai_layer_motion`, `aiai_grade`, `aiai-fx/scopes.rs`, `aiai-fx/lut.rs`.
- **Layout padrão** (segue o AE: Project à esquerda, Composition no centro, Effects & Presets à direita, **Timeline embaixo**; [fonte](https://blog.fmctraining.com/blog/after-effects-tutorial-understanding-workspaces-panels)): `Col[0.64 Row[0.20 Tabs[Projeto, Controles de efeito, Planos], 0.52 Canvas(viewer), 0.28 Col[0.50 Tabs[Efeitos e predefinições, Câmera], 0.50 Tabs[Cor/Grade, Escopos, Informações, Prévia]]], 0.36 Linha do tempo da composição]`
- **Viewer** (o `Canvas` neste workspace **não pinta**): ferramentas só de composição; barra do viewer: resolução `Completa | 1/2 | 1/4 | 1/8` (para os 8 GB), fundo de transparência (xadrez), canais `RGB | Alfa`, margens seguras, câmera ativa, "visão rápida" (desliga efeitos pesados durante o arrasto), `Ctrl+Shift+H` mostra/oculta alças das camadas.
- **Painéis novos**:
  - **`Linha do tempo da composição`** (`comp_timeline`) — o equivalente da Timeline do AE, **diferente** da Timeline de animação: 
    - Colunas por camada (configuráveis): visibilidade (olho), solo, cadeado, cor/rótulo, nº, nome, **Modo** (blend), **Matte** (usar a camada acima/abaixo como máscara: alfa/luma/invertido), **Pai** (parent), **Entrada/Saída/Duração/Esticar**.
    - À direita, a **barra de duração** de cada camada (arrastar corpo = deslocar; arrastar pontas = cortar entrada/saída), régua com área de trabalho (barras `B`/`N`), marcadores.
    - Cada camada **expande** (setinha ou `U`) em grupos: `Transformar` (Âncora, Posição, Escala, Rotação, Opacidade; do `aiai_layer_motion`), `Efeitos` (a pilha), `Câmera` (para a camada de câmera). Cada propriedade tem **cronômetro** (liga/desliga animação) e losangos de chave; arrastar losango muda tempo; clique-direito = interpolação (linear, ease, hold, bezier).
    - **Editor de gráfico** (alternar com `Shift+F3`): curva de valor e de velocidade por propriedade com alças bezier; botão `Easy Ease`.
    - Cabeça de reprodução, snap às chaves e bordas, zoom horizontal (`+`/`-`/`;`), "Ajustar ao corte" (`\`).
  - **`Efeitos e predefinições`** (`effects_browser`) — como o painel Effects & Presets: caixa de busca, **árvore por categoria** dos 25 efeitos de `aiai-fx::Effect`: *Desfoque* (Gaussiano, Direcional, Radial, Ciente de cor), *Luz* (Brilho, Difusão, Halação, **Luz transmitida 透過光**, Light wrap, Luz de recorte, God rays, Lens flare), *Cor* (Pará パラ, ajuste/LUT), *Lente/Filme* (Aberração cromática, Vinheta, Grão, Deband)... mais **Predefinições de anime** (as de `satsuei::effect_presets`) e **Minhas predefinições** (salvar a pilha atual como predefinição na Biblioteca); favoritos ★ e recentes; miniatura-prévia renderizada no quadro atual da camada selecionada em 96 px (cache; máx. 2 por quadro); **arrastar para a camada na linha do tempo ou para o viewer aplica** (`Duplo clique` = aplica na camada selecionada).
  - **`Controles de efeito`** (`effect_controls`): a pilha da camada selecionada: ligar/desligar efeito, reordenar por arrasto, recolher, **cronômetro por parâmetro** (animar parâmetros: *lacuna de motor*, hoje o efeito é um JSON estático + `posterize_time`; sem isso o painel mostra só valores), reiniciar, copiar/colar pilha, "tornar camada de ajuste". É o `layer_effects` extraído de `Properties`.
  - **`Câmera`** (`camera`): chaves da câmera do corte (posição, zoom, rotação), tremor, predefinições (PAN, TU, TB, ZOOM IN/OUT, FIX, FOLLOW), "ver pela câmera"; extraído de `satsuei::camera`.
  - **`Cor/Grade`** (`color_grade`): o `satsuei::grade` movido da Cor: rodas lift/gamma/gain, saturação, temperatura, contraste, LUT (`.cube`, depois de P5), vinheta/difusão/grão.
  - **`Escopos`** (`scopes`): histograma, waveform, RGB parade, vectorscope (motor pronto) com atualização sob demanda (botão e ao soltar ajuste) e resolução reduzida.
  - **`Planos`** (`multiplane`): vista **lateral** do multiplano (como as views Side/Top do [Harmony](https://docs.toonboom.com/help/harmony-24/premium/getting-started/interface.html)): eixo horizontal = profundidade, uma barra por camada, a câmera como cunha; arrastar a barra muda `depth`; mostra o aviso da lacuna 8 (borda do canvas visível com o pan).
  - **`Informações`**: cor sob o cursor (RGB/HSV/hex), coordenada em px, nome da camada sob o cursor.
  - **`Prévia`**: botões Reproduzir (`Space`), Prévia em RAM (`Num0`) com **orçamento de cache em disco/RAM** e barra verde de quadros em cache, taxa de quadros real, loop, resolução da prévia.
- **Ferramentas do viewer** (perfil): Seleção `V` (mover camada/arrastar caixa), **Mão `H`**, **Zoom `Z`**, **Rotacionar `W`**, **Âncora `Y`** (mover o ponto de âncora sem mover a camada), **Máscara `Q`** (forma/caneta como máscara da camada; precisa de suporte do motor **[a verificar]**), **Câmera `C`** (orbitar/pan/dolly alternando como o AE).
- **Atalhos** (contexto `comp_timeline`, seguem o [AE](https://motiondesign.school/after-effects-keyboard-shortcuts/)): `A/P/S/R/T` mostram Âncora/Posição/Escala/Rotação/Opacidade, `E` efeitos, `U` propriedades animadas, `UU` modificadas, `Shift+tecla` soma propriedades, `Alt+Shift+tecla` cria chave, `J/K` chave anterior/seguinte, `F9` Easy Ease (`Shift+F9` in, `Ctrl+Shift+F9` out), `B/N` início/fim da área de trabalho, `Ctrl+D` duplica camada, `Ctrl+Shift+D` divide camada no cabeçote, `[` `]` move entrada/saída para o cabeçote (no painel, não no canvas, onde `[` `]` continuam sendo tamanho do pincel nos outros workspaces).

### 9. Biblioteca (ライブラリ) — renomeia "Cor"

- **Propósito**: o acervo de **materiais**, **modelos 3D**, **paletas** e demais recursos reutilizáveis, do projeto e do usuário (entre projetos). Como a Library do [Harmony](https://docs.toonboom.com/help/harmony-24/premium/getting-started/interface.html) (animação, desenhos, símbolos, modelos 3D) e os "materiais" do CSP.
- **Hoje**: nada equivalente (a Cor virou grade; a Paleta, Pincéis e SubView são painéis isolados).
- **Tipos de item** (`LibItem { id, kind, name, tags, source, license, blob, thumb, scope }`): `Textura`, `Padrão` (procedural), `Paleta`, `Modelo 3D`, `Pincéis` (conjuntos importados, ligação com `brush_assets`), `Referência` (imagem), `Pose`, `Predefinição de efeito`, `Predefinição de câmera`, `Cenário pronto`, `Áudio` (SFX/BGM que o usuário queira reusar), `Modelo de layout` (layout de painéis). `scope ∈ Fábrica | Usuário | Projeto`. Armazenamento: usuário em `%LOCALAPPDATA%\AnimeIAI\library\` (índice + blobs endereçados por hash, com pasta alternativa configurável porque o disco C: está cheio), projeto em `biblioteca/` dentro do `.aiai`.
- **Layout padrão**: `Row[0.18 Árvore, 0.50 Grade, 0.32 Col[0.52 Prévia, 0.48 Propriedades do item]]`
- **Painéis novos**:
  - **`Árvore`** (`lib_tree`): escopos (Fábrica/Usuário/Projeto), tipos, coleções (pastas) e **etiquetas** (nuvem de tags); `Todos`, `Favoritos`, `Recentes`, `Sem tag`.
  - **`Grade`** (`lib_grid`): miniaturas com controle de tamanho (S/M/L) e alternância grade/lista, busca por nome/tag, filtros (tipo, licença, tamanho), multi-seleção, arrastar para o canvas (cria camada ou aplica padrão), para a Timeline (áudio), para a Pasta do personagem (modelo/pose), para as Camadas.
  - **`Prévia`** (`lib_preview`): imagem em zoom; **textura com teste de repetição 3×3** (costura visível); **modelo 3D girando** (turntable com o render de linhas/sombreado do `aiai-guide3d`); **paleta** com amostras e botão "Extrair paleta de uma imagem" (k-means); áudio com player e forma de onda.
  - **`Propriedades do item`**: nome, tags, **fonte e licença** (obrigatório ao importar: texto livre + URL), tamanho em disco, usos no projeto ("usado em 3 cortes"), botões *Usar no canvas*, *Inserir como camada*, *Abrir na Modelagem*, *Adicionar ao personagem*, *Reimportar*, *Remover* (confirma).
  - Editor de **paletas** (move o `colors` atual para cá como aba "Paletas" ampliada): harmonias (complementar, análoga, tríade), ordenar, agrupar por região de settei, importar `.ase`/`.gpl` **[a verificar formatos]**.
  - **Fábrica sugerida (licença livre)**: texturas/HDRIs/modelos CC0 do [Poly Haven](https://polyhaven.com/license) (CC0, sem atribuição, uso comercial livre); conferir a licença de cada pacote antes de incluir (regra do app: nada de fonte desconhecida) e **baixar só com confirmação do usuário** (nome, origem, tamanho).
- **Atalhos**: `Ctrl+I` importar, `Ctrl+F` buscar, `F2` renomear, `Ctrl+Click` multi-seleção, `Space` pré-visualiza em tela cheia.

### 10. Modelagem (3D para referência de desenho) — novo

- **Propósito**: **montar cena 3D simples** (blocos, escadas, paredes com abertura, mobília) e **posar o manequim** para gerar linhas de guia, sombra de referência e pontos de fuga que o artista desenha por cima, "como no Clip Studio com modelos 3D". Não é Blender: escultura e UV ficam fora. Complexo = modelar fora e **importar** (glTF/OBJ).
- **Hoje**: `aiai-guide3d` completo em motor: `Scene3d`, `Object3d`, `Shape` (Box, Cylinder, Cone, Sphere, Plane, Stairs), `Mannequin` (proporção em cabeças, pose nomeada, ajuste por osso), `Camera3d`, `render_lines` (linhas ocultas removidas), `render_shaded`, `vanishing_points`; ferramenta `aiai_guide3d`. Nada na UI.
- **Layout padrão**: `Col[0.84 Row[0.16 Outliner, 0.56 Viewport 3D, 0.28 Col[0.40 Propriedades 3D, 0.30 Manequim e pose, 0.30 Tabs[Câmera, Luz]]], 0.16 Tabs[Biblioteca 3D (primitivas, estruturas, poses, importados), Enviar ao Layout]]`
- **Painéis novos**:
  - **`Outliner`** (`outliner3d`): árvore da cena (objetos, manequins, câmeras, luzes), olho/cadeado, renomear, agrupar, arrastar para hierarquia (pai-filho), duplicar (`Ctrl+D`).
  - **`Viewport 3D`** (`viewport3d`): 1 vista ou quatro (`Ctrl+Space`: perspectiva, topo, frente, direita); ortográfica/perspectiva (`Num5`); sombreado `Linhas | Sólido | Sólido+linhas`; grade do chão e eixos; **gizmos** mover/rotacionar/escalar; snap (grade, 15°); `F` enquadra a seleção; `Num1/3/7` vistas. **Implementação em duas etapas**: fase A = prévia por software com o `render_lines`/`render_shaded` já existentes em textura 640×360 (suficiente e sem GPU), fase B = viewport `egui_wgpu::Callback` (o app já usa wgpu via eframe) para orbitar a 60 fps.
  - **`Propriedades 3D`**: transformação numérica (posição/rotação/escala em m/°), dimensões do `Shape` (largura, altura, raio, nº de degraus...), material de referência (cor lisa, **sombreado toon de 2/3 passos**), visibilidade em linhas/sombra.
  - **`Manequim e pose`** (`pose`): proporção (cabeças 5 a 9, largura de ombros, etc., `Proportions`), predefinições de pose (parado, andando, sentado, apontando, braços acima, T), **lista de ossos com 3 sliders (x, y, z)** e alças de arrasto no viewport, espelhar pose, salvar pose na Biblioteca; **puxar a altura em cabeças do personagem** da Pasta do personagem.
  - **`Câmera`** (`camera3d`): lente em mm/FOV, posição/alvo, altura do olho, rotação (roll), "usar câmera do corte" (sincroniza com `aiai_camera`), mostra os pontos de fuga e a linha do horizonte resultantes (alimenta `Réguas`).
  - **`Luz`**: direção (azimute/elevação) e dureza do sombreado de referência.
  - **`Biblioteca 3D`**: primitivas (caixa, cilindro, cone, esfera, plano, escada, arco, parede com porta, parede com janela, telhado, mesa, cadeira, cama, poste), poses e importados (`Modelo 3D` da Biblioteca).
  - **`Enviar ao Layout`**: escolhe o corte, a camada de guia de destino, estilo de linha (espessura, ângulo de vinco para as arestas, silhueta), sombreado opcional, e executa `aiai_guide3d`; "Congelar em traços" assa em vetor, "Manter ligado" regera quando a cena muda (com debounce de 400 ms).
- **Importação de modelos**: `.glb/.gltf` pelo crate [`gltf`](https://openapps.pro/packages/gltf) (Apache-2.0, parseia cena/malha/material/câmera, `.glb` binário), `.obj` por [`tobj`](https://docs.rs/crate/tobj/2.0.0) (leve), `.vrm` é glTF com extensão (modelos de anime, ex. VRoid) **[não verificado quanto a licença por modelo: exigir o campo Licença]**; **FBX não incluir** (o único caminho em Rust é o [`russimp`/Assimp](https://crates-io.ganymede-static.progval.net/crates/russimp/2.0.6), que liga biblioteca C++ pesada): pedir conversão para `.glb` (ex. via Blender) e dizer isso na mensagem de erro. Converter para `aiai_guide3d::Mesh` (`verts + tris`), guardar o arquivo original como blob (`Object3d::Imported { blob, scale, up_axis }`), limitar a 150 k triângulos no `render_lines` (aviso + decimação), pedir licença/fonte ao importar.
- **Atalhos** (contexto `viewport3d`, padrão de DCC): `Q` selecionar, `W` mover, `E` rotacionar, `R` escalar, `Shift+A` adicionar objeto, `Ctrl+D` duplicar, `Delete` apagar, `F` enquadrar, `Alt+arrastar` orbitar, `Shift+Alt+arrastar` pan, roda = dolly, `Num1/3/7` vistas, `Num5` ortográfica, `Ctrl+Space` quatro vistas, `Space` alterna o seletor de ferramenta.

### 11. Áudio (音響) — nível Audacity/Audition

- **Propósito**: produzir e mixar vozes (gravadas ou sintéticas), SE, BGM e ambiente; editar áudio de verdade (cortar, reparar, tratar) e medir (ARIB −24 LKFS, −14 LUFS web).
- **Hoje**: `audio::media` (cortes), `audio::monitor` (quadro do corte), `audio::mixer` (clipes por trilha Dialogue/Sfx/Bgm/Ambience, ganho, pan, início e fades, loudness, WAV), `aiai-audio` (`dsp.rs` biquads e EQ de 3 bandas, `mixer.rs`, `loudness.rs`, `kokoro.rs`, `voicevox.rs`, `lipsync.rs`, `timing.rs`, `sfx.rs`, `bgm.rs`).
- **Layout padrão** (baseado em [Audition: waveform com espectro embaixo, multitrack, Effects Rack](https://helpx.adobe.com/audition/using/displaying-audio-waveform-editor.html) e [Audacity: controle de faixa, espectrograma, rótulos](https://manual.audacityteam.org/man/track_control_panel_and_vertical_scale.html)): `Col[0.56 Row[0.16 Tabs[Arquivos, Falas, Biblioteca de áudio], 0.42 Forma de onda, 0.20 Monitor, 0.22 Col[0.60 Tabs[Rack de efeitos, Voz sintética], 0.40 Medidores]], 0.44 Tabs[Multipista, Mixer]]`
  - Variantes de fábrica: `Edição de forma de onda` (forma de onda gigante + espectro + rack), `Multipista` (padrão acima), `Voz` (Falas + Voz sintética + forma de onda), `Mixagem` (Mixer grande + medidores).
- **Painéis novos**:
  - **`Forma de onda`** (`waveform`): editor de **um clipe** em forma de onda com **espectrograma** logo abaixo (divisória arrastável; duplo clique recolhe, [como no Audition](https://helpx.adobe.com/audition/using/displaying-audio-waveform-editor.html)); seleção de intervalo (`I`), seleção espectral retangular (`M` marquee, só no espectro), **rolagem vertical de frequência** na régua, zoom de amplitude/tempo, **cruzamento por zero** (`Z`), snap a rótulos; edição não destrutiva com histórico: cortar/copiar/colar/apagar/silenciar (`Ctrl+L`)/aparar (`Ctrl+T`); **ferramentas** Seleção `I`, Deslocar no tempo `Y`, Envelope `E` (curva de volume por pontos), Lápis `P` (só em zoom de amostra), Mão `H`, Zoom `Z`, Multi `M`; efeitos de Audacity/Audition aplicados à seleção (menu **Efeito**): Amplificar, Normalizar, Fade in/out, Inverter, Reverter, Mudar velocidade/tom/andamento, **Equalizador** (gráfico 10 bandas e paramétrico; o `dsp.rs` tem biquads: peaking, shelf, low/high-pass), Compressor, Limitador, **Redução de ruído** (amostrar perfil, ajustar), Noise gate, Filtro passa-alta/baixa, De-esser, Remover estalos, Reverb, Eco/Delay, Repetir. Marcar **quais já existem em `aiai-audio`** e quais são novos (a maioria dos de dinâmica, ruído e reverb é nova).
  - **`Rack de efeitos`** (`rack`): cadeia ordenada por clipe ou por trilha (até 16 slots como no Audition), ligar/desligar por slot, predefinições, botão "A/B", **efeito em tempo real** na reprodução (processa em blocos), botão "Aplicar (renderizar)". Dados: `effects: Vec<AudioFx>` no clipe/trilha, MCP via `aiai_edit_mix`.
  - **`Multipista`** (`multitrack`): cabeçalho de faixa estilo Audacity ([painel de controle de faixa](https://manual.audacityteam.org/man/track_control_panel_and_vertical_scale.html)): nome, **M**ute/**S**olo/**R**ec(armar gravação), volume, pan, cor, altura (3 tamanhos), barramento (Diálogo, SE, BGM, Ambiente, Master), vista Forma de onda/Espectrograma; régua em **timecode + quadros** com cabeçote sincronizado ao corte e ao quadro; clipes com **alças de fade**, linha de **envelope de volume** (arrastar pontos), ganho, loop, divisão (`Ctrl+K`... no cursor), crossfade ao sobrepor; **faixa de rótulos** (Audacity label track) para marcar fala, impacto, pontos de sincronização, exportável; arrastar da Biblioteca/Arquivos; mostra o vídeo do corte como **faixa de miniaturas** acima (picture ref).
  - **`Falas`** (`dialogue_cues`): lista de falas do roteiro (como lista de ADR) com personagem, texto, tradução, emoção, estado (**sem áudio / TTS gerado / gravado / precisa regerar**), duração, tempo no corte; ações por linha: ouvir, regerar (`aiai_tts_regenerate`), editar texto, ir ao clipe; ação em lote "Gerar todas as pendentes" com fila e progresso; cache por hash `(texto, voz, parâmetros)`.
  - **`Medidores`** (`loudness`): VU/pico em tempo real por trilha e master, **LUFS momentâneo, curto e integrado, true-peak, faixa de loudness (LRA)**, alvo com marcador (−24 LKFS ARIB / −14 LUFS web), botão "Normalizar para o alvo" (`aiai_loudness`/`aiai_mix`).
  - **`Espectro`** (`spectrum`, aba nos Medidores): FFT em tempo real (barras) e espectrograma rolando.
  - **`Voz sintética`** (`tts`) — especificação completa em (c.6).
- **Atalhos** (contexto `waveform`/`multitrack`, seguem Audacity/Audition): `Space` reproduz/pausa, `Shift+Space` reproduz em loop a seleção, `Home/End` início/fim, `[`/`]` ajuste de ponto (**só neste painel**), `Ctrl+L` silenciar seleção, `Ctrl+T` aparar, `Ctrl+B` adicionar rótulo, `Ctrl+K` dividir clipe, `Delete` apagar, `Ctrl+Z/Shift+Z` desfazer/refazer, `+/-` zoom, `Ctrl+E` ajustar zoom à seleção, `Shift+E` envelope liga/desliga, `S` solo da trilha, `M` mute da trilha, `R` armar gravação.

### 12. Edição (編集) — estilo Premiere

- **Propósito**: montar o episódio (cortes fotografados, créditos, eyecatch), com trilhas separadas, transições, filtros e legendas; é o passo "picture lock" antes da mixagem final.
- **Hoje**: `edit::media` (cortes do projeto), `edit::preview` (quadro com transição/legenda/créditos), `edit::timeline` (clipes proporcionais, transições, faixa de legendas, créditos), `edit::properties`; motor `EditDoc { clips, subtitles, credits, fade_out_end }`, 7 `TransitionKind`, `aiai_edit_preview`, `aiai_edit_mix`. Lacunas conhecidas: **18** (legendas em quadros globais), **19** (sem reprodução com áudio).
- **Layout padrão** (Premiere "Editing": Project/Effects à esquerda, **Source + Program** no centro, Timeline embaixo; [fonte](https://videoguys.com/blogs/news-and-sales/premiere-bro-tutorial-working-workspaces)): `Col[0.55 Row[0.20 Tabs[Mídia, Transições, Efeitos, Legendas], 0.28 Monitor de origem, 0.30 Monitor do programa, 0.22 Tabs[Controles do efeito, Propriedades, Escopos]], 0.45 Linha do tempo]`
  - Variantes de fábrica (como os workspaces do Premiere): `Montagem` (Mídia gigante com miniaturas, [como o Assembly](https://helpx.adobe.com/ee/premiere/desktop/get-started/tour-the-workspace/what-are-workspaces.html)), `Edição` (padrão), `Legendas` (painel Legendas grande + monitor do programa + linha do tempo com faixas de legenda), `Cor` (Cor/Grade + Escopos grandes), `Áudio` (Mixer por trilha + medidores).
- **Linha do tempo nova** (`edit_timeline`, substitui `edit::timeline`): 
  - **Trilhas separadas**: `V3, V2, V1` (vídeo; V2/V3 para títulos, sobreposição e camadas de ajuste) | `A1 Diálogo, A2 SE, A3 BGM, A4 Ambiente` (mapeadas a `AudioTrack`) | `S1…Sn` (legendas por idioma) | trilha de marcadores. Cabeçalho: olho, cadeado, M/S, **seletor de destino** (patch de origem), altura (`Shift+rolar`), nome.
  - **Clipe de corte tem áudio ligado**: ao colocar um corte, o áudio do corte (`cut.audio`) aparece em A1-A4 como clipes **vinculados** (link ícone) ao vídeo; `Ctrl+L` desvincula; mover/cortar o vídeo move o áudio junto; "Expandir áudio" sob demanda (memória).
  - **Ferramentas** [como no Premiere](https://blog.motionarray.com/learn/premiere-pro/ultimate-list-of-adobe-premiere-keyboard-shortcuts/): Seleção `V`, Faixa à frente `A`, **Ripple `B`**, **Rolling `N`**, **Navalha `C`**, **Slip `Y`**, **Slide `U`**, Mão `H`, Zoom `Z`, Texto `T` (cria legenda/título). 
  - **Atalhos**: `J/K/L` retroceder/pausa/avançar (`L` repetido acelera) **[não verificado]**, `I/O` marcar entrada/saída no monitor de origem, `,` **inserir** e `.` **sobrescrever** o trecho marcado **[não verificado: confirmar teclas do Premiere]**, `Ctrl+K` dividir no cabeçote, `Q/W` ripple trim anterior/seguinte até o cabeçote, `Delete`/`Shift+Delete` apagar / apagar com ripple, `M` marcador, `Ctrl+D` aplicar transição padrão, `\` ajustar zoom, `S` liga/desliga snap.
  - **Transições**: arrastar da aba `Transições` para o ponto de corte, alinhamento (centro, início, fim) e duração (padrão 12 qd) ajustáveis por arrasto da alça; lista: Corte, Dissolver, Fade para preto, Fade para branco, **Flash** (já existem os 7 do `TransitionKind`), **+ Cortina (esq./dir./cima/baixo), Empurrar, Deslizar, Íris** e **Dissolver ao preto/branco** (novos). Cada transição tem prévia pequena animada ao passar o mouse.
  - **Filtros e ajustes por clipe** (`edit_effects` + `Controles do efeito` reaproveitando `effects_browser`/`effect_controls` da Composição filtrados em Cor/Estilo/Desfoque): Movimento (posição/escala/rotação/opacidade, chaves), **Cor** (brilho, contraste, saturação, temperatura, LUT; usa `aiai-fx::color/lut`), Desfoque, Vinheta/Grão, **Velocidade/duração** (retime com `Ctrl+R`), Recorte, Espelhar; **camada de ajuste** na V2/V3 (efeito sobre tudo abaixo).
  - **Legendas** (`captions`) — ver (c.4).
  - **Monitor de origem** (`source_monitor`): abre um corte da Mídia (`Duplo clique`), marcas `I/O`, botões Inserir/Sobrescrever; o **monitor do programa** é o atual `edit::preview` (transição, legenda queimada, créditos).
- **Modelo de dados** (EditDoc v2, **com migração do v1**): `tracks: Vec<EditTrack { id, kind: Video|Audio(AudioTrack)|Subtitle(lang)|Adjust, name, locked, muted, solo, height }>`, `items: Vec<EditItem { id, track, start, dur, source: Cut{id, in_, out} | Audio{asset, in_, out} | Title{...} | Caption{...} | Adjust, transition_in, transition_out, effects: Vec<EffectInst>, volume_keys, link: Option<u32>, speed }>`. **Legendas ancoradas ao clipe** (item `Caption { clip: Option<ItemId>, offset }`) resolvem a lacuna 18 (hoje deslocam ao mexer em fusão/trecho). `EDIT_SET` e `aiai_edit` ganham o esquema v2 (paridade MCP), com `v1→v2` automático ao abrir.
- **Reprodução** (lacuna 19): player com áudio ao vivo (saída de `audio_out.rs`) e cache de quadros à frente; baixa resolução (1/2) por padrão no perfil dev.

### 13. Entrega (納品)

- **Propósito**: exportar com segurança e em lote, em vários formatos e destinos, conferir (QC), embalar e arquivar.
- **Hoje**: `deliver::presets` (TV ARIB −24 LKFS + SRT ao lado, web −14 LUFS, H.264 GPU, ProRes, sequência PNG), `deliver::queue` (fila de **uma exportação por vez**, estado/tempo/resultado, thread com cópia da sessão), `deliver::properties` (pedido: o que exportar, arquivo, ajustes); `EXPORT_VIDEO`, `aiai_export_xdts/otio/psd/svg`.
- **Layout padrão**: `Row[0.20 Tabs[Predefinições, Destinos], 0.46 Col[0.62 Fila de exportação, 0.38 Entregáveis], 0.34 Col[0.46 Prévia, 0.54 Tabs[Pedido, Verificação final]]]`
- **Painéis novos / ampliados**:
  - **Fila de exportação** (ampliar `queue`): linha por item com **miniatura (quadro de pôster)**, nome, fonte (edição/episódio/corte), predefinição, destino, **barra de progresso por quadro + ETA** (hoje só estado/tempo), tamanho; ações: pausar, retomar, cancelar, repetir, **reordenar por arrasto**, prioridade, "mostrar na pasta", "copiar caminho", log em linha expansível; limite de **jobs simultâneos = 1 por padrão** (8 GB; configurável); "Adicionar à fila" vs "Exportar agora"; **matriz "exportar para…"**: marcar várias fontes × várias predefinições e enfileirar todas; a fila **persiste** entre sessões (reabre e oferece retomar).
  - **Formatos** (predefinições; tabela em `deliver::Preset`): vídeo — H.264/H.265 (NVENC se houver GPU), ProRes 422/4444, DNxHR, VP9/AV1 WebM **[conferir o que o ffmpeg do app encoda]**; GIF/APNG; sequência PNG/TIFF/EXR; áudio — WAV, FLAC, MP3, AAC e **stems** (diálogo, SE, BGM, ambiente separados); legendas — SRT, ASS, VTT; **handoff** — XDTS, OTIO, PSD, SVG; imagens — pôster JPG, **folha de contato** (`aiai_render_sheet`). Cada predefinição mostra resolução, taxa de quadros, codec, alvo de loudness, tamanho estimado.
  - **`Destinos`** (`destinations`): pasta local com **modelo de nome** (`{projeto}_{ep}_{fonte}_{predefinição}_{data}`), **cópia automática** para uma segunda pasta/unidade (útil com o disco C: cheio), pasta sincronizada (OneDrive/Drive/Dropbox como pasta comum), **pacote de entrega** (zip com MP4 + SRT + pôster + `creditos.txt` com as linhas `VOICEVOX:…` exigidas em `licencas-voz.md` + checksums SHA-256 + XDTS/OTIO opcionais). Upload direto (YouTube etc.) **fica fora** da primeira fase (OAuth/contas; política do app proíbe criar conta/entrar sozinho): deixar o ponto de extensão `Destination::Custom` e documentar.
  - **`Entregáveis`** (`deliverables`): grade de **cartões** do que foi exportado (miniatura/pôster com timecode escolhido, duração, tamanho, codec, loudness medido, resultado do QC), duplo clique abre o arquivo/pasta; **editor de pôster**: escolher o quadro (cabeçote), título/legenda sobre a imagem, exportar miniatura 1280×720 (estilo YouTube).
  - **`Verificação final`** (`preflight`): lista com semáforo antes de exportar: retakes abertos (Produção), lint de regiões abertas, etapas não aprovadas, créditos faltando (vozes usadas sem linha de crédito), **loudness fora do alvo**, **legendas fora do limite** (cps/linhas), clipes sem áudio, quadros pretos/silêncio no meio, resolução e fps do preset vs projeto; botão "Exportar mesmo assim" com confirmação. Reusa `aiai_production`, `aiai_lint`, `aiai_loudness`.
  - **Prévia**: toca o arquivo exportado (ou a fonte) para conferência rápida.
- **Atalhos**: `Ctrl+E` adiciona à fila, `Ctrl+Shift+E` exporta agora, `Delete` remove da fila, `Ctrl+Click` multi-seleção, `F5` reexecuta o item com falha.

### 14. Produção (制作進行)

- **Propósito**: gerir o episódio como estúdio: o que está pronto, atrasado ou travado, onde está o **gargalo**, e o que fazer.
- **Hoje**: `production::episodes` (episódios/cenas como filtro, resumo de etapas, relatório), `production::tasks` (a カット表: miniatura, nº, cena, duração, as **8 etapas**, correções/retakes), `production::properties` (etapas, lint, correções, retakes); `PRODUCTION`/`CORRECTION`/`RETAKE`.
- **Layout padrão**: `Row[0.17 Episódios e visões, 0.55 Principal, 0.28 Col[0.45 Detalhe do corte, 0.55 Tabs[Gargalos, Análise por IA, Propriedades]]]`
  - Variantes de fábrica: `Gestão` (acima), `Análise` (Gráficos + Gargalos grandes, lado a lado), `Revisão` (vista Grande + Detalhe).
- **Painel `Principal`** (`prod_main`, **uma única `PanelKind` com seletor de visão**, lembrada por episódio): `Lista | Miniaturas | Grande | Quadro | Cronograma` (inspirado em [Kitsu](https://kitsu.cg-wire.com/), [ftrack: cartão/lista/timeline/kanban](https://www.ftrack.com/en/features) e [Flow/ShotGrid](https://www.autodesk.com/eu/products/shotgrid/features)):
  - **Lista** (a カット表 atual, melhorada): colunas escolhíveis (miniatura, corte, cena, duração em qd, **etapa atual**, as 8 etapas em células coloridas, responsável, prazo, nº de retakes, nº de avisos do lint, **peso** = esforço estimado); ordenar, **agrupar** (por cena/etapa/responsável/status), **filtros** em chips (status, etapa, "com retake", "atrasado", "lint>0"), busca; células clicáveis ciclam o estado (`next_status` já existe), `Shift+clique` abre o detalhe; seleção múltipla com ações em lote (marcar etapa, atribuir, mudar prazo).
  - **Miniaturas**: grade de cartões (miniatura do quadro 0, nº do corte, duração, **barra de 8 segmentos coloridos** com o status de cada etapa, selo de retake, prazo) com tamanho S/M/L e agrupamento por cena; passar o mouse faz **folhear** (scrub) do corte na miniatura; reutiliza `cut_thumbnail` (orçamento `THUMB_BUDGET`).
  - **Grande**: um corte por vez, **grande**: prévia do corte (quadro a quadro com `←/→` e reprodução curta), à direita linha do tempo das 8 etapas com status/data/autor, correções e retakes abertos com imagens (`aiai_compare`/folha de diferença), botões Aprovar/Pedir retake, notas; navegação `↑/↓` entre cortes. Este é o modo de revisão/aprovação.
  - **Quadro** (kanban): colunas = status (`Não iniciado | Em andamento | Revisão | Aprovado | Retake`) para a **etapa escolhida** num seletor, ou colunas = etapas (cada corte na etapa ativa); cartões arrastáveis mudam o status (comando `PRODUCTION`).
  - **Cronograma**: barras por corte × etapa ao longo dos dias úteis (estimado vs realizado, marca de hoje, prazo do episódio); arrastar muda prazo; mostra dependências (Layout → Genga → Douga → Shiage; Fundo em paralelo; Satsuei depende de Shiage+Fundo; Som em paralelo) e o **caminho crítico**.
  - **Visões salvas** (como ShotGrid/ftrack): `Minhas visões` (nome + visão + filtros + agrupamento + colunas), listadas no painel Episódios.
- **`Gráficos`** (`prod_charts`; aba grande ao lado do Principal, e na variante `Análise`): usar o crate [`egui_plot`](https://docs.rs/egui_plot) **[a verificar compatibilidade com egui 0.36]** ou desenho próprio. Gráficos:
  1. **Barras empilhadas por etapa** (nº de cortes por status; alternar **cortes ↔ quadros**, como as [estatísticas do Kitsu](https://blog.cg-wire.com/lee-film-studio-sponsors-enhanced-statistics-for-kitsu), com % e exportar CSV).
  2. **Rosca geral** (quadros aprovados / total) e por cena.
  3. **Matriz cortes × etapas** (mapa de calor por status).
  4. **Burndown** (cortes restantes por dia vs. linha ideal até o prazo).
  5. **Fluxo cumulativo** (áreas empilhadas por status ao longo dos dias): mostra o gargalo como a faixa que engorda.
  6. **Vazão** (cortes concluídos por dia, média móvel de 7 dias) e **previsão de término** (restantes / vazão).
  7. **Tempo por etapa** (média de dias entre "em andamento" e "aprovado") e **taxa de retake por etapa**.
  - **Histórico**: derivado do **WAL/`aiai_log`** (cada mudança de status já é um comando com autor e hora): nenhum dado novo para guardar; um índice `StatusEvent { cut, stage, from, to, t, author }` reconstruído ao abrir (cache em disco). Sem isso não há burndown/fluxo; é o único requisito de motor da Produção.
- **`Gargalos`** (`prod_bottleneck`; **análise determinística, sem IA**, na linha de teoria das restrições/fluxo): por etapa calcula **fila** (cortes cuja etapa anterior está aprovada e esta não iniciou), **em andamento (WIP)**, **vazão** (média móvel de 7 dias), **idade média da fila** e **tempo de espera estimado = fila / vazão**; **gargalo = etapa com maior tempo de espera** (empate: maior WIP), **ociosa = etapa sem fila com vazão potencial** (aguarda a anterior); destaca cortes **atrasados** (prazo vencido), **travados** (retake aberto ou lint com erro há > N dias) e os que estão no **caminho crítico**. Saída em cartões: "Gargalo: Douga (14 cortes na fila, vazão 1,2/dia → 11,7 dias); Pintura ociosa; 3 cortes no caminho crítico atrasados" com links que **filtram a Lista** para os cortes envolvidos. Sugestões por regra (não por IA): "priorizar cortes que liberam Satsuei", "dividir cortes longos", "repetir lint antes de passar para a Pintura".
- **`Análise por IA`** (`prod_ai`, **opcional, desligada por padrão**):
  - Conceito: o app **gera um pacote de dados compacto** (JSON/Markdown de `aiai_production`: contagens por etapa, vazão, fila, atrasos, retakes recorrentes por motivo, pesos, últimos eventos, o resultado do painel Gargalos) e **o mesmo pacote é consumido por um agente pelo MCP** (`aiai_production`) — o caminho já usado em todo o projeto. A UI oferece: *Copiar pacote*, *Salvar como `analise-<data>.md`*, e *Colar resposta da análise* (guardada como nota do episódio com carimbo "gerado por IA" e a data).
  - **Opcional local**: botão "Analisar com modelo local" só aparece se houver um servidor local configurado (ex.: Ollama/llama.cpp em `127.0.0.1`), nunca nuvem por padrão (privacidade; 8 GB de RAM limitam modelos locais: avisar). O texto da IA **nunca aplica mudanças**: "Sugestões" viram botões (*Criar retake*, *Atribuir*, *Mudar prazo*) que pedem confirmação, cada um um passo de desfazer.
  - Conteúdo da análise (modelo de prompt fixo e versionado): resumo em 5 linhas, riscos (por corte/etapa), 3 ações recomendadas com impacto estimado, perguntas a quem gerencia; o app **não decide por si** (princípio "nenhuma decisão de limitador").
- **Painel `Detalhe do corte`**: miniatura grande com quadro a quadro, linha do tempo das 8 etapas (status/autor/data), lint, correções, retakes, notas e botões de pular para o workspace certo (Layout, Animação, Pintura, Composição, Áudio) **no mesmo corte**.
- **Atalhos**: `1..5` troca a visão, `Ctrl+F` busca, `Space` abre o Grande, `←/→/↑/↓` navega, `A` aprova a etapa selecionada, `R` pede retake, `Ctrl+Shift+C` copia o pacote para IA.

---

## (c) As lacunas apontadas pelo usuário, uma por uma

### c.1 Animação: estrutura Genga/Douga, cena do roteiro, fichas do personagem, timeline compacta

Resumido no item 6 de (b). Pontos de decisão:

1. **Chave × intercalação já existe no modelo** (`CelRole`): não criar campo novo. A UI mostra, na célula, ● (Key) ◐ (Breakdown) ○ (Inbetween) e a Linha do tempo compacta tem a **trilha-resumo de marcas**. Atalhos `K`, `Shift+K`, `Insert`.
2. **Genga e Douga são fases do corte** (`phase` no `CutData`), liga ao `Stage::Genga/Douga` da Produção; a fase muda: variante de layout, padrão da onion (Genga: só chaves; Douga: 2 antes + 2 depois, quadros), travas de camada, filtro da Linha do tempo/Folha.
3. **Acesso à cena do roteiro** (`scene_info`) e **às fichas** (`char_ref`) em painéis docados na própria Animação; as fichas vêm da Pasta do personagem (item 2), então depende dela (ou, no mínimo, da folha `MODEL_SHEET` que já existe).
4. **Compacta**: `density` na `timeline.rs` + grupos recolhíveis + linha de áudio fina + altura padrão 22 %; alternativa **Folha vertical** em aba. Medida-alvo: com 12 camadas, as linhas cabem em 22 % de uma tela de 1080 px sem rolagem (12 × 14 px + régua 22 px = 190 px ≈ 18 %).
5. **Mesa de luz/onion** (o maior buraco do código): só há modelo nos cels vizinhos; compor em textura de cache com orçamento.

### c.2 Layout: réguas de perspectiva e modelo 3D como guia

**Réguas e guias** (`rulers`) — o que implementar, do CSP ([Perspective Rulers](https://help.clip-studio.com/en-us/manual_en/510_ruler/Perspective_Rulers.htm)):

- **Dados**: `Ruler { id, name, kind, visible, locked, snap, color, opacity, ... }` guardados **na camada de tipo Guide** (`LayerType::Guide`; como no CSP, a régua pertence a uma camada de régua; vale só para quem enxerga a camada). Tipos:
  - `Perspective { points: Vec<[f32;2]> /*1..3 pontos de fuga*/, horizon: { y, angle }, fix_eye_level, fixed_points: Vec<bool>, guides: Vec<Guide> /*raios extras por ponto*/ , density }` (1, 2 ou 3 pontos).
  - `Fisheye { center, radius, distortion, fov }` (olho de peixe do CSP v2.0, [fisheye](https://tips.clip-studio.com/en-us/articles/7623)).
  - `Parallel { angle, spacing }` (linhas paralelas), `Radial { center, count }` (linhas de velocidade/concentração), `Symmetry { axis, count }`, `Line { a, b }` (guia reta), `Grid { quad: [[f32;2];4], nu, nv }` (**grade em perspectiva sobre um plano** por homografia; reaproveita `aiai_paint::warp::Homography`), `FrameGrid { thirds | golden | custom }`.
- **Interação (ferramenta Régua `R`)**: clicar cria o tipo escolhido no painel; **alças**: ponto de fuga arrastável; alça azul move o ponto ao longo da guia, alça branca gira o ângulo (como no CSP); alça do **horizonte** (move/rotaciona); `Shift+arrastar` alça = infinitizar o ponto (linhas paralelas); **Fixar horizonte / Fixar ponto** (cadeados, cinza); `Alt+arrastar` move só o ponto; `Delete` apaga o ponto (régua de um ponto apagada = régua apagada, como no CSP).
- **Snap** (`snap` por régua e global `Ctrl+Shift+;`): Caneta, Pincel, formas e a ferramenta Pontos **encaixam** na linha de fuga mais próxima do ponto de fuga escolhido no **primeiro ponto do traço** dentro de uma tolerância em px; modo "forçar direção" (o traço segue o raio do ponto mesmo se a mão desvia, soltar termina); indicador visual do ponto/raio ativo; `Alt` segurado liga/desliga temporariamente.
- **Overlay**: linhas finas na cor/opacidade da régua, N raios por ponto (densidade ajustável 8..64), horizonte tracejado, alças só com a ferramenta Régua ou a régua selecionada; **cache**: renderização por `Painter` (vetor, sem textura); visibilidade `Ctrl+;`.
- **Painel `Réguas`**: lista (olho, cadeado, snap, cor, nome); menu `+`: Perspectiva 1/2/3 pontos, Olho de peixe, Paralelas, Radial, Simetria, Linha, Grade em perspectiva, Grade de enquadramento; Propriedades da selecionada: tipo, coordenadas numéricas dos pontos, ângulo e altura do horizonte, densidade, cor/opacidade, botões **Fixar horizonte**, **Fixar ponto**, **Infinitizar**; **`A partir da câmera 3D`** (usa `vanishing_points` de `aiai-guide3d` para posicionar os pontos a partir da `Camera3d` do Guia 3D, o equivalente do "régua de 3 pontos casando com a câmera do modelo 3D" do CSP); **`Converter em traços`** (assa na camada, é o `perspective_command` existente, para quem quiser a régua impressa); duplicar/mesclar.
- **Paridade MCP**: novas ações `RULER` (criar/editar/apagar/listar) e ferramenta `aiai_ruler`; `aiai_perspective` passa a poder gravar uma régua editável em vez de traços (parâmetro `as_ruler: true`).

**Modelo 3D como guia de layout** (`guide3d` no Layout, `viewport3d` na Modelagem): importa `.glb/.gltf/.obj/.vrm`, posiciona e gera linhas/sombra de referência (detalhe no item 10 e na lista de importadores). No Layout o painel faz o **trabalho leve** (importar, mover/escalar o modelo, escolher câmera e lente, atualizar a camada de guia), enquanto a **Modelagem** é onde se monta cena e se posa o manequim com mais espaço; o mesmo `Scene3d` é compartilhado (guardado no projeto como `guide_scenes`), e o botão `Abrir na Modelagem` leva de um lado a outro. **Orbitar no canvas** (`O`): arrastar gira a câmera do Guia 3D, a camada de linhas regenera com debounce de 300 ms (render de linhas em resolução reduzida durante o arrasto, completo ao soltar). **Calibrar por foto** (futuro): clicar dois pontos de fuga na imagem do SubView para ajustar a câmera (não necessário na primeira entrega).

### c.3 Composição: linha do tempo estilo AE e navegador de efeitos

Ver item 8 de (b): `Linha do tempo da composição` (colunas, expansão por propriedade, cronômetro/chaves, editor de gráfico, barra de trabalho, atalhos de AE) e `Efeitos e predefinições` + `Controles de efeito`. Pendências de motor: **animar parâmetros de efeito** (hoje JSON estático), **modo de matte entre camadas**, **parent** entre camadas, **máscara por camada** e **expressões** (fora do escopo).

### c.4 Edição: faixas de áudio e vídeo separadas, transições, filtros e legendas

Ver item 12 de (b). Detalhes das **legendas** (`captions`; como o painel Texto/Legendas do Premiere):

- Lista de entradas (início, fim, duração, texto, falante, estilo, idioma), edição em linha; várias **faixas de legenda por idioma** (PT, EN, JA) `S1..Sn`; arrastar na linha do tempo muda tempo; `Enter` divide, `Ctrl+Enter` junta.
- **Gerar do roteiro**: usa as falas (original/tradução do `script`) e as **durações reais dos clipes de TTS** ou posições dos clipes de diálogo para sincronizar; atualiza quando o áudio muda ("legendas desatualizadas" com botão Ressincronizar).
- **Estilo**: fonte, tamanho, contorno e sombra, caixa, cor, posição vertical/margens, quebra automática de linha (já existe na queima), **estilo por falante** (cor por personagem), pré-visualização no monitor do programa.
- **Verificação**: limites configuráveis de **caracteres por linha** e **caracteres por segundo** (padrões de partida 42 e 17 **[não verificado: conferir guia de legendagem do destino]**), duração mínima/máxima, sobreposição, linhas fora da fala; aviso por entrada (ícone) e na `Verificação final`.
- **Importar/exportar** SRT/ASS/VTT; **queimada** ou **ao lado** (`sidecar_srt` já existe); múltiplos idiomas exportam um arquivo por idioma.
- Ancoragem ao clipe resolve a lacuna 18 (ver modelo v2).

### c.5 Produção: lista, miniaturas, grande, gráficos, gargalos e IA opcional

Ver item 14 de (b). Resumo do que depende de motor: **histórico de status** (do WAL), **prazos/responsáveis/peso** como campos novos em `StageState` (cada etapa do corte: `due`, `assignee`, `estimate_h`), e `egui_plot` ou desenho próprio.

### c.6 Áudio: editor nível Audacity + UI de TTS opcional de alta qualidade

Editor: item 11 de (b). **Painel `Voz sintética`** (`tts`, em Áudio e atalho a partir de Falas e do Roteiro):

**Motores** (cada um é um *sidecar* local por HTTP/WAV, nunca ligado ao código do app, regra 4 de licenças):

| Motor | Papel | Idiomas | Hardware | Licença (relida nas páginas) | Observação |
|---|---|---|---|---|---|
| **Kokoro-82M** (já feito) | padrão leve | PT-BR, EN | CPU | Apache-2.0 (`licencas-voz.md`) | sem emoção, só velocidade e mistura de vozes |
| **VOICEVOX** (já feito) | japonês | JA | CPU | engine LGPL v3; termos de cada voz; crédito obrigatório | velocidade, altura, entonação, volume, edição de moras |
| **Style-Bert-VITS2** (opcional HQ) | japonês expressivo | JA (JP-Extra); outros por modelo | síntese **roda em CPU**, treino exige GPU NVIDIA; CPU-only instalável | **AGPL-3.0** (`text/user_dict` LGPL-3.0) ([repo](https://github.com/litagin02/Style-Bert-VITS2)) | **vetores de estilo** com intensidade contínua; servidor `server_fastapi.py` (limite padrão 100 caracteres por chamada) → usar como **processo separado** por HTTP (AGPL não contamina o app); cada modelo de voz tem **termos próprios** (3 arquivos: `config.json`, `.safetensors`, `style_vectors.npy`) |
| **IndexTTS 2/2.5** (opcional HQ) | emoção forte, controle de duração | ZH, EN, JA, ES, AR | GPU recomendada (README não dá VRAM) | licença própria da bilibili; **uso comercial não declarado no README** ([repo](https://github.com/index-tts/index-tts), [paper](https://arxiv.org/pdf/2506.21619)) | emoção por vetor de 8 valores (feliz, raiva, triste, medo, nojo, melancólico, surpreso, calmo), texto de emoção, ou áudio de referência (**desabilitar referência de voz de pessoa real**; regra 2) |
| **Fish Speech** | **não incluir** | JA etc. | 12 GB VRAM mín. | **CC BY-NC-SA 4.0 nos pesos** ([resumo](https://www.spheron.network/blog/deploy-open-source-tts-gpu-cloud-2026/)): proíbe uso comercial | listar como "indisponível: licença" para ser transparente |

> Decisão proposta: o HQ padrão é **Style-Bert-VITS2** (japonês, estilos contínuos, CPU ok) e **IndexTTS** como segundo opcional **só se** a licença for confirmada pelo usuário. Releia a licença **antes** de publicar (Princípio de `licencas-voz.md`).

**Painel (layout interno)**:

1. **Motor** (dropdown) + cartão de estado: `Instalado ✓ | Não instalado [Instalar…] | Indisponível (motivo)`, tamanho em disco, **VRAM/RAM mínimas**, licença e texto de crédito exigido, botão `Abrir pasta`/`Desinstalar` (libera disco).
2. **Detecção de hardware** (topo do painel, sempre visível): GPU(s) e VRAM (adaptadores `wgpu` que o app já enumera para `GpuPref`; VRAM por `nvidia-smi --query-gpu=memory.total,memory.free --format=csv` se NVIDIA), versão de driver/CUDA, **RAM livre** (`sysmem.rs`), **espaço livre** nos discos (C: e a pasta de modelos); um **veredito por motor**: `Roda bem | Roda lento (CPU) | Não recomendado | Impossível` com a razão ("GTX 4 GB < 6 GB necessários"; "RAM livre 2 GB < 4 GB"). Seletor `Dispositivo: Auto | GPU | CPU`.
3. **Instalação opcional do modelo**: o botão `Instalar` abre diálogo que mostra **nome do arquivo, origem (URL oficial), tamanho, SHA-256 esperado, licença**; o usuário **precisa confirmar** (política de download); escolhe a **pasta de instalação** (padrão `sidecars/<motor>/` fora do git; sugerir outra unidade se o C: tiver < 10 GB livres); progresso com cancelar e **retomar**; verifica o hash; registra em `licencas-voz.md`/manifesto; nada é baixado sem clique.
4. **Controles por fala** (aplicam à fala selecionada em Falas ou ao texto digitado): `Personagem/voz` (perfil, `aiai_voice_profile`), idioma, **estilo/emoção**: botões de predefinição (Neutro, Alegre, Triste, Raiva, Medo, Surpresa, Sussurro, Grito, Sarcástico, Cansado) **mapeados para cada motor** (SBV2 → nome do estilo + `intensidade` 0–2; IndexTTS → vetor de 8 valores + `emo_alpha`; VOICEVOX → estilo ノーマル/あまあま/…; Kokoro → só velocidade); **intensidade** (slider); **vetor de emoção** em 8 sliders (só IndexTTS, com `random` desligado por padrão); **velocidade**, **tom** (semitons), **energia/entonação**, **pausa antes/depois** (ms) e **pausa entre frases**, **duração-alvo** ("encaixar no tempo do corte": IndexTTS controla duração; os outros usam *time-stretch* limitado a ±12 %), **semente** (determinismo; Kokoro não é determinístico bit a bit, ver `licencas-voz.md`), `nº de takes` (gera 1–5 e escolhe A/B/C).
5. **Ouvir e aplicar**: `▶ Ouvir`, `Comparar takes`, `Aplicar ao corte` (clipe na trilha Diálogo no tempo da fala; crédito gravado no asset), `Gerar pendentes` (fila em segundo plano com progresso por linha, **cache por hash** do `(texto, voz, parâmetros, motor, versão)`, retomável).
6. **Regras** (UI não permite violá-las): sem clonagem de voz de pessoa real (campo de áudio de referência só aceita **áudio gerado pelo próprio app**/gravação do usuário com aviso), créditos obrigatórios sempre adicionados à lista do episódio, nada de nuvem.
7. MCP: `aiai_tts_line`/`aiai_tts_regenerate` ganham `engine`, `style`, `style_weight`, `emotion_vector`, `target_seconds`, `seed`, `device`; nova `aiai_tts_engines` (lista motores, estado, hardware, instalar com confirmação humana).

### c.7 Entrega: fila, formatos, destinos, miniaturas

Ver item 13 de (b).

### c.8 Personagens: pasta do personagem com todo o material

Ver item 2 de (b), em especial o modelo `CharMaterial`/`Outfit`, as seções e a ligação com Animação (`char_ref`), Layout (`char_ref`/`guide3d`), Pintura (`settei`) e Áudio (`Voz`, tabela de boca).

---

## (d) Layout de fábrica × layout personalizado

### d.1 Estado e persistência (hoje não existe)

- **Novo arquivo** `layouts.json` em `%LOCALAPPDATA%\AnimeIAI\` (ao lado de `prefs.json`; não dentro do projeto, porque layout é preferência do usuário): 

```
LayoutStore {
  version: 1,
  workspaces: { <ws_key>: {
      factory_rev_seen: u32,            // revisão de fábrica com que o usuário salvou
      variant: "padrao" | "genga" | "douga" | ...,
      current: Option<SavedTree>,       // árvore em uso se difere da fábrica (None = fábrica)
      named: [ { name, created, tree: SavedTree } ],
      locked: bool
  } }
}
SavedTree { tree: egui_tiles::Tree (serde, painéis por chave de texto), hidden: [panel_key] }
```
- `egui_tiles` oferece `serde`; `PanelKind` precisa de `Serialize/Deserialize` **por `key()`** (string estável), nunca por índice do enum, para sobreviver à inserção de novos painéis. Carregar: chaves desconhecidas descartadas (painel removido/renomeado → some em silêncio e registra no log), raiz vazia → fábrica.
- **Salvar**: ao terminar o arrasto/fechar aba/redimensionar (debounce 500 ms) e ao fechar o app; escrita atômica (temp + rename).
- **`factory_rev`**: cada workspace tem `FACTORY_REV` (constante). Se a revisão de fábrica for **maior** que a salva, mostrar faixa discreta no topo do workspace: "O layout padrão deste workspace foi atualizado. [Ver novo padrão] [Manter o meu]" — nunca sobrescrever o layout do usuário sem pedir; um "Ver novo padrão" abre a fábrica como layout temporário (`Esc` volta).
- **Indicador**: um ponto pequeno na aba do workspace quando o layout está personalizado (tooltip "Layout personalizado — clique para restaurar o padrão"); a barra de status mostra "Layout: Padrão / Personalizado / <nome>".

### d.2 Menus (Janela e Exibir)

- **Janela > Layout** (submenu, por workspace atual):
  - `Padrão de fábrica` e as **variantes de fábrica** (`Genga`, `Douga` na Animação; `Montagem`, `Edição`, `Legendas`, `Cor`, `Áudio` na Edição; `Edição de forma de onda`, `Multipista`, `Voz`, `Mixagem` no Áudio; `Gestão`, `Análise`, `Revisão` na Produção): rádio, marca a ativa.
  - **Meus layouts**: lista de nomeados (clique aplica; ✓ no ativo), `Salvar layout atual como…` (diálogo, `Enter` confirma, nome único por workspace), `Atualizar "nome" com o layout atual`, `Renomear`, `Excluir` (confirma), `Exportar…`/`Importar…` (`.aiai-layout.json`, valida as chaves de painel).
  - `Restaurar layout padrão` (o atual `menu.window.reset`), `Restaurar todos os layouts` (o atual `reset_all`, agora pedindo confirmação), `Travar painéis` (cadeado: sem arrastar/fechar, como "Lock Room Panes" do [OpenToonz](https://opentoonz.readthedocs.io/en/latest/interface_overview.html)).
- **Exibir > Painéis**: lista **todos** os painéis disponíveis neste workspace (por `PanelMeta::workspaces`/`needs_canvas`), agrupados por categoria e com busca; ✓ = visível; os que ainda **não estão na árvore são inseridos** ao marcar: como aba junto ao painel de mesma categoria; senão, como nova coluna/linha segundo `dock_hint` (Left/Right/Bottom/Center) com a proporção padrão 0,22/0,25; os "(recomendado)" são os da fábrica; `Esconder painéis laterais` (`Tab`), `Maximizar painel sob o cursor` (`Shift+` ` ` ` ou duplo clique no cabeçalho, como o Premiere), `Restaurar este painel` (volta ao lugar de fábrica).
- **Variantes de fábrica não apagam personalizações**: cada variante tem sua própria entrada em `current`; trocar de `Genga` para `Douga` e voltar mantém o que o usuário ajustou em cada.

### d.3 Regras de implementação

- A árvore de fábrica continua sendo o `default_node(ws, variant)` (função pura, testada). Mudar `PanelKind` ou a fábrica **incrementa `FACTORY_REV`** do workspace (um teste compara o hash da descrição declarativa com o `FACTORY_REV` esperado para impedir esquecer).
- Testes novos: (1) todo workspace tem fábrica com ≥ 1 painel exclusivo (impede workspaces indistinguíveis; ex.: a interseção de painéis entre Layout, Cenário, Pintura e Composição deve ser ≤ 4 genéricos); (2) serialização ida e volta da árvore; (3) chave desconhecida é descartada; (4) variante troca sem perder a personalização; (5) `Exibir > Painéis` insere painel ausente.
- **Bridge/MCP**: `UI_ACTION` ganha `layout.apply`, `layout.save`, `layout.reset`, `layout.list`, `panel.show/hide` (o MCP `aiai_ui` já navega a UI) para o agente conseguir validar visualmente cada workspace (princípio de validação visual).
- Orçamento de memória (8 GB): painéis novos só desenham quando visíveis (a `pane_ui` do egui_tiles já pula abas escondidas); caches de miniaturas com teto por painel.

---

## (e) Backlog priorizado (S = pequeno ≤ 1 dia, M = 2–4 dias, L = 5+ dias)

### Fase 0 — Fundação (destrava tudo; fazer primeiro)

| # | Item | Tam. |
|---|---|---|
| 0.1 | `WorkspaceProfile` (ferramentas por workspace, ferramenta padrão, overlays) substituindo `has_canvas`; prateleira lê o perfil | M |
| 0.2 | `PanelMeta` (categoria, dock_hint, workspaces, needs_canvas) + testes de catálogo e i18n PT/EN dos painéis novos | S |
| 0.3 | Persistência de layout (`layouts.json`, serde por chave, `factory_rev`, carregar/descartar) | M |
| 0.4 | Layouts nomeados + variantes de fábrica + menu Janela > Layout (+ exportar/importar, travar) | M |
| 0.5 | Exibir > Painéis completo (inserir painel ausente, busca, categorias, maximizar painel) | M |
| 0.6 | Renomear `Cor`→`Biblioteca` (alias `cor`), adicionar `Modelagem` (esqueleto com painéis "em construção" + ícones), grupos na barra | M |
| 0.7 | `Keymap` com contextos + novos atalhos por workspace + tela de atalhos agrupada | M |
| 0.8 | Teste "workspaces distinguíveis" (interseção de painéis) | S |

### Fase 1 — Parar de parecer tudo igual (usa o que já existe)

| # | Item | Tam. |
|---|---|---|
| 1.1 | Fábricas novas de Layout/Cenário/Pintura/Animação(Genga, Douga)/Composição com os painéis atuais reorganizados + perfis de ferramenta | S |
| 1.2 | Painel `Cena` (`scene_info`: roteiro da cena, painel do storyboard, tempo) | M |
| 1.3 | Painel `Referências do personagem` (`char_ref`) v1 com a folha existente | M |
| 1.4 | **Mesa de luz / onion skin** (modelo de opções, composição de cels vizinhos em cache, atalhos `O`/`Shift+O`, painel) | L |
| 1.5 | Marcas K/B/I nas células e atalhos `K`, `Shift+K`, `Insert`, `Ctrl+→/←` | S |
| 1.6 | Linha do tempo compacta (`density`, grupos recolhíveis, linha de áudio fina, trilha-resumo de chaves) | M |
| 1.7 | Seletor de fase Genga/Douga (`cut.phase`, variantes de layout, trava de camada Genga em Douga) | M |
| 1.8 | Mover `GRADE` da Cor para a Composição (`color_grade`), extrair `effect_controls` e `camera` de `Properties` | M |
| 1.9 | `Faixa de cortes` (`cut_strip`) | S |
| 1.10 | Painel `Settei` (grade região×tom) e `Verificação` (lint) da Pintura | M |
| 1.11 | `Fila de pintura` com propagação de cores em lote | M |

### Fase 2 — Desenho e recursos (Layout, Cenário, Personagens, Biblioteca)

| # | Item | Tam. |
|---|---|---|
| 2.1 | **Réguas de perspectiva** (dados na camada Guide, overlay, ferramenta `R`, alças, snap, painel, MCP `RULER`) | L |
| 2.2 | Réguas: olho de peixe, paralelas, radial, simetria, grade em perspectiva (homografia) | M |
| 2.3 | `Guia 3D` no Layout (UI sobre `aiai_guide3d`: objetos, câmera, atualizar camada, orbitar `O`) | L |
| 2.4 | Importar `.glb/.gltf/.obj/.vrm` → `Mesh` (+ licença/fonte, limite de triângulos, blob) | M |
| 2.5 | Pasta do personagem: modelo de dados `CharMaterial/Outfit` + seções Folha, Expressões, Boca, Olhos, Figurinos | L |
| 2.6 | Pasta do personagem: Modelo 3D, Poses, comparar A/B, migrar `consistência` e `voz` para abas | M |
| 2.7 | Biblioteca: modelo `LibItem`, armazenamento (usuário/projeto), árvore, grade, prévia, importação | L |
| 2.8 | Biblioteca: textura com teste 3×3, turntable 3D, extração de paleta, editor de paletas | M |
| 2.9 | Cenário: `Variantes e câmera`, `Roteiro de cor`, `Céu e gradiente`, `Texturas`, modo valores, ferramentas (`S`, `Shift+G`, `Shift+F`, `Ctrl+T`) | L |
| 2.10 | Câmera do Layout: retângulos início/fim + predefinições PAN/TU/TB | M |
| 2.11 | `Folha de exposição` vertical (editar, segurar, K/B/I, coluna de fala e câmera; XDTS) | L |
| 2.12 | Intervalos: inserir N intercalações, gráfico de espaçamento, ferramenta `Q` | M |
| 2.13 | Modelagem v1: Outliner, Viewport 3D (prévia por software), primitivas, manequim e pose, câmera, "Enviar ao Layout" | L |
| 2.14 | Modelagem v2: viewport wgpu, gizmos, quatro vistas, estruturas prontas | L |

### Fase 3 — Pós-produção (Composição, Áudio, Edição)

| # | Item | Tam. |
|---|---|---|
| 3.1 | Composição: viewer sem pintura (resolução, xadrez, canais) + ferramentas `V H Z W Y C` | M |
| 3.2 | `Linha do tempo da composição` v1 (colunas, barra de duração, expansão por grupo, chaves de transformação já existentes) | L |
| 3.3 | Comp: editor de gráfico, interpolações, `Easy Ease`, atalhos de AE | L |
| 3.4 | `Efeitos e predefinições` (árvore, busca, favoritos, miniaturas, arrastar-soltar, predefinições de usuário) | M |
| 3.5 | Animar parâmetros de efeito no motor + cronômetro em `Controles de efeito` | L |
| 3.6 | Modo matte, parent, camadas de ajuste na Composição (motor) | L |
| 3.7 | Painéis `Planos` (vista lateral), `Escopos`, `Informações`, `Prévia` com cache | M |
| 3.8 | Áudio: `Forma de onda` + espectrograma + seleção + cortar/copiar/silenciar/aparar | L |
| 3.9 | Áudio: efeitos de Audacity (Amplificar, Normalizar, Fade, EQ gráfico, Compressor, Limitador, Noise gate, Redução de ruído, Reverb) em `aiai-audio` | L |
| 3.10 | `Rack de efeitos` em tempo real por clipe/trilha | L |
| 3.11 | `Multipista` (cabeçalho Audacity, envelopes, rótulos, picture ref) + `Medidores` LUFS/true-peak/LRA | L |
| 3.12 | `Falas` (cue list ligada ao roteiro) + fila de geração em lote com cache | M |
| 3.13 | **EditDoc v2** (trilhas separadas, itens com link, legendas ancoradas, migração v1→v2, `aiai_edit` esquema v2) | L |
| 3.14 | Edição: nova linha do tempo (V/A separadas, ferramentas `V A B N C Y U H Z T`, atalhos, snap, links) | L |
| 3.15 | Edição: transições novas + painel `Transições` + arraste ao corte | M |
| 3.16 | Edição: filtros por clipe, camada de ajuste, retime, Cor/Escopos (reaproveita Composição) | L |
| 3.17 | Edição: painel `Legendas` (várias faixas, gerar do roteiro, estilo, verificação cps, importar/exportar) | L |
| 3.18 | Edição: monitor de origem, marcas I/O, inserir/sobrescrever; reprodução com áudio ao vivo | L |

### Fase 4 — Gestão, entrega e voz de alta qualidade

| # | Item | Tam. |
|---|---|---|
| 4.1 | Produção: índice de histórico (eventos de status do WAL) + campos prazo/responsável/peso | M |
| 4.2 | Produção: visões `Miniaturas` e `Grande` | M |
| 4.3 | Produção: `Lista` melhorada (colunas, agrupar, filtros, visões salvas, ações em lote) | M |
| 4.4 | Produção: `Quadro` (kanban) e `Cronograma` | L |
| 4.5 | Produção: `Gráficos` (barras por etapa, rosca, matriz, burndown, fluxo cumulativo, vazão, tempo por etapa, retake) + export CSV | L |
| 4.6 | Produção: `Gargalos` determinístico + filtros por clique | M |
| 4.7 | Produção: `Análise por IA` (pacote, notas com carimbo, opcional modelo local) | M |
| 4.8 | Entrega: fila ampliada (progresso por quadro, ETA, reordenar, pausar, persistir, matriz de exportação) | L |
| 4.9 | Entrega: `Destinos` (modelo de nome, cópia automática, pacote de entrega com créditos) | M |
| 4.10 | Entrega: `Entregáveis` com pôster/miniatura + editor de pôster | M |
| 4.11 | Entrega: `Verificação final` (preflight) | M |
| 4.12 | Entrega: formatos extras (H.265, WebM, stems, folha de contato) | M |
| 4.13 | TTS: painel `Voz sintética` (motor, hardware, controles por fala, takes, cache) sobre Kokoro/VOICEVOX | M |
| 4.14 | TTS HQ: instalador opcional com confirmação/hash/pasta, detecção de GPU/VRAM, Style-Bert-VITS2 (HTTP sidecar) | L |
| 4.15 | TTS HQ: IndexTTS (vetor de emoção, duração-alvo) — só após confirmar licença | L |
| 4.16 | Storyboard com canvas (painel desenhável no próprio workspace) | L |
| 4.17 | Roteiro: Elenco, Notas, Estatísticas, Beats em aba | S |

### Ordem de entrega sugerida (menor caminho até "cada workspace é diferente")

`0.1 → 0.2 → 0.3 → 0.4 → 0.5 → 1.1 → (1.2, 1.3) → 1.4 → 1.5 → 1.6 → 1.7 → 0.6 → 1.8 → 2.1 → 2.4 → 2.3 → 2.5 → 2.7 → 3.1 → 3.2 → 3.4 → 3.13 → 3.14 → 3.8 → 3.11 → 4.1 → 4.3 → 4.5 → 4.8 → 4.13`.

O usuário vê a diferença real já ao fim da Fase 1 (cada workspace com layout próprio, perfil de ferramentas próprio e os painéis centrais da Animação). As fases 2–4 podem seguir em paralelo por área porque tocam crates diferentes (`aiai-guide3d`/`aiai-tools` · `aiai-fx` · `aiai-audio` · `aiai-tools/edit.rs` · `production`).

---

## Perguntas em aberto (para decidir antes da Fase 2)

1. **Storyboard com canvas próprio** (4.16) ou continuar abrindo o desenho em outro workspace? A pesquisa recomenda canvas próprio.
2. **Biblioteca e Modelagem na barra**: manter a posição pedida (com separadores) ou levá-las para o grupo "Recursos" no fim? É uma troca de uma linha em `ALL`.
3. **IndexTTS**: licença da bilibili sem declaração de uso comercial; o micro anime pode ser comercial? Se sim, adiar até confirmar.
4. **Modelos 3D**: aceitar `.vrm` (modelos de anime, licenças por modelo) ou só glTF/OBJ próprios e CC0?
5. **Prévia 3D**: começar pela prévia por software (rápida de entregar) e só depois wgpu, ou já wgpu? A pesquisa recomenda por software primeiro.
6. **IA de produção**: só o pacote para o agente MCP (recomendado) ou também modelo local configurável?
7. O **padrão de página da folha de exposição** (6 s por página, colunas) deve ser conferido contra o XDTS exportado e, se possível, um modelo de folha real de estúdio antes de implementar `timesheet`.

## Fontes (todas as citadas em linha)

Pipeline: [Kanzenshuu](https://kanzenshuu.com/animation-production/positions-and-roles) · [Toei/SIGGRAPH 2021](https://history.siggraph.org/wp-content/uploads/2025/09/2021-SA_Courses_Japanese-Animation-in-2021-To-CG-or-not-CG.pdf) · [Animenation](https://www.animenation.net/blog/?p=1370) · [animeanime.jp](https://animeanime.jp/article/2024/10/31/87413.html) · [CGWORLD タイムシート](https://cgworld.jp/terms/タイムシート.html) · [Wikipedia Anime](https://en.wikipedia.org/wiki/Anime) · [RETAS](https://en.wikipedia.org/wiki/RETAS).
Ferramentas: [CSP animação](https://clipstudio.net/en/animation) · [CSP dicas](https://tips.clip-studio.com/en-us/articles/9951) · [CSP onion skin](https://help.clip-studio.com/en-us/manual_en/600_animation/Onion_skin.htm) · [CSP perspective rulers](https://help.clip-studio.com/en-us/manual_en/510_ruler/Perspective_Rulers.htm) · [CSP fisheye](https://tips.clip-studio.com/en-us/articles/7623) · [Harmony](https://docs.toonboom.com/help/harmony-24/premium/getting-started/interface.html) · [Storyboard Pro](https://docs.toonboom.com/help/storyboard-pro-20/storyboard/reference/workspaces/about-workspaces.html) · [OpenToonz](https://opentoonz.readthedocs.io/en/latest/interface_overview.html) · [onion/light table genérico](https://gitnux.org/best/onion-skin-animation-software/) · [AE workspaces](https://helpx.adobe.com/hu/after-effects/desktop/get-started/get-familiar-with-the-interface/workspaces-panels-viewers.html) · [AE layout padrão](https://blog.fmctraining.com/blog/after-effects-tutorial-understanding-workspaces-panels) · [AE atalhos](https://motiondesign.school/after-effects-keyboard-shortcuts/) · [Premiere workspaces](https://helpx.adobe.com/ee/premiere/desktop/get-started/tour-the-workspace/what-are-workspaces.html) · [Premiere lista](https://videoguys.com/blogs/news-and-sales/premiere-bro-tutorial-working-workspaces) · [Premiere atalhos](https://blog.motionarray.com/learn/premiere-pro/ultimate-list-of-adobe-premiere-keyboard-shortcuts/) · [Audition waveform](https://helpx.adobe.com/audition/using/displaying-audio-waveform-editor.html) · [Audacity track panel](https://manual.audacityteam.org/man/track_control_panel_and_vertical_scale.html) · [Blender workspaces](https://docs.blender.org/manual/en/latest/interface/window_system/workspaces.html) (só a existência da página; conteúdo não lido).
Tracking: [Kitsu](https://kitsu.cg-wire.com/) · [Kitsu estatísticas](https://blog.cg-wire.com/lee-film-studio-sponsors-enhanced-statistics-for-kitsu) · [Flow/ShotGrid](https://www.autodesk.com/eu/products/shotgrid/features) · [ftrack](https://www.ftrack.com/en/features).
TTS e 3D: [Style-Bert-VITS2](https://github.com/litagin02/Style-Bert-VITS2) · [IndexTTS](https://github.com/index-tts/index-tts) · [IndexTTS2 paper](https://arxiv.org/pdf/2506.21619) · [Fish Speech/guia](https://www.spheron.network/blog/deploy-open-source-tts-gpu-cloud-2026/) · [crate gltf](https://openapps.pro/packages/gltf) · [tobj](https://docs.rs/crate/tobj/2.0.0) · [russimp](https://crates-io.ganymede-static.progval.net/crates/russimp/2.0.6) · [Poly Haven licença CC0](https://polyhaven.com/license).

**Limites da pesquisa**: páginas da Adobe (helpx) bloquearam a leitura direta (403); o que se diz de AE/Premiere/Audition vem de páginas de terceiros e de resultados de busca. Atalhos de Premiere para inserir/sobrescrever (`,` `.`) e J/K/L ficaram **[não verificados]**. Páginas oficiais de TVPaint e o padrão exato da folha de exposição japonesa não foram encontrados. Tudo o que está marcado **[não verificado]** deve ser conferido antes de virar requisito.
