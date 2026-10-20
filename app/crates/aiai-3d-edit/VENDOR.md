# VENDOR: aiai-3d-edit

Commit do ANIGO: `bc5e83af1267eec28cc13c1777204b6ac1a6940f` (cópia de 2026-10-20). Código do mesmo autor (regra 4).

## Copiados de `crates/anigo-modeling/src`

| Arquivo aqui | Mudança |
|---|---|
| `error.rs`, `mesh.rs`, `topology.rs`, `select.rs`, `primitives.rs`, `normals.rs`, `subdiv.rs` | nenhuma |
| `ops/extrude.rs`, `inset.rs`, `loops.rs`, `merge.rs`, `dissolve.rs`, `delete.rs`, `transform.rs` | nenhuma |
| `ops/bevel.rs` | nos testes, saiu o poliedro de referência cortado com o `bisect` (não trazido); o resto dos testes fica |
| `ops/smooth.rs` | só o gerador `XorShift64` (usado pela edição proporcional do `transform.rs`) |
| `ops/mod.rs` | só os módulos acima |
| `modifiers/mirror.rs`, `modifiers/array.rs` | nenhuma |
| `doc.rs` → `triangulate_polygon` | copiado do `convert.rs` (a ponte com o `anigo_core::Mesh` não veio) |

## Não trazidos (o ANIGO tem; entram depois sem rearquitetar)

`attributes`, `bvh`, `curve`, `hair`, `history` (a Modelagem guarda o documento inteiro por passo, ver `doc::History`),
`shapekeys`, `sculpt`, `uv`, `normals_tools`, `convert`, os operadores `bisect`, `bridge`, `fill`, `flip`, `spin`,
`triangulate`, `misc`, `smooth` (as ferramentas), e os modificadores `stack` (a pilha é o `doc::ModifierSpec`),
`solidify`, `weld`, `edge_split`, `decimate`, `displace`.

## Escrito no AnimeIAI

`doc.rs`: documento (objetos, transformação, cor toon, sombreamento, pilha mirror/subsurf/array), operações que a UI e a
MCP usam, conversão para `aiai_3d_model::Model` (GLB normalizado) e histórico.
