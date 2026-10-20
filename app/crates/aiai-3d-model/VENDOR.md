# VENDOR: aiai-3d-model

Commit do ANIGO: `bc5e83af1267eec28cc13c1777204b6ac1a6940f` (cópia de 2026-10-20). Código do mesmo autor (regra 4).

## Copiados

| Arquivo aqui | Origem no ANIGO | Mudança |
|---|---|---|
| `src/gltf.rs` | `crates/anigo-vrm/src/gltf.rs` | nenhuma (parser glTF 2.0/GLB, accessors interleaved/quantizados/esparsos, `build_glb`) |
| `src/gizmo.rs` | `crates/anigo-studio/src/scene/gizmo.rs` | só a parte pura (geometria, hit-test, `GizmoDrag`) e os testes dela; `NodeXf`/`apply_*` (nós do ANIGO) ficaram de fora; `CamView` vem de `camera.rs` |
| `src/camera.rs` (`OrbitCamera`, `project_with`, `CamView`) | `crates/anigo-studio/src/viewport/camera.rs` e `scene/camview.rs` | subconjunto (sem presets, zoom ao cursor, voo); `view_up()` exposto para o renderer |

## Escritos no AnimeIAI

`import.rs` (glTF de vários nós e malhas, OBJ + MTL, limites, VRM recusado "em breve"), `model.rs`, `glb.rs` (GLB
normalizado determinístico), `gizmo_apply.rs` (arrasto → `Transform3d` com pivô fixo), `raster.rs` (rasterizador CPU de
reserva: toon/flat/wire/silhouette + contorno), `scene.rs` (cena do renderer cel, com compensação da exposição e do ACES
do shader), ponte câmera da camada ⇄ `OrbitCamera` em `camera.rs`; `pose.rs` (pose por nome de osso, skinning na CPU, anel e IK de
dois ossos no estilo do `solve_limb` do `anigo-ik`, poses prontas) e `mannequin.rs` (o corpo do `aiai-3d-charbase` com
esqueleto e pesos). A pele do glTF é lida em `import.rs` e escrita em `glb.rs`.

Sem dependências novas: o crate `gltf` NÃO foi adicionado (o parser do ANIGO já cobre o necessário).
