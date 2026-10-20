# VENDOR: aiai-3d-render

Origem: `C:\ANIGO\crates\anigo-renderer` (src, shaders WGSL e testes) e `C:\ANIGO\contracts\fixtures\render_contract_v1.json`.
Commit do ANIGO: `bc5e83af1267eec28cc13c1777204b6ac1a6940f` (cópia de 2026-10-20).

## Copiados sem mudança de conteúdo (exceto `anigo_core` -> `aiai_3d_core`, `anigo_renderer` -> `aiai_3d_render`)

- `src/*.rs` (culling, device_recovery, diagnostics, face_sdf_map, headless, mesh_validation, outline_normals,
  render_contract, render_graph, render_targets, uniforms)
- `shaders/*.wgsl` (cel_shading, inverted_hull, anime_eye, face_sdf, morph_sparse_compute, postprocess_dof)
- `tests/wgsl_validation.rs`
- `contracts/render_contract_v1.json` (era `contracts/fixtures/` na raiz do ANIGO)

## Alterados

- `src/render_contract.rs`: `include_str!` do contrato aponta para `../contracts/render_contract_v1.json`. Os caminhos de
  shader declarados no JSON continuam os do ANIGO (são dado do contrato, não caminho de arquivo).
- `src/headless.rs`, teste `test_headless_renderer_canonical_base_mesh`: usa o manequim proxy (a base canônica não veio).
- `Cargo.toml`: `pollster = "1"` (o ANIGO fixa 0.4; a 1.x já estava no `Cargo.lock` do AnimeIAI, mesma API `block_on`).

## Não vieram

- `shaders/webgl2_fallback/*.glsl` (só o viewport TypeScript legado usava).
- Testes de integração que dependem da base canônica ou de hashes de quadro do ANIGO: `golden_frames`, `cel_features`,
  `extra_lights`, `light_response`, `render_to_texture`, `shader_math_parity`, `shadow_map`. O AnimeIAI mede o seu
  próprio caminho em `tests/spike.rs` (fase 0) e nos testes do `aiai-3d-model`.

## Notas do AnimeIAI

- A cor de saída já sai codificada em sRGB pelo shader (alvo `rgba8unorm`, "linear_working_no_srgb_view"); as cores de
  material entram em **sRGB** (o shader converte para linear com `srgb_to_linear`). Com fundo `[0,0,0,0]` o resolve do MSAA dá RGBA sRGB **pré-multiplicado** (medido no spike:
  nenhum pixel com cor acima do alfa), que é o formato do compositor do AnimeIAI.
