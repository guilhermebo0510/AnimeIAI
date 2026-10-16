# S13/S15 - tempos dos efeitos em 1920x1080

Máquina: i7-10750H (6C/12T), CPU, rayon. Perfil: release (lto thin). Cena: céu + janela + personagem + pontos (opaca, 1920x1080, RGBA f32).
Tempo = melhor de 3 execuções (depois de 1 de aquecimento) já sem o custo de clonar a imagem; coluna 3 = mediana. Meta: <= 400 ms.

| efeito (parâmetros em fração da largura) | melhor ms | mediana ms | meta |
|---|---:|---:|---|
| gaussian_blur | 114 | 118 | ok |
| directional_blur | 159 | 169 | ok |
| radial_blur | 202 | 213 | ok |
| color_aware_blur (noisy) | 281 | 284 | ok |
| glow | 120 | 121 | ok |
| diffusion | 28 | 29 | ok |
| halation | 75 | 81 | ok |
| transmitted_light | 129 | 131 | ok |
| light_wrap | 133 | 139 | ok |
| rim_light | 102 | 106 | ok |
| para (multiply) | 2 | 3 | ok |
| chromatic_aberration | 26 | 26 | ok |
| vignette | 6 | 7 | ok |
| grain | 13 | 14 | ok |
| deband | 4 | 5 | ok |
| lens_flare | 96 | 97 | ok |
| god_rays | 115 | 118 | ok |
| drop_shadow | 103 | 104 | ok |
| outline (white) | 59 | 62 | ok |
| color_overlay | 2 | 2 | ok |
| invert | 0 | 1 | ok |
| two_tone | 9 | 9 | ok |
| adjust (hue/sat) | 21 | 25 | ok |
| lut (sunset .cube) | 18 | 20 | ok |
| grade (teal/orange) | 13 | 13 | ok |
| gaussian_blur r=30 (exato, pior caso) | 132 | 140 | ok |
| gaussian_blur r=120 (caixa×3) | 96 | 103 | ok |
| directional_blur length=200 | 239 | 243 | ok |
| radial_blur amount=0.5 | 361 | 390 | ok |
| color_aware_blur r=30 | 291 | 299 | ok |
| glow r=200 | 100 | 102 | ok |
| lut 33^3 (aplicar) | 17 | 18 | ok |

## Scopes e LUT

| operação | melhor ms | mediana ms |
|---|---:|---:|
| scopes::histogram | 8 | 9 |
| scopes::waveform | 20 | 20 |
| scopes::rgb_parade | 52 | 54 |
| scopes::vectorscope | 13 | 14 |
| Lut::parse 65^3 (274625 linhas) | 45 | 45 |
| bake_lut Grade 33^3 | 1 | 1 |

Pior efeito: 361 ms. Todos dentro da meta de 400 ms.
