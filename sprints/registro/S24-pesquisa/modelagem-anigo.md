# Modelagem: o que o ANIGO tem, o que o AnimeIAI tem e o plano do porte

Origem: `C:\ANIGO`, commit `bc5e83af1267eec28cc13c1777204b6ac1a6940f` (o mesmo do vendor de 2026-10-20). Código do mesmo
autor; copiar é permitido.

## Por que o viewport atual é lento

`aiai-studio/src/app/modeling.rs` (escrito do zero no df4da70) pede cada quadro ao serviço 3D (`model3d::preview_request`):
a GPU renderiza num alvo fora da tela, **lê de volta para a CPU** (`render_scene` → `RgbaImage`), o app converte em
`ColorImage` e **sobe de novo** como textura do egui. Um pedido por vez; cada mudança de câmera paga GPU→CPU→GPU de um
quadro de até 2 MP, mais o `to_model()` (GLB normalizado) do documento inteiro a cada edição. As sobreposições (grade,
arestas, vértices) saem do `Painter` do egui com `Topology::build` refeito a cada quadro. Daí o zoom "travado" e nada de
órbita fluida. Não havia órbita sem botão do meio além de Alt+esquerdo.

## Comparação

| Área | ANIGO (`anigo-studio` + `anigo-modeling`) | AnimeIAI antes (df4da70) |
|---|---|---|
| Render do viewport | Workbench próprio na GPU (grade infinita, sólido, linhas e pontos de edição em instâncias, MSAA 4×) num alvo registrado **uma vez** no egui (`register_native_texture`), redesenhado direto na GPU; prévia Cel pelo renderer canônico com `render_to_texture` (sem leitura de volta) por baixo das sobreposições | quadro do serviço 3D lido de volta e re-subido como textura; sobreposições no `Painter` |
| Câmera | `viewport::camera::OrbitCamera` turntable: órbita, pan, zoom multiplicativo, **zoom para o cursor**, enquadrar, vistas frente/trás/lados/topo/baixo, ortográfica, raio/projeção testados | `aiai_3d_model::camera::OrbitCamera` sem ortho nem zoom no cursor |
| Gizmo de eixos | `nav_gizmo` (bola de eixos clicável, arrastar orbita, botões pan/zoom/projeção) | nenhum |
| Gizmo de transformação | `app/gizmo.rs`: setas/anéis/quadrados com teste de clique por eixo | nenhum (arrasto livre da ferramenta) |
| Seleção | `viewport::picking`: vértice/aresta/face com oclusão, caixa (box), clique com Shift; x-ray | clique só; sem caixa |
| Modal G/R/S | `modeling::modal`: eixo, plano (Shift+eixo), número digitado, encaixe, direção (extrude), **edição proporcional** (`ops::transform::proportional_weights`) | eixo, número, encaixe; sem proporcional |
| Operadores | 35 (`OpKind`): extrudar região/individual, inset região/individual, chanfro com segmentos e perfil, loop cut com cortes e deslizar, bisect, excluir (4), unir centro/distância, dissolver (4) + limitada, preencher buraco/grade, ponte, triangular, tris→quads, inverter/recalcular normais, suavizar, relaxar, subdividir, poke, sharp/seam/vinco, ocultar/revelar; painel "Ajustar última operação" que refaz com parâmetros novos | extrude, inset, loop cut (1 corte), bevel (1 segmento), merge, apagar, dissolver |
| Modificadores | pilha `stack.rs`: subsurf, mirror, solidify, array, weld, edge split, displace, decimate, triangulate; UI com ligar/subir/descer/tirar | mirror, subsurf, array |
| Histórico | `History<T>` genérico por snapshot do documento com rótulos, limite e `pop_undo` (usado pelo "Ajustar última operação"); `SnapshotHistory`/`PositionDelta` no motor para escultura | documento inteiro por passo, sem rótulo |
| Escultura | `sculpt/*` (pincéis, simetria, automáscara, dyntopo, voxel remesh) + espelho incremental na GPU (`sculpt_mesh.rs`) + `sculpt_ui.rs` | nenhuma |
| UV | `uv/*` (costuras, unwrap, pack, projeções, métricas) + editor 2D `uv_ui*` (~2,6 mil linhas) | nenhum |
| Multivista | `multiview.rs` (1/2/4 vistas, divisórias arrastáveis, câmera por vista) | nenhuma |
| Atalhos | keymap por contexto com modos (`modelagem.*`, G/R/S do Blender), editor de atalhos | teclas do Blender fixas no código, sem contexto no mapa do app (que usa V mover, S seleção, R girar a vista) |

O histórico do ANIGO é a mesma estratégia do AnimeIAI (snapshot do documento inteiro), mas com rótulos, limite e
`pop_undo`; vale trocar pelo dele (é o que o painel "Ajustar última operação" precisa).

## Plano do porte (esta tarefa)

1. **Motor (`aiai-3d-edit`)**: vendorizar o resto que a UI usa: `convert` (triangulação, `quadify`, `to_core_mesh` para o
   renderer cel: `aiai_3d_core::Mesh` é o mesmo formato do `anigo_core`), operadores `bisect`, `bridge`, `fill`, `flip`,
   `spin`, `triangulate`, `misc`, `smooth` inteiro; modificadores `solidify`, `weld`, `edge_split`, `decimate`,
   `displace`; `history` do motor e o `History<T>` do estúdio. O catálogo de operadores do ANIGO (`OpKind`, `OpParams`,
   `apply_kind`, puro) vem para o motor (a UI e a MCP usam o mesmo). `ModifierSpec` (formato salvo, compatível) ganha
   solidify, weld, edge_split, decimate, displace e triangulate. Fica de fora: `attributes`, `normals_tools`, `bvh`,
   `curve`, `hair`, `shapekeys`, `stack.rs` (a pilha salva é o `ModifierSpec`), `uv`, `sculpt`.
2. **Viewport nativo**: `app/modeling/` com o Workbench do ANIGO (WGSL + `mesh_data`), a câmera, o `nav_gizmo`, o picking
   e o gizmo de transformação. A prévia Cel (contorno ligado por padrão, o visual do AnimeIAI) é um `HeadlessRenderer`
   próprio no device do egui renderizando com `render_to_texture` numa textura registrada uma vez; o Workbench desenha
   grade e sobreposições por cima (modo `OverlayOnly`) ou sólido/arame. Malha só sobe para a GPU quando o objeto muda;
   câmera nova = só uniformes. Nada de leitura de volta. Navegação: botão do meio (Shift pan, Ctrl zoom), Alt+esquerdo,
   **botão direito orbita** (Shift+direito pan, Ctrl+direito zoom) para quem não tem botão do meio, roda = zoom no cursor
   (suave), gizmo de eixos e botões pan/zoom/projeção, vistas por Alt+1/3/7 (o egui não distingue o teclado numérico),
   Alt+5 perspectiva/ortográfica, enquadrar seleção.
3. **Edição**: modal com eixo/plano/número/encaixe e edição proporcional (O), caixa e laço de seleção, deslizar aresta
   (escrito aqui; o ANIGO não tem), chanfro com segmentos e perfil, loop cut com cortes; painel Operadores e
   "Ajustar última operação"; painel Modificadores com os 9 tipos.
4. **Atalhos**: contexto "Modelagem" no mapa de atalhos com dois perfis em Preferências > Atalhos: **AnimeIAI** (padrão:
   V mover, R girar, T escalar, S seleção, E extrudar, I inset, Ctrl+B chanfro, Ctrl+R loop cut…) e **Blender/ANIGO**
   (G/R/S). Conflitos conferidos dentro do contexto; as dicas da prateleira e dos botões mostram a tecla do perfil.
5. Manter salvar na biblioteca, "Enviar ao Layout", MCP `aiai_modeling`, poses em camadas Model3d.

## Fica para depois (planejado)

- **Escultura**: motor `sculpt/*` + `bvh` + `PositionDelta` (≈9 mil linhas) e o espelho incremental na GPU
  (`workbench/sculpt_mesh.rs`) com `sculpt_ui.rs`. Entra como um modo novo do mesmo viewport sem rearquitetar: o
  Workbench já tem `update_solid` por faixa.
- **UV**: o AnimeIAI não usa textura nos modelos (material toon por cor), então o editor 2D não paga o custo agora. As
  costuras já entram pelos operadores (marcar/limpar costura).
- **Multivista** (1/2/4 vistas): o Workbench já tem alvos por vista (`MAX_VIEW_SLOTS`); falta a UI de divisórias.
