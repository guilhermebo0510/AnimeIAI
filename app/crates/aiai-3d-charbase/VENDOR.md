# VENDOR: aiai-3d-charbase

Commit do ANIGO: `bc5e83af1267eec28cc13c1777204b6ac1a6940f` (cópia de 2026-10-20). Código do mesmo autor (regra 4).

## Copiados de `crates/anigo-charbase/src`

| Arquivo aqui | Origem | Mudança |
|---|---|---|
| `src/cage/{mod,head,trunk,limbs}.rs` | `cage/*` | nenhuma |
| `src/character.rs`, `dims.rs`, `params.rs`, `qmesh.rs`, `skeleton.rs`, `subdiv.rs`, `uv.rs`, `weights.rs` | idem | nenhuma |
| `src/topology.rs` | `topology.rs` | o `AnatomicalZone` vem de uma cópia local do enum (o `anigo-core::morph_catalog` não foi trazido) |
| `src/lib.rs` | `lib.rs` | só os módulos acima |

## Não trazidos

`export.rs` (VRM, puxaria o `anigo-vrm`), `import.rs`/`preview.rs`/`morphs.rs`/`expressions.rs` (puxariam o
`anigo-core`), `parts.rs` (olhos, cílios, boca: o manequim é só o corpo), `contract.rs`, `debugview.rs` (puxaria o
`image`) e os testes do ANIGO (`tests/*.rs`; aqui fica `tests/gen.rs`, o mínimo que o Manequim usa).

O uso fica em `aiai-3d-model::mannequin` (malha do corpo + esqueleto + pesos → `Model` com `Skin`) e
`aiai-3d-model::pose` (pose por nome de osso, skinning na CPU).
