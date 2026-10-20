# VENDOR: aiai-3d-core

Origem: `C:\ANIGO\crates\anigo-core\src` (ANIGO, código do mesmo autor; regra 4 de `sprints/00-REGRAS-INVIOLAVEIS.md`).
Commit do ANIGO: `bc5e83af1267eec28cc13c1777204b6ac1a6940f` (cópia de 2026-10-20).

## Copiados sem mudança de conteúdo

- `math.rs`, `npr.rs`, `shadow.rs`, `morph.rs`

## Copiados com mudança

- `mesh.rs`: removidas `create_canonical_base_unskinned` e `create_canonical_base` (gerador da base humanoide de 4.070
  vértices e atribuição de skin; dependem de `skinning`/`bone_sync`, que não vieram). O restante (Vertex de 72 B, cubo,
  esfera, manequim proxy, `to_glb_bytes`/`from_glb_bytes`) é igual.
  Testes removidos de `mesh.rs` (dependem da base canônica ou dos `.glb` de `assets/models` do ANIGO):
  `test_canonical_base_meshes_generation_and_isomorphism`, `test_canonical_base_meshes_outward_normals`,
  `test_disk_assets_canonical_models_isomorphic`; `test_glb_serialization_roundtrip` usa o manequim proxy.
- `scene.rs`: `Scene::default()` usa `Mesh::create_mannequin_proxy()` no lugar da base canônica.
- `hierarchy.rs`: só `NodeKind`, `HierarchyError`, `resolve_indexed` e `local_matrix`; as funções sobre `NodeSlot`
  (`ProjectState` do ANIGO) e seus testes ficaram de fora.

## Escritos aqui (subconjuntos com os mesmos caminhos de módulo, para facilitar a fusão futura)

- `snapshot.rs`: só `SkinPayload` (paleta identidade de 24 ossos) e `identity_palette_floats`.
- `level.rs`: só `LightShape`.
- `lib.rs`: reexporta o subconjunto.

## Não vieram

`command`, `project`, `snapshot` completo, `level` completo, `skinning`, `bone_sync`, `pose`, `deformation`,
`morph_catalog`, `retarget`, `animation`, `export`, `ids`, `tactile`, `somatotype`, `bvh`. Pose e skinning entram
nas fases 8 e 9 do backlog (`S24-pesquisa/3d.md`).

Edição Rust: 2021 (como no ANIGO).
