# S24 · Spike 3D (fase 0): renderer cel do ANIGO offscreen nesta máquina

Data: 2026-10-20. Código: `app/crates/aiai-3d-render/tests/spike.rs` (`cargo test -p aiai-3d-render --test spike -- --ignored --nocapture`).
Imagem: `sprints/registro/evidencias/S24/3d_spike_1080p.png`.

## Máquina

- GPU: NVIDIA GeForce GTX 1650 (4 GB), escolhida pelo `HighPerformance` (a integrada Intel UHD também aparece). Backend: Vulkan.
- O app (eframe) também abre na GTX 1650 com Vulkan; o serviço 3D usa o MESMO device do egui (`HeadlessRenderer::from_device`).

## Cena do spike

Manequim proxy do ANIGO + esfera UV (32×48) + cubo, 3.240 triângulos, materiais toon com contorno (casco invertido),
fundo `[0,0,0,0]`, MSAA 4x, passes `outline → cel → dof_post` (o último é o resolve; DoF desligado).

## Números (`render_scene` completo: codificar, enviar, ler de volta)

| Tamanho | 1º quadro | mediana de 10 | mín / máx | Alocador wgpu (alocado / reservado) |
|---|---|---|---|---|
| 1920×1080 | 17,6 ms | **4,8 ms** | 4,5 / 5,6 ms | 64 MB / 256 MB |
| 960×540 (prévia do arrasto) | 4,4 ms | **1,7 ms** | 1,5 / 2,0 ms | 17 MB / 256 MB |
| 3840×2160 | 27,6 ms | **16,1 ms** | 15,6 / 17,3 ms | 256 MB / 512 MB |

- Antes do primeiro quadro: 0,5 MB alocados / 192 MB reservados (pipelines, rampa toon, mapas neutros).
- VRAM: o `nvidia-smi` deste notebook (WDDM) não informa memória por processo (mostra 0 MiB); o número confiável é o do
  alocador do wgpu (`Device::generate_allocator_report`), acima. 1080p fica em ~64 MB, bem dentro dos 4 GB.
- No app, o quadro inteiro do canvas com a camada 3D (GPU + composição CPU das outras camadas) ficou em ~0,9 a 1,4 s no
  projeto de calibração cheio (o custo é das camadas 2D; o raster 3D sai do cache por estado na segunda vez).

## Alfa e espaço de cor

- Canto vazio: `[0,0,0,0]` (fundo transparente de verdade). 1.793.887 de 2.073.600 pixels transparentes; 3.940 com alfa
  parcial (bordas do MSAA); **nenhum pixel com cor acima do alfa** → a saída é RGBA **pré-multiplicada**.
- O shader escreve sRGB codificado num alvo `rgba8unorm` ("linear_working_no_srgb_view"), então a saída é **sRGB
  pré-multiplicado**, o formato do compositor do AnimeIAI (decisão 16). Nenhuma conversão extra.
- As cores de material entram em **sRGB** (o shader lineariza). O shader aplica exposição fixa `exp2(1) = 2` e tonemap
  ACES: sem compensação a referência sai lavada (azul vira azul-claro). `aiai_3d_model::scene::precompensate` inverte
  exposição + ACES, e a luz/ambiente vão a branco/1,0: a camada sai com as cores do modelo (luz = base, sombra = base ×
  0,62/0,62/0,74, igual ao rasterizador CPU). Evidência: `evidencias/S24/3d/03_camada_so_gpu.png` (antes) e
  `05_camada_so_gpu_cor.png` (depois).

## Disco

- Livre em C: antes do spike: 13,8 GB (`df -m`: 13.832 MB). Depois do teste do spike: 13.656 MB.
- `target/debug` antes: 14.286 MB; depois do teste do spike: 14.461 MB → **+175 MB** (crates vendorizados + binário de
  teste; o wgpu já compilado foi reaproveitado, nenhuma variante nova de wgpu/naga).
- Fim do trabalho (studio, bridge, MCP, testes; com os outros trabalhadores compilando ao mesmo tempo): `target/debug`
  16.951 MB, C: com 11 GB livres. Artefatos `aiai_3d_*` em `target/debug/deps`: ~95 MB.

## Veredito

O caminho GPU passa com folga (4,8 ms a 1080p, 1,7 ms na prévia reduzida, 64 MB) e com alfa/cor corretos. Não foi preciso
cair para a CPU na v1. O rasterizador CPU (`aiai_3d_model::raster`) existe mesmo assim: é o padrão do compositor sem GPU
registrada (headless/MCP sem app, testes) e desenha o modo `wire`, que o renderer cel não tem.
