# S24, pesquisa do núcleo de desenho e pintura (AnimeIAI x Photoshop, Clip Studio Paint EX, Procreate, Krita)

Data: 2026-10-19. Somente leitura: nenhum código foi alterado e nada foi compilado.
Base analisada: `HEAD` (`3e5378d`). Importante: no momento da auditoria **outra sessão está editando, sem commit,
`canvas.rs`, `editor.rs` e `tools.rs`** com parte dos pedidos 1 e 2 (caixa de seleção de traços, 8 alças de escala não
uniforme com Shift/Alt, Ctrl+T/Ctrl+A/Ctrl+D). Cada item abaixo diz o que vale no HEAD e o que a árvore de trabalho já
cobre (marcado como **[em andamento]**). Linhas `arquivo:linha` são do HEAD, salvo `layers.rs`, `keymap.rs`, `compose.rs`
e demais arquivos que não estão modificados. As referências a outros apps vêm de pesquisa na web quando indicado, e do
conhecimento do uso dos programas nos outros casos (marcado com "(conhecimento)").

Convenção de estado: **ok**, **parcial**, **ausente**, **quebrado**.

---

## (a) Inventário

### a.1 Ferramentas da prateleira

`ToolId` tem só 10 entradas (`aiai-studio/src/workspace.rs:~113`): Mover, Pincel, Caneta, Editar pontos, Borracha, Balde,
Seleção (Laço, com modos), Conta-gotas, Mão, Zoom. Atalhos em `keymap.rs:138-147`.

| Recurso | Estado | Evidência | Como os outros apps fazem |
|---|---|---|---|
| Pincel raster com motor próprio (ponta de forma/imagem, textura, dual, jitter, espalhamento, aquarela, borrador, pressão) | ok, acima da média | `aiai-paint/src/brush.rs:257` (`BrushPreset`), `brush_import.rs` (.abr, .brush, .bundle, .gbr) | CSP/Krita/Procreate equivalentes. Faltam inclinação, rotação da caneta e velocidade como dinâmica (`grep tilt\|velocity` em brush.rs: 0 resultados) |
| Caneta vetorial (G-pen etc., perfil de largura, estabilizador, formas por régua) | ok | `tools.rs:182` `commit_pen`, `editor.rs:~261` `PenShape` | CSP: camada vetorial com ferramenta Objeto, Linha de controle |
| Editar pontos, alças, id estável de ponto | ok, diferencial | `app/nodes.rs`, `aiai-tools/src/points.rs` | CSP "Control point"; Illustrator |
| Borracha raster e vetorial (trecho, interseção, traço inteiro) | ok | `aiai-tools/src/erase.rs`, `tools.rs:483,501` | CSP "Vector eraser" idêntico |
| Balde com fechamento de frestas, lint, regiões por camada de linha | ok, diferencial de produção | `aiai-tools/src/fill.rs`, `tools.rs:566` | CSP "Fill > Refer other layers / Close gap". Falta tolerância/referência "todas as camadas" na UI (`ToolOptions` só tem `fill_gap`, `fill_expand`, `fill_close_leftovers`, `editor.rs:~484`) |
| Gradiente | parcial (só MCP) | `aiai-tools/src/paint.rs:325` `gradient_command`; sem ferramenta na UI (nenhum `gradient` em `aiai-studio` além do bridge) | PS/CSP/Krita/Procreate: ferramenta Gradiente com pontos, mapa de gradiente |
| Formas (retângulo, elipse, polígono preenchido) | parcial | só como `PenShape` da caneta (traço fechado, sem preenchimento): `tools.rs:231`; preenchida só por `aiai_fill_shape` (MCP) | CSP "Figure" (formas editáveis após criar); PS shapes |
| Texto | ausente na UI | ver (b)4 | PS/CSP/Procreate/Krita têm ferramenta Texto |
| Conta-gotas | ok | `tools.rs:628` | Alt para pegar cor em todos |
| Mão/Zoom/rotação e espelho da vista | ok | `editor.rs:25-115`, botão em `canvas.rs:200` | CSP/Procreate idem. Falta navegação por gesto (conhecimento) |
| Cola/recorta/copia/duplica | ausente | Menu Editar só tem Desfazer, Refazer, Preferências (`app.rs:372-390`); nenhum clipboard (`grep clipboard`: 0) | Ctrl+C/X/V, Ctrl+J, colar vira nova camada |
| Mover | parcial (ver b1, b2) | `canvas.rs:489-512`, `tools.rs:727-770` | |

### a.2 Seleção

| Recurso | Estado | Evidência | Referência |
|---|---|---|---|
| Retângulo, elipse, laço à mão livre, varinha | ok básico | `SelMode` `editor.rs:271`, `canvas.rs:485-488`, `tools.rs:688` `wand_select` | PS/CSP idem |
| Somar, subtrair, interseção (Shift, Alt) | ausente na UI; existe no motor | `aiai-paint/src/select.rs:32` `SelectOp`, `combine` `:136`; `finish_select` (`tools.rs:637`) sempre substitui | PS: Shift/Alt/Shift+Alt; CSP: modos no Tool Property |
| Seleção poligonal, por cor (faixa de cor), por camada | ausente na UI; faixa de cor existe no motor | `SelectShape::ColorRange` `select.rs:27` não ligado na UI | CSP "Select color range" e Ctrl+clique na miniatura da camada; PS idem |
| Inverter, expandir/contrair, suavizar borda (feather) | ausente na UI; existe no motor | `select.rs:148 grow`, `:167 feather`, `:191 invert`; nenhum chamado em `aiai-studio` | CSP Selection Launcher: Inverter, Expandir, Contrair |
| Máscara rápida (quick mask), salvar seleção em canal | ausente | | PS Q; CSP Quick mask; Krita máscara de seleção local |
| Seleção limita pincel, borracha, balde, gradiente | **ausente** | `grep selection` em `tools.rs` fora de mover/varinha: 0; `aiai-tools/src/paint.rs`, `fill.rs`, `erase.rs`: 0. A seleção só serve para Mover/transformar | Toda a categoria: pintar só dentro da seleção |
| Seleção guarda a máscara de verdade | parcial | `Selection` guarda só a forma (pts) ou máscara de varinha (`editor.rs:303`); não há bitmap persistente de seleção composto de várias formas | PS/CSP: máscara de 8 bits, editável |
| Launcher de seleção (barra flutuante: transformar, apagar, preencher, inverter, copiar...) | ausente | | CSP Selection Launcher (conhecimento) |
| Seleção de objetos vetoriais (clique, Shift soma) | ok; caixa **[em andamento]** | `tools.rs:836` `move_pick` | CSP "Object" tool: arrastar caixa pega todos os traços tocados |
| Selecionar tudo (Ctrl+A), desfazer seleção (Ctrl+D) | ausente no HEAD; **[em andamento]** | `keymap.rs:138-166` não tem; árvore de trabalho: `tools.rs:1018-1029` | |

### a.3 Transformação

Motor (`aiai-tools/src/transform.rs`) é bem mais rico que a UI.

| Recurso | Estado | Evidência | Referência |
|---|---|---|---|
| Afim: translação, escala X/Y, rotação, cisalhamento | motor ok; UI usa só translação, escala uniforme e rotação | `Deform::Affine` `transform.rs:13-25`; UI `tools.rs:748` passa `scale: [sel.scale, sel.scale]` e `shear: [0,0]`; `Selection.scale: f32` (`editor.rs:311`) | PS Ctrl+T; CSP Ctrl+T |
| Escala não uniforme (alças de lado, canto livre) | ausente no HEAD; **[em andamento]** (`Selection.scale: Vec2`, `scale_drag`) | `canvas.rs:569-573` (HEAD: só `ScaleSel` de canto, distância ao centro) | |
| Distorcer, perspectiva, malha, liquify, espelhar | motor ok (`Deform::Perspective/Mesh/Liquify/Flip`, `transform.rs:27-33`), **sem UI** (só MCP `aiai_transform`) | `warp.rs:13` `Homography`, `:126` `MeshWarp`, `:225` `Liquify` | PS Distort/Perspective/Warp; CSP Mesh Transformation (até 20 pontos), Liquify; Krita Warp, Cage, Liquify; Procreate Distort/Warp |
| Pivô movível, entrada numérica, Enter aplica/Esc cancela | parcial: Enter/Esc existem (`tools.rs:1000,1006`); pivô fixo no centro, sem campos numéricos | `Selection::center` `editor.rs:328` | PS: barra de opções X, Y, L, A, ângulo |
| Transformar raster sem perda (reamostragem acumulada) | **quebrado/parcial**: cada commit reamostra o canvas inteiro e grava `SetTiles` de toda a área; fora do canvas é cortado para sempre; repetir escala borra | `transform.rs:~185-205` (`tiles_for_region([0,0,w,h])`, ramo Raster) | PS Smart Object; CSP guarda a transformação até confirmar e a camada pode ser maior que o canvas; Procreate idem reamostra |
| Prévia ao vivo | ok | `move_live.rs` | |
| Alças aparecem ao selecionar tudo / camada inteira | ausente | alças só com `!sel.whole` (`canvas.rs:1015`) | PS: Ctrl+T numa camada sem seleção mostra a caixa |
| Transformação de vários itens com camadas selecionadas | ausente | `env.ed.layer: Option<NodeId>` | PS/CSP: transformar camadas selecionadas ao mesmo tempo |

### a.4 Camadas

| Recurso | Estado | Evidência | Referência |
|---|---|---|---|
| Tipos: Vetor, Raster, Preenchimento, Pasta, Guia, Referência, Efeito, Partículas, Ajuste, Texto | parcial: menu `+` cria só 8 (`layers.rs:13-22` `ADD_KINDS`); **Ajuste e Texto não são criáveis na UI** | `model.rs:427-439` | PS/CSP todos |
| Modos de mistura | ok (17) | `commands.rs:181` `BLEND_MODES`, `blend.rs:100` | Faltam Color Dodge, Color Burn, Linear Burn/Light, Vivid, Pin, Hard Mix, Divide (CSP/PS/Krita têm; conhecimento) |
| Opacidade, olho, cadeado | ok | `layers.rs:170-258` | Falta **travar pixels transparentes (alpha lock)** (`grep lock_alpha`: 0) e travar posição/pintura separadas |
| Recortar pela camada de baixo ("Recortar") | parcial; causa do bug em b3 | `layers.rs:296`, `compose.rs:305-430` | PS Clipping Mask; CSP "Clip to layer below" |
| Máscara de camada | motor ok (`LayerData.mask`, `model.rs:~515`; `compose.rs` usa), **sem UI** | `grep mask` em `panels/*.rs`: só brush_maker | PS/CSP/Krita: máscara pintável, Ctrl+clique |
| Pastas | parcial (ver b3) | `commands.rs:85` `layer_rows`, `layers.rs:~109` | |
| Multisseleção, arrastar e soltar, duplicar, mesclar, achatar, agrupar, cor de etiqueta, buscar | **ausente** | `Editor.layer: Option<NodeId>`; sem `Duplicate`/`Merge` em `aiai-core/src/command.rs` nem em `commands.rs` | PS/CSP/Krita/Procreate todos |
| Papéis de cel (Chave/Breakdown/Intercalação) | ok, diferencial (anime) | `layers.rs:394-408`, `cmds::set_cel_role` | CSP não tem o papel; só marca de cor |
| Camadas de efeito/ajuste | motor ok (Levels, Curves, HSL, Color Balance, Gradient Map, Threshold, Posterize, Invert: `aiai-paint/src/adjust.rs:11-33`; 26 efeitos em `aiai-fx/src/effects.rs`); UI só para efeitos por camada (`satsuei.rs:163`), **sem UI de camada de ajuste** | | PS Adjustment Layers; CSP "Correction layer" |
| Camada de referência / Sub View / mesa de luz (light table) | parcial: camada Referência e Sub View existem; **sem onion skin** (`grep -i onion`: 0) | `doc.rs:399`, `panels/subview.rs`; `S23-AUDITORIA-BACKLOG.md` lista onion skin como P2 | CSP Onion skin, Light table; Procreate Animation Assist; Krita Onion skins |
| Composições de camadas (layer comps), estilos de camada (sombra, contorno) | ausente (existe `Effect::DropShadow/Outline`, mas só pela pilha de efeitos) | | PS Layer Comps/Styles (conhecimento) |
| Camadas 3D | ausente na pintura: existe `aiai-guide3d` (manequim, câmera) só como guia de linhas via MCP | `aiai-tools/src/guide.rs:43` | CSP 3D layers com poses; Procreate 3D import |

### a.5 Texto, réguas, guias, cor, histórico, exportação

| Recurso | Estado | Evidência | Referência |
|---|---|---|---|
| Texto como contorno (onomatopeia, título) | parcial, só por MCP | `aiai-paint/src/text.rs:93` `text_outline`; `aiai-tools/src/fx2d.rs:24,71`; `aiai-bridge/src/production_tools.rs:88`; descrição `aiai-mcp/src/tools_production.rs:16` | |
| Painel de caractere/parágrafo, edição de texto na tela | ausente | ver b4 | |
| Réguas: perspectiva, simetria, linhas de concentração, régua paralela/curva, snap | **ausente na UI**; perspectiva só como linhas desenhadas por `aiai_perspective` | `aiai-tools/src/guide.rs:109` (cria traços), nenhuma régua com snap (`grep snap`: 0 em studio/tools) | CSP Rulers (perspectiva 1/2/3, simetria, concentração), Procreate Drawing Guide + Symmetry, Krita Assistants + Mirror tools |
| Grade de pixels, moldura de câmera, guias | parcial | `canvas.rs:~205` (`show_grid`, `show_camera`, `show_guides`) | PS guias arrastáveis das réguas |
| Seletor de cor (HSV quadrado + matiz), paleta ligada ao projeto, cor direta | ok; paleta indexada é diferencial de anime | `panels/colors.rs:37-300` | CSP Color set, "Color Mixing"; faltam cores recentes, misturador, harmonias, histórico de cor (`grep recent`: 0) |
| Gerenciamento de cor (perfis ICC, CMYK, sRGB/P3, linear/HDR) | ausente | P2 em `S23-AUDITORIA-BACKLOG.md` ("cor linear/HDR") | PS completo; CSP perfis; Krita OCIO, 16 e 32 bits |
| Histórico: Desfazer/Refazer lineares | ok | `app.rs:374-383`; `aiai-core` WAL | **Sem painel Histórico** (lista de passos clicável, instantâneos); PS History, Krita Undo History, CSP History |
| Exportar: PNG do quadro, PSD em camadas, SVG, XDTS, OTIO, vídeo | ok, acima da média | `aiai-tools/src/export.rs:17,180,231,312,478,606`; `doc.rs:309` | Faltam JPEG/WebP/TIFF/sequência PNG por interface, exportar camadas selecionadas, ICC |
| Importar imagem | parcial: vira camada **Referência** a 60%, não pintável | `doc.rs:399-410` | PS/CSP: vira camada raster editável; colar do clipboard |
| Biblioteca de materiais/assets | ausente | | CSP Materials (Clip Studio Assets), Procreate brushes, Krita bundles (pincéis importados existem) |
| Teclado: Ctrl+Z/Y, B, E, V, G... | parcial | `keymap.rs:138-166` (ferramentas, tamanho, quadro, zoom, arquivo, desfazer, pontos); faltam Ctrl+A/D/T (**[em andamento]**), Ctrl+C/X/V, Ctrl+J, Ctrl+G, Ctrl+E, Delete (apagar seleção/camadas), Ctrl+Shift+N (nova camada), Ctrl+I, X (trocar cores), D (cores padrão) | |

---

## (b) Reclamações explícitas, análise no código e correção proposta

### b1. Caixa de seleção de traços vetoriais

**Diagnóstico (HEAD).** Com a ferramenta Mover, o clique chama `tools::move_pick` (`tools.rs:836-871`). Se há traço sob o
cursor (`stroke_at`, `tools.rs:772`, tolerância de 6 px de tela convertidos para canvas, `canvas.rs:505`), ele é
escolhido. Se não há, `move_pick` só limpa a escolha e devolve `()`. Em seguida o `on_press` cai em
`else if tools::begin_move(env)` (`canvas.rs:509`), que cria uma seleção `whole` e inicia `Gesture::MoveSel`
(`tools.rs:651-658`). Ou seja: **arrastar no vazio move a camada inteira**, nunca desenha uma caixa. Não existe um
`Gesture` de caixa. A seleção de traços só aceita clique, e `Shift` soma/tira um traço por vez.

**Estado na árvore de trabalho [em andamento].** `Gesture::BoxPick` (`editor.rs`), `tools::box_pick`, `move_pick` devolve
`bool` (arrasto vira caixa se começou fora de traço e fora da caixa dos escolhidos), retângulo translúcido em
`paint_gestures`, Shift soma. Critério: o traço entra se **qualquer** amostra dele cai na caixa (toque), igual ao CSP.

**Lacunas que ficam mesmo com isso, para fechar:**
1. Photoshop e CSP distinguem "tocar" (caixa por contato, padrão do CSP Object) de "conter totalmente" (Alt/Ctrl
   alterna). Propor: arrastar da esquerda para a direita = contém tudo, da direita para a esquerda = toca (convenção
   AutoCAD/Illustrator) ou uma opção na ferramenta. Tamanho: S.
2. Arrastar a partir de **dentro** da caixa dos escolhidos move, mas arrastar começando sobre um traço **não** escolhido
   troca a escolha e já move. Manter, e testar com Shift (soma sem mover).
3. Raster: não há equivalente (traços raster fundem em pixels). A "caixa" do raster é a ferramenta Seleção
   (retângulo), que funciona só com a ferramenta Laço, não com o Mover. Propor "Mover com seleção automática" (b2).
4. Teste de mouse real obrigatório (princípio do usuário): caixa em C002 selecionando 3 traços, arrastar, desfazer.
   Sem teste de gesto, a regressão que a S23 teve em "mover" voltaria.

Tamanho do que falta: S.

### b2. Seleção parcial de raster e transformação livre (Ctrl+T)

**Diagnóstico (HEAD).**
- Seleção raster é feita com a ferramenta Laço (modos retângulo, elipse, laço, varinha) em `canvas.rs:485-488`, e depois é
  preciso trocar para o Mover. Não há atalho Ctrl+T, nem Ctrl+A/D (`keymap.rs:138-166`).
- As alças (`canvas.rs:958` `handles`, desenhadas em `paint_selection`, `canvas.rs:997`) só aparecem com a ferramenta Mover
  e `!sel.whole`: **4 cantos + botão de rotação**. Os 4 cantos fazem **escala uniforme em volta do centro**
  (`canvas.rs:569-573`: `s.scale = start_scale * dist/start_dist`). Não há alças de lado, não há escala por eixo.
- `Selection.scale` é `f32` (`editor.rs:311`), então a UI não consegue expressar escala não uniforme, **embora o motor
  já aceite**: `Deform::Affine { scale: [f32;2], shear: [f32;2] }` (`transform.rs:13-25`) e `commit_move` passa
  `[sel.scale, sel.scale]` e `shear: [0,0]` (`tools.rs:748`).
- Distorcer, perspectiva, malha e liquify existem no motor e no MCP (`aiai_transform`), sem nenhuma superfície na UI.
- A varinha só lê a camada ativa (`tools.rs:688`), sem opção "todas as camadas".
- Raster destrutivo e limitado ao canvas (a.3 acima).

**Sobre o comportamento de referência (verificado na web).**
- Photoshop CC 2019 (v20.0) **inverteu** o antigo: a escala é **proporcional por padrão** e o **Shift passa a escalar sem
  manter proporção**; o ícone de corrente ("Maintain Aspect Ratio") na barra de opções define o padrão e o Shift inverte
  o que estiver configurado; formas e caminhos vetoriais continuam não proporcionais por padrão. Há a preferência
  "Use Legacy Free Transform" para voltar ao comportamento antigo (fontes: comunidade Adobe, ver Fontes).
  **Portanto a premissa do pedido ("não uniforme por padrão, Shift mantém a proporção") é o comportamento legado do
  Photoshop e o atual do Krita (conhecimento) e do Clip Studio em Free Transform; o Photoshop atual é o contrário.**
- Clip Studio Paint: por padrão a escala mantém a proporção; "Keep aspect ratio" é uma opção da Tool Property e Ctrl
  alterna para Free Transform (fonte: manual e dicas do CSP). Ctrl+T abre Free Transform com escala, rotação,
  cisalhamento, distorção, perspectiva e espelho; existe também Mesh Transformation (até 20 pontos por eixo) e Puppet Warp.
- Procreate: modos explícitos Uniforme, Livre, Distorcer, Deformar (Warp) mais Magnético (conhecimento).
- Recomendação: seguir o pedido do usuário (livre por padrão, Shift mantém), **mas guardar a escolha numa opção da
  ferramenta** ("Manter proporção" ligada/desligada, com Shift invertendo), assim cobre PS atual, CSP e o pedido, e dá
  uma linha de preferência. A árvore de trabalho já faz livre por padrão com Shift mantendo.

**Estado na árvore de trabalho [em andamento].** 8 alças (`handle_points`, 4 cantos + 4 lados), `ScaleSel` com alça
oposta como âncora (`scale_drag`), Alt escala do centro, passar da âncora espelha, `Selection.scale: Vec2`, Ctrl+T
(`free_transform`: troca para Mover e seleciona tudo da camada quando não há seleção), Ctrl+A e Ctrl+D. Isso resolve a
maior parte do pedido.

**O que ainda falta para ficar no nível profissional (backlog, ver d):**
1. Alças visíveis também com a camada inteira (`sel.whole`): `free_transform` contorna isso selecionando a caixa do
   conteúdo (`raster_content_box`), mas em camada vetorial `select_all` seleciona traços e a caixa é a deles; testar o
   caso de camada vazia e o de camada com movimento (`layer_placement`), que a S23 deixou pendente ("seleção/mover nessas
   camadas").
2. Pivô arrastável e campos numéricos (X, Y, L, A, ângulo, cisalhamento) na barra do canvas. M.
3. Cisalhar (Ctrl+arrastar alça de lado) e Distorcer (Ctrl+arrastar canto): `Deform::Affine.shear` e um novo modo de
   4 cantos livres via `Deform::Perspective { from, to }` já existem no motor, só falta o gesto e o painel. M.
4. Modo Perspectiva e Malha (grade 3x3) e Liquify como modos da barra de transformação: `Deform::Mesh`, `Deform::Liquify`
   prontos. M a L (a prévia ao vivo `move_live.rs` precisa aceitar um `Deform` genérico, hoje recebe `Affine`:
   `canvas.rs:795`, `move_live.rs`).
5. Raster sem perda: guardar a imagem original e só reamostrar na confirmação (já é um commit só), e **não cortar o que
   sai do canvas** (aceitar camada maior ou avisar). Escala sucessiva borra porque cada Enter reamostra a imagem já
   reamostrada; padrão do PS é igual sem Smart Object, mas o CSP mantém a transformação aberta até confirmar. L
   (depende do modelo de cel raster do tamanho do canvas, `cel_raster(blobs, cel, w, h)`).
6. Seleção deve poder ser somada, subtraída, interseccionada, invertida, suavizada, e **deve limitar pincel, borracha,
   balde e gradiente** (a.2). Isso é a lacuna de maior impacto do núcleo de seleção (ver c).
7. Mover automático: com o Mover, clicar em raster dentro de uma seleção move só a seleção (já), clicar fora e arrastar
   move a camada. Falta "Seleção automática de camada" (Ctrl+clique escolhe a camada sob o cursor, como PS/CSP). M.
8. Largura do traço vetorial na escala não uniforme usa a média geométrica (`width_scale`, `transform.rs`):
   aceitável, mas mostrar na prévia (a árvore de trabalho usa o máximo dos dois eixos, divergente do commit
   `sqrt(|det|)`). Risco: **prévia ≠ resultado**, falha que o usuário já reportou (U7). Unificar. S.

### b3. Painel de camadas

Tudo em `aiai-studio/src/app/panels/layers.rs` (nenhuma mudança local nele). A seleção de camada é
`env.ed.layer: Option<NodeId>` (um valor só), o que impede qualquer ação em lote.

**1. Multisseleção com Ctrl/Shift: ausente.** `layer_row` usa `Sense::click()` e só faz `env.ed.layer = Some(id)`
(`layers.rs:~253-258`). Proposta: trocar por `Editor.layers_sel: Vec<NodeId>` mais `anchor`, mantendo `layer` como a
camada ativa. Clique simples substitui; Ctrl+clique alterna; Shift+clique seleciona o intervalo na ordem visível
(`rows` já vem em ordem de cima para baixo com profundidade, `commands.rs:85`; intervalos que cruzam pastas seguem a
ordem visível, como no PS). Desenhar as selecionadas com `accent_dim` e a ativa com a barra de acento. Tamanho: M.

**2. Arrastar camadas para uma pasta: ausente.** Não há nenhum drag. O motor suporta:
`Command::MoveNode { id, parent, index }` (`aiai-core/src/command.rs:305-325`) com **proteção contra ciclo**
(`p.subtree(id).contains(np)` → erro) e retorno do inverso para o desfazer. Proposta: `egui::DragAndDrop`
(`dnd_drag_source`/`dnd_drop_zone`) em `layer_row`; três zonas por linha (terço de cima = antes, meio de uma pasta =
dentro, terço de baixo = depois), linha-guia de inserção, e um `Command::Batch` com um `MoveNode` por camada
arrastada (um passo de desfazer). O índice em `children` é de baixo para cima (`move_layer`, `commands.rs:418` e o
comentário "a lista do documento é de baixo para cima"): converter a posição visual. Arrastar pasta com filhos move a
subárvore. Auto-rolagem da lista ao arrastar. Tamanho: M.

**3. Apagar várias com a tecla Delete: ausente.** Não há Delete para camadas: o botão Lixeira apaga só a ativa
(`layers.rs:94-108`, `cmds::delete_node`), e Delete só existe para ponto de traço (`keymap.rs:162`, só na ferramenta
Editar pontos, `tools.rs:903`). Ainda falta um escopo de foco: Delete do painel de camadas (quando o painel tem o foco
ou a última interação foi nele) apaga as camadas escolhidas; com foco no canvas e seleção, apaga o conteúdo
selecionado (vetor: `RemoveStrokes`; raster: limpar tiles da seleção). Proposta: ação `LayerDelete`/`SelectionClear`
em `keymap.rs` (Delete e Backspace), `Command::Batch` de `DeleteNode` com label "Apagar N camadas", a próxima camada
ativa fica a vizinha do primeiro apagado, confirmação só se alguma tiver conteúdo e não houver desfazer (há). Tamanho: S.

**4. Botão "Mover para pasta" desabilitado: causa encontrada.** `layers.rs:109`:
`env.icon_button(ui, IconId::LayerGroup, "layers.move_to_group", false, has && !groups.is_empty(), ...)`. O botão fica
**desabilitado enquanto o corte não tem nenhuma camada do tipo Pasta** (`groups` é a lista das pastas existentes,
`layers.rs:108`), e nenhuma é criada por padrão nem existe "Agrupar camadas" (Ctrl+G). A única forma de criar uma
pasta é `+` > Pasta (vazia), item que o usuário não reconheceu como o jeito de começar. Além disso o ícone é o mesmo da
camada Pasta e a dica não explica o motivo de estar desligado. Propostas: (a) botão **Nova pasta com as selecionadas**
(Ctrl+G): cria `LayerType::Group` na posição da camada mais alta escolhida e move as escolhidas para dentro, um
`Batch`; (b) manter "Mover para" como menu sempre habilitado quando há camada escolhida, com "Nova pasta..." no topo,
"Fora de pastas" e a lista; (c) `on_disabled_hover_text` dizendo "crie uma pasta antes". Tamanho: S (com o Batch do
item 2).

**5. Em "Cels da camada", o menu laranja "Chave" desenhado sobre a linha "Cel 1".** Em `cel_list`
(`layers.rs:344-410`) cada cel é um `allocate_exact_size(.., 24.0, Sense::click())` e o papel é um `ComboBox` colocado
**dentro da mesma linha**: `role_rect = [right-108, top+2] a [right-4, bottom-2]` e `ui.new_child(UiBuilder::new().max_rect(role_rect))`
(`layers.rs:394-395`), sem reservar espaço no layout. Logo o combo **sempre cobre a direita da linha**, inclusive o
texto "Cel N" (começa em `left+10`) e o ponto de "exposto" (`left+66`, `layers.rs:~381`) quando o painel é estreito
(abaixo de ~230 px o combo, de 108 px, invade o texto e o ponto); na linha escolhida o fundo é laranja escuro
(`accent_dim`, `theme.rs:95`) com borda laranja, e o combo aberto empurra a lista por cima das linhas seguintes. Não
consegui reproduzir pela captura (o app aberto do usuário está na área Produção; não troquei a área dele), por isso a
causa exata do visual laranja "por cima" é a combinação acima, não confirmada em imagem. Correção: dividir a linha em
colunas fixas (rótulo | ponto | papel) usando `ui.allocate_ui_at_rect` em coordenadas calculadas, **o papel como chip
de texto curto (C, B, I, com cor própria) que abre um menu**, deixar o combo só na linha escolhida, e dar à linha
largura mínima de rótulo (`clip_rect` no texto). O mesmo ComboBox "Chave" em linhas não escolhidas deve virar texto
simples. Tamanho: S.

**6. O que é "Recortar" e por que as linhas sumiram.** "Recortar" liga `clip_to_below` (`layers.rs:296-298`, texto "Recortar
pela camada de baixo"). No compositor (`aiai-render/src/compose.rs:305-430`), cada camada **não recortada** deixa o seu
alfa em `clip_base` (`compose.rs:424`), e a camada recortada tem o alfa multiplicado por ele (`compose.rs:412-421`,
`a *= c[i]`). Ou seja: **só aparece onde a camada de baixo tem pixel**. Quem sobe a opção na camada de linhas vê as
linhas sumirem quando:
- a camada logo abaixo está **vazia ou não tem cel exposto neste quadro** → `clip_base` fica todo zero
  (`compose.rs:405-408`) e a camada recortada some inteira (é o caso mais provável: Raster vazia, camada de
  Preenchimento sem tinta, ou cel vazio no quadro);
- a de baixo é a camada de **Preenchimento**: as linhas só ficam onde há tinta, e a borda externa das linhas, sem tinta
  embaixo, desaparece;
- a camada de baixo está **oculta**: `clip_base = None` (`compose.rs:320`) e **nada** é recortado (o oposto: o efeito
  some e parece que "não faz nada"; no PS uma camada oculta continua valendo como base).
Não há sinal visual na lista de que uma camada está recortada (`layer_row` não desenha seta/recuo, `layers.rs:170-258`),
nem aviso quando a base está vazia, e o `let _ = set_layer_props(...)` do checkbox (`layers.rs:298`) engole o erro.
Também não há "recortar pelo grupo". Correções: (1) seta de recorte e recuo na linha recortada (como PS/CSP); (2) aviso
na barra de status quando `clip_base` fica todo zero ("a camada de baixo não tem pixels no quadro N"); (3) o oculto
passar a valer como base (política: decisão do usuário, hoje é `compose.rs:320`); (4) a dica explicar "só aparece onde
a camada de baixo tem pixels"; (5) Alt+clique entre camadas liga o recorte (atalho do PS) e Ctrl+Alt+G. Tamanho: S.

### b4. Não existe ferramenta de texto nem painel de texto

**Diagnóstico.** O motor sabe desenhar texto, mas é só uma **assadura em lote**: `text_outline` converte glifos
(ab_glyph) em polígonos (`aiai-paint/src/text.rs:93-170`); `text_command` (`aiai-tools/src/fx2d.rs:71`) grava o
preenchimento numa camada raster e o contorno numa vetorial, sem guardar o texto. Só se chega nisso pelo MCP `aiai_text`
(`aiai-mcp/src/tools_production.rs:16,124,197`) ou ação `TEXT` da bridge (`production_tools.rs:88`). Não existe:
- `ToolId::Text` (`workspace.rs:~113`, só 10 ferramentas), nem atalho T, nem painel de caractere/parágrafo;
- camada de texto editável: `LayerType::Text` existe (`model.rs:433`, ícone `LayerText`, `layers.rs:31`) mas não está
  em `ADD_KINDS` (`layers.rs:13-22`) e o compositor trata como camada de cels (`compose.rs:~383`), sem leitura de
  `params` de texto;
- o texto depois de assado não é editável (trocar o texto, a fonte ou o tamanho exige refazer).
Limites do motor atual: **sem quebra de linha** (o `\n` não é tratado, `text.rs:~101` itera `chars` num único
`pen`), **sem kerning** (`sf.kern` não é usado), **sem alinhamento, entrelinha, caixa de parágrafo, quebra
automática** (existe `preprod::wrap_text` só para legendas, `edit.rs:263`), **sem shaping** (ligaduras, árabe,
indiano, emoji colorido), **fonte só da pasta de ativos** (`aiai-studio/assets/fonts`: NotoSans e NotoSansJP regular e
black; o MCP aceita caminho sob `C:\AnimeIAI`), sem lista de fontes do sistema, sem estilo (negrito/itálico real),
sem contorno/sombra por glifo, sem texto em curva, e o centro do bloco é o centro da caixa dos glifos (não a linha
de base).

**Comparação.** PS e CSP: ferramenta Texto (T) com caixa de ponto/parágrafo, painel Caractere e Parágrafo (fonte,
estilo, tamanho, entrelinha, rastreamento, kerning, alinhamento, sublinhado, cor, antialias), camada de texto
editável que rasteriza sob demanda, texto vertical japonês (tategaki) com rubi e tate-chu-yoko (CSP, importante para
mangá/anime), balões e fonte de onomatopeia. Procreate: ferramenta Adicionar > Texto, painel Estilo/Design/Atributos.
Krita: Texto SVG com painel Rich Text.

**Proposta.**
1. Camada de texto de verdade: `LayerData.params.text = { content, font, size, color, align, line_height, tracking,
   vertical, box: {x, y, w?, h?}, rotation, outline?, shadow? }` (esquema por tipo já previsto no comentário de
   `params`, `model.rs:~522`) e o render **vetoriza na hora** com `text_outline` (já devolve contornos), cacheando por
   hash dos parâmetros. Sem novo tipo de dado raster. Tamanho: L.
2. Motor de layout: quebra de linha e de parágrafo, alinhamento, entrelinha, kerning (`ab_glyph` expõe `kern`),
   `unicode-linebreak` para quebra, e tategaki completo com rotação de glifos de pontuação, rubi. Considerar
   `cosmic-text` (shaping via `rustybuzz`, fallback de fonte, descobre fontes do sistema) em substituição ao
   `ab_glyph` puro. Tamanho: L.
3. Ferramenta Texto (T): clique cria caixa de ponto, arrasto cria caixa de parágrafo; edição direta com `TextEdit`
   do egui sobre o canvas (cursor, seleção, IME de japonês via `ime` do egui), Esc/clique fora confirma. Um passo de
   desfazer por edição concluída. Tamanho: L.
4. Painel Texto (aba, como o painel Ferramenta existente): fonte (lista do sistema e do projeto), estilo, tamanho,
   entrelinha, rastreamento, alinhamento, vertical, cor (paleta do projeto), contorno e sombra (reaproveitar
   `Effect::Outline` e `Effect::DropShadow`, `aiai-fx/src/effects.rs:56-58`), rugosidade (`roughen`, já existe em
   `text.rs:30`, diferencial de onomatopeia), "converter em curvas" (assar para vetor, como `text_command` faz hoje).
   Tamanho: M.
5. MCP: `aiai_text` passa a criar/editar a camada de texto (`id`, `content`, atributos), mantendo a opção de assar.
   Tamanho: S.
6. Fontes: incluir mais uma fonte livre de mangá (licença OFL, como `OFL-NotoSansJP.txt`), e avisar licença de fonte
   do usuário. Tamanho: S.
Sem isto, balões, legendas e onomatopeias do curta continuam só pela IA e não editáveis pelo usuário.

---

## (c) Outras lacunas profissionais, por impacto

Impacto = quanto atrapalha produzir uma cena de anime inteira no app (A = bloqueia, B = grande atrito, C = comparativo).

| # | Lacuna | Impacto | Por quê / referência |
|---|---|---|---|
| 1 | Seleção não restringe pintura, balde, borracha e gradiente (`grep selection` em `aiai-tools/src/paint.rs`, `fill.rs`, `erase.rs`: 0) | A | Base de toda pintura de sombras e luzes. PS/CSP/Krita/Procreate fazem "pintar só dentro" |
| 2 | **Sem onion skin / mesa de luz** | A (anime) | Animar quadro a quadro sem ver o anterior e o seguinte é impraticável; CSP, Krita e Procreate têm onion skin; CSP tem light table. Já listado como P2 na S23 |
| 3 | Camada: multisseleção, arrastar, agrupar, duplicar, mesclar (b3), sem Ctrl+J/G/E | A | Organização de cena com 24+ camadas (caso da auditoria) |
| 4 | Sem clipboard (copiar, recortar, colar, colar como nova camada) e sem Edit > Limpar/Preencher seleção | A | Ctrl+C/X/V básico; copiar traços entre quadros |
| 5 | Máscara de camada, travar pixels transparentes, "recorte" visível | B | Pintura de sombra recortada pela cor-base é o fluxo padrão de CSP/anime; o motor tem `mask` (`model.rs:~515`) e falta tudo na UI |
| 6 | Sem painel Histórico (lista de passos) | B | PS/Krita/CSP; o núcleo guarda 275 passos desfazer (`aiai_status`) e a lista de rótulos já existe (`Command::label`, `command.rs:140-160`) |
| 7 | Réguas: perspectiva com snap, simetria, paralela, concentração; assistente de forma | B | CSP Rulers, Procreate Drawing Guide + Symmetry, Krita Assistants. O motor gera linhas de fuga (`guide.rs:109`) sem snap; dependem de `aiai-guide3d` |
| 8 | Seleção: soma/subtração, inverter, expandir, suavizar, faixa de cor, por camada, launcher | B | Motor pronto em `select.rs:32-191`, falta UI |
| 9 | Transformação: cisalhar, distorcer, perspectiva, malha, liquify, pivô, numérico, sem perda (b2) | B | Motor pronto em `transform.rs:13-35`, falta UI |
| 10 | Ferramenta Gradiente e Formas preenchidas na UI (sem aiai_gradient/aiai_fill_shape) | B | |
| 11 | Camadas de ajuste (níveis, curvas, matiz, mapa de gradiente) criáveis e editáveis na UI | B | Motor completo em `adjust.rs:11-33` (incluindo Gradient Map); só há criação por MCP; PS Adjustment Layers, CSP Correction layers |
| 12 | Texto (b4) | B | |
| 13 | Dinâmica do pincel por inclinação, rotação, velocidade; curva de pressão por pincel; simetria do traço (espelho, 2 a 12 eixos) | C | Procreate/Krita/CSP; já listados como pendentes ("WinTab, inclinação") |
| 14 | Importar imagem como camada raster editável e colar do clipboard | B | `doc.rs:399-410` só cria `Reference` a 60% |
| 15 | Gerenciamento de cor: ICC, 16 bits/linear, perfil de exportação | C | Cor linear/HDR está em P2 (`S23-AUDITORIA-BACKLOG.md`); o pipeline é float (`Image.data: [f32;4]`), então a base existe |
| 16 | Biblioteca de materiais (texturas, tons, fundos, 3D), tons de trama (screentone), moldura de quadro | C | CSP Materials; `aiai_pattern_fill` existe pelo MCP |
| 17 | Camadas 3D/poses de referência na tela | C | `aiai-guide3d` existe só como guia por MCP |
| 18 | Mais modos de mistura (Color Dodge/Burn, Linear Light, Vivid Light, Divide...) | C | `BLEND_MODES` (`commands.rs:181`) tem 17 |
| 19 | Composições de camadas, estilos de camada (sombra, brilho, contorno) | C | |
| 20 | Estabilização por velocidade, espelhamento ao vivo, navegação por gestos de toque, atalhos configuráveis para tudo (hoje configuráveis, mas com poucas ações) | C | `keymap.rs:138-166` |

Diferenciais que o AnimeIAI tem e os outros não têm: paleta indexada ligada ao traço, papéis de cel de produção,
balde com lint de regiões, editor de pontos com id estável para a IA, bridge/MCP sobre o documento, WAL com
recuperação, pipeline de produção do roteiro à entrega, exportação XDTS e PSD em camadas. Convém não perder isso ao
copiar fluxos do CSP: um recurso novo deve passar pelo mesmo caminho de comando (`Command`) para valer no desfazer e
no MCP.

---

## (d) Backlog priorizado (tamanhos: S até 1 dia, M 2 a 4 dias, L mais de uma semana)

A ordem leva em conta dependências: a camada de comandos e a multisseleção entram antes das ações em lote.

| # | Item | Onde | Tam. | Depende de |
|---|---|---|---|---|
| 1 | Fechar b1/b2 pendentes na árvore de trabalho: teste de mouse real da caixa e das 8 alças (captura antes/durante/depois), prévia = resultado da largura do traço, camada com movimento, criar testes unitários de `scale_drag` com Shift/Alt/espelho | `canvas.rs` (`scale_drag`, `on_release` `BoxPick`), `tools.rs` (`box_pick`, `free_transform`), `editor.rs` (`Selection.scale: Vec2`) | S | sessão em andamento |
| 2 | **Seleção que limita pintura**: máscara `Selection` de motor (`aiai_paint::select::Selection`) guardada no `Editor` (cov por pixel) e passada a `commit_brush`, `commit_erase_*`, `do_fill`, a prévia ao vivo (`live.rs`) e `gradient_command`; clip no tile | `editor.rs:303`, `tools.rs:461,483,501,566`, `live.rs`, `aiai-tools/src/paint.rs`, `fill.rs`, `erase.rs` | L | |
| 3 | Seleção: Shift soma, Alt subtrai, Shift+Alt interseção; Inverter (Ctrl+Shift+I), Expandir/Contrair, Suavizar; barra com os botões; faixa de cor e "por camada" (Ctrl+clique na camada) | `tools.rs:637` `finish_select`, UI no painel Ferramenta (`panels/properties.rs`), motor `select.rs:32-191` pronto | M | 2 |
| 4 | Multisseleção de camadas (`Editor.layers_sel`), Ctrl/Shift+clique, destaque; ações em lote (visibilidade, trava, opacidade, apagar, mover) | `panels/layers.rs:170-258`, `editor.rs` (`layer`), `commands.rs` | M | |
| 5 | Delete apaga camadas escolhidas (foco no painel) e conteúdo da seleção (foco no canvas); ação no `keymap` | `keymap.rs:104-166`, `tools.rs:892`, `layers.rs:94` | S | 4 |
| 6 | Arrastar camadas (antes, dentro, depois), com `Batch` de `MoveNode`; Agrupar (Ctrl+G), Desagrupar, "Mover para" sempre habilitado com "Nova pasta..." | `layers.rs:104-135`, `commands.rs:418-436`, motor `command.rs:305` | M | 4 |
| 7 | Duplicar camada (Ctrl+J) e cel, Mesclar abaixo (Ctrl+E), Achatar pasta, com novos comandos de alto nível (`Batch` de `CreateNode` + `SetTiles`/`InsertStrokes`) | `commands.rs`, `layers.rs` | M | 4 |
| 8 | Recortar: seta e recuo na linha, aviso de base vazia, dica correta, Alt+clique, política para base oculta; `layers.rs:298` mostrar o erro | `layers.rs:170-258,296`, `compose.rs:320,405-424` | S | |
| 9 | Cels: linha com colunas fixas e papel como chip; combo só na linha escolhida | `layers.rs:344-410` | S | |
| 10 | Barra de transformação (numérico X/Y/L/A/ângulo/cisalhamento, pivô, "Manter proporção" invertido por Shift, Aplicar/Cancelar visíveis); Ctrl+arrastar lado = cisalhar | `canvas.rs` (novo painel de opções do Mover), `editor.rs` (`ToolOptions`), `tools.rs:727` | M | 1 |
| 11 | Modos Distorcer, Perspectiva, Malha e Liquify na transformação (gestos, `move_live` genérico com `Deform`) | `move_live.rs`, `canvas.rs:795`, `tools.rs:727`, motor `transform.rs:27-33`, `warp.rs` | L | 10 |
| 12 | Raster sem perda e sem corte: transformar mantendo a imagem original até confirmar, avisar/permitir sair do canvas | `aiai-tools/src/transform.rs:~185-205`, `paint.rs` (`cel_raster`) | L | 11 |
| 13 | Clipboard: Ctrl+C/X/V (vetor: traços serializados; raster: PNG com alfa; imagens do sistema); "Colar como nova camada"; Editar > Limpar, Preencher com a cor | `app.rs:372`, `keymap.rs`, `tools.rs`, novo `app/clipboard.rs` | M | 2 |
| 14 | Onion skin e mesa de luz: render de quadros vizinhos tingidos (vermelho/verde), contagem antes/depois, opacidade, só chaves ou todos; atalho; no timeline | `canvas.rs` (render), `aiai-render/src/compose.rs`, `panels/timeline.rs`, `ToolOptions` | L | |
| 15 | Painel Histórico (lista de passos com rótulo do `Command`, clique desfaz até ali) | novo `panels/history.rs`, `layout.rs` (novo `PanelKind`), `aiai-core` undo | M | |
| 16 | Máscara de camada na UI (criar a partir da seleção, pintar, Ctrl+clique) e **travar pixels transparentes** (campo `lock_alpha` em `LayerData`, respeitado por `commit_brush` e transformação) | `layers.rs`, `aiai-core/src/model.rs:~512`, `tools.rs:461`, `compose.rs` | M | 2 |
| 17 | Texto (b4): camada com `params.text`, layout (quebra, kerning, tate), ferramenta T, painel, MCP `aiai_text` editando | `aiai-paint/src/text.rs`, `aiai-tools/src/fx2d.rs:71`, `workspace.rs` (`ToolId::Text`), `compose.rs`, `panels/` novo `text.rs`, `aiai-mcp/src/tools_production.rs:16` | L | |
| 18 | Réguas com snap: perspectiva (1, 2, 3 pontos), simetria (eixo, radial), paralelas; a régua é um objeto de camada Guia e o snap age em caneta, pincel, formas e transformação | `aiai-tools/src/guide.rs`, `canvas.rs`, `tools.rs` (`push` do traço), `panels/properties.rs` | L | |
| 19 | Camadas de ajuste na UI (Níveis, Curvas, Matiz, Mapa de gradiente, Limiar) com prévia ao vivo | `layers.rs:13`, novo painel, motor `aiai-paint/src/adjust.rs` | M | |
| 20 | Ferramenta Gradiente (linear, radial, ângulo, refletido, mapa de gradiente) e Formas preenchidas editáveis (retângulo, elipse, polígono) | `workspace.rs`, `tools.rs`, `aiai-tools/src/paint.rs:325`, `fill.rs` | M | 2 |
| 21 | Dinâmica do pincel (inclinação, rotação, velocidade) e simetria do traço | `aiai-paint/src/brush.rs:257`, `tools.rs:71`, painel Ferramenta | M | mesa física para inclinação |
| 22 | Importar imagem como raster editável e arrastar do disco; colar da área de transferência | `doc.rs:386-410` | S | 13 |
| 23 | Mais modos de mistura e estilos de camada simples (sombra, contorno) | `model.rs:457`, `blend.rs:100`, `commands.rs:181` | S a M | |
| 24 | Gerenciamento de cor (perfil ICC no export, 16 bits no PSD, sRGB x P3) | `aiai-tools/src/export.rs:180` | L | |
| 25 | Biblioteca de materiais (texturas, tons, fundos), tons de trama | novo painel; reaproveitar `aiai_pattern_fill` | M | |

Ordem de ataque sugerida (maior retorno por esforço): 1, 8, 9, 5 (rápidos, destravam o cotidiano), 4, 6, 7 (camadas),
2 e 3 (seleção que limita pintura), 13, 14, 15, 10, 16, 17 (texto), 19, 18, 11, 12.

Antes de implementar qualquer item: conferir o diff local de `canvas.rs`, `editor.rs`, `tools.rs` da sessão paralela
para não duplicar nem conflitar (itens 1, 10 e 11 tocam as mesmas funções).

---

## Fontes consultadas na web

- Photoshop CC 2019, escala proporcional por padrão e Shift para livre, "Use Legacy Free Transform":
  discussões da comunidade Adobe
  ([thread 1](https://community.adobe.com/t5/photoshop-ecosystem-discussions/can-t-seem-to-scale-non-proportionally-anymore/m-p/10108661),
  [thread 2](https://community.adobe.com/t5/photoshop-ecosystem-discussions/scaling-images-with-shift-no-longer-works-after-update-20-0/m-p/10077023),
  [thread 3](https://community.adobe.com/questions-712/why-did-ps-decide-to-have-shift-key-and-scale-be-non-proportional-1555173)).
  Não achei nota de versão oficial da Adobe sobre a mudança; é a leitura das respostas dos usuários e da Adobe nas
  discussões.
- Clip Studio Paint, tipos de transformação, Keep aspect ratio, Free Transform (Ctrl+T), Mesh Transformation (até 20 pontos):
  [Types of transformations](https://help.clip-studio.com/en-us/manual_en/360_transform/Types_of_transformations.htm),
  [Adjust your illustrations with transformation tools](https://tips.clip-studio.com/en-us/articles/11035),
  [How can I free transform an image?](https://support.clip-studio.com/en-us/faq/articles/20200062).
  Teclas exatas de Distorcer em Free Transform não confirmadas na documentação oficial.
- Os itens marcados "(conhecimento)" (Selection Launcher, Procreate modos de transformação, Krita Cage/Warp, CSP Rulers,
  3D, Materials) não foram reverificados nesta sessão.
