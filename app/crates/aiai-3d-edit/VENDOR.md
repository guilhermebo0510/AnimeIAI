# VENDOR: aiai-3d-edit

Commit do ANIGO: `bc5e83af1267eec28cc13c1777204b6ac1a6940f` (cópia de 2026-10-20; o segundo lote, S24 "Modelagem como no
ANIGO", é do mesmo commit). Código do mesmo autor (regra 4). Em todos: `anigo_core` → `aiai_3d_core`,
`anigo_modeling` → `aiai_3d_edit`.

## Copiados de `crates/anigo-modeling/src`

| Arquivo aqui | Mudança |
|---|---|
| `error.rs`, `mesh.rs`, `topology.rs`, `select.rs`, `primitives.rs`, `normals.rs`, `subdiv.rs`, `convert.rs` | nenhuma |
| `ops/bevel.rs`, `bisect.rs`, `bridge.rs`, `delete.rs`, `dissolve.rs`, `extrude.rs`, `fill.rs`, `flip.rs`, `inset.rs`, `loops.rs`, `merge.rs`, `misc.rs`, `smooth.rs`, `spin.rs`, `transform.rs`, `triangulate.rs` | nenhuma (o `bevel.rs` e o `smooth.rs` agora são os inteiros; no primeiro lote eram parciais) |
| `modifiers/mirror.rs`, `array.rs`, `solidify.rs`, `weld.rs`, `edge_split.rs`, `decimate.rs`, `displace.rs` | nenhuma |
| `bvh.rs`, `sculpt/automask.rs`, `brush.rs`, `dyntopo.rs`, `falloff.rs`, `stroke.rs`, `symmetry.rs`, `voxel_remesh.rs`, `sculpt/mod.rs` | nenhuma (28/10, escultura) |
| `history.rs` (só `PositionDelta`) | acrescentado ao nosso `history.rs` (que é o `History<T>` do estúdio) |
| `ops/mod.rs`, `modifiers/mod.rs` | só a lista de módulos (mais `catalog` e `slide`, do AnimeIAI) |

## Copiados de `crates/anigo-studio/src`

| Arquivo aqui | Origem | Mudança |
|---|---|---|
| `ops/catalog.rs` | `app/operators.rs`: `OpKind`, `OpParams` (com `apply_json`), `apply_kind`, `components` | só a parte pura (sem `StudioApp`, sem a UI); os testes passaram de `ModelingSession` para `EditMesh` + `Selection` |
| `history.rs` | `modeling/history.rs` (`History<T>` com rótulos, limite, `pop_undo`) | `push_owned`, `len`, `Default` (100 passos) |

## Escrito no AnimeIAI

- `doc.rs`: o documento (objetos, transformação, cor toon, sombreamento, pilha `ModifierSpec`), operações que a UI e a
  MCP usam, conversão para `aiai_3d_model::Model` (GLB normalizado). A pilha salva (`ModifierSpec`, formato do
  documento) ganhou `solidify`, `weld`, `edge_split`, `decimate`, `displace` e `triangulate` (campos com padrão: os
  documentos antigos continuam lendo) sobre os módulos vendorizados; o `stack.rs` do ANIGO não veio. O histórico é o
  `History<T>` do ANIGO (`doc::History = History<ModelDoc>`), que substituiu o `doc::History` próprio (mesma estratégia,
  documento inteiro por passo, agora com rótulo e `pop_undo`). `triangulate_polygon` agora é o do `convert.rs`.
- `ops/slide.rs`: deslizar aresta/vértice pelos trilhos (o ANIGO não tem).

## Não trazidos (o ANIGO tem; entram depois sem rearquitetar)

`attributes`, `normals_tools` (pintura de canais NPR e ferramentas de normais), `bvh`, `history` do motor
(`SnapshotHistory`/`PositionDelta`, só a escultura usa), `sculpt/*`, `uv/*`, `curve`, `hair`, `shapekeys` e
`modifiers/stack.rs`. Plano em `sprints/registro/S24-pesquisa/modelagem-anigo.md`.
