# VENDOR: viewport da Modelagem (`aiai-studio/src/app/modeling`)

Commit do ANIGO: `bc5e83af1267eec28cc13c1777204b6ac1a6940f` (2026-10-20), `crates/anigo-studio/src`. Código do mesmo autor
(regra 4).

| Arquivo aqui | Origem | Mudança |
|---|---|---|
| `camera.rs` | `viewport/camera.rs` (`OrbitCamera`, `ViewPreset`, zoom para o cursor, ortográfica) | nenhuma |
| `picking.rs` | `viewport/picking.rs` | `box_select` virou `region_select` comum; `lasso_select` e `point_in_polygon` escritos aqui |
| `nav_gizmo.rs` | `viewport/nav_gizmo.rs` | ícones SVG do AnimeIAI (mão, lupa, cubo/quadro), cores do `Tokens`, retângulos registrados para a bridge (`mdl:nav.*`), `preset_key` |
| `gizmo.rs` | `app/gizmo.rs` (tamanho, anéis, teste de clique das alças) | nenhuma |
| `modal.rs` | `modeling/modal.rs` (`ModalTransform`: eixo, plano, direção, número, encaixe, pesos proporcionais) | nenhuma |
| `workbench/mod.rs`, `workbench/workbench.wgsl` | `workbench/mod.rs`, `workbench/workbench.wgsl` | sem `sculpt_mesh` e sem a recarga de shader por arquivo; `release_targets` para o orçamento de VRAM |
| `workbench/mesh_data.rs` | `workbench/mesh_data.rs` | sem o overlay de normais e a vista de canais NPR (dependem de `attributes`, não trazido); cor de seleção local |
| `multiview.rs` | `app/multiview.rs` (1/2/4 vistas, câmera por vista, divisórias) | cores do `Tokens`, sem salvar em disco; a grade da vista ortográfica alinhada (XY/ZY) foi escrita aqui no `workbench.wgsl` |
| `sculpt.rs` | `modeling/sculpt.rs` (pincéis, traço, hover, máscara, alisar tudo, voxel remesh, simetrizar) | a `ModelingSession` com checkpoint virou o `ModelingState` (o traço guarda o documento de antes e só entra no desfazer se mudou algo); sem o espelho incremental na GPU (`sculpt_mesh.rs`): o objeto sobe de novo a cada quadro do traço; a máscara escurece o sólido no `mesh_data.rs` |

Escritos aqui seguindo `app/viewport_ui.rs` e `celview.rs` do ANIGO: `gpu.rs` (Workbench + prévia cel com
`render_to_texture` em texturas nativas do egui, chaves por objeto, nada lido de volta), `mod.rs` (entrada, modais de
extrude/inset/bevel/loop cut/deslizar, gizmo, caixa/laço) e `keys.rs` (contexto de atalhos com os perfis AnimeIAI e
Blender/ANIGO).

Não trazidos: `sculpt_ui.rs` + `modeling/sculpt.rs` + `workbench/sculpt_mesh.rs`,
`uv_ui*`, `normals_ui.rs`, `hair_ui.rs`. Plano em `sprints/registro/S24-pesquisa/modelagem-anigo.md`.
