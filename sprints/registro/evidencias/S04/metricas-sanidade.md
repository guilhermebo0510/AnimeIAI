# S04 - Sanidade das metricas (aiai-metrics)

Quadro de referencia: um quadro do corte A (#138, quadro 12 do corte, 1920x1080, menino loiro). Variantes geradas por Python (`calibracao/sanity/`, fora do git): identica, deslocada 1 px e 2 px em x (borda replicada), deslocamento de cor com **DeltaE2000 medio = 4,00** (Lab: +5,87 na direcao (a,b) = (0,6; 0,8); medido com scikit-image), e desfoque gaussiano sigma 1,5 px.

Numeros produzidos pelo crate Rust (`compare` com opcoes padrao: PPD 67, tolerancia de borda 2 px, k = 6), via o gancho `harness_compare_files` (teste `#[ignore]` em `tests/suite.rs`). Tempo por comparacao 1080p: ~3,5 s (perfil de teste).

| Metrica | identica | deslocada 1 px | deslocada 2 px | cor DeltaE=4 | desfoque 1,5 px | limiar do protocolo |
|---|---|---|---|---|---|---|
| FLIP medio | **0,0000** | 0,0587 | 0,0935 | 0,1487 | 0,0425 | <= 0,05 |
| FLIP p50 / p95 / max | 0 / 0 / 0 | 0,017 / 0,247 / 0,768 | 0,028 / 0,395 / 0,963 | 0,146 / 0,196 / 0,298 | 0,016 / 0,193 / 0,649 | (teto por quadro 0,10 na media) |
| SSIM (luma) | **1,0000** | 0,9557 | 0,8932 | 0,9998 | 0,9616 | >= 0,95 |
| DeltaE2000 medio, regioes chapadas (78% da imagem) | **0,000** | 0,276 | 0,442 | 4,025 | 0,231 | <= 5,0 |
| DeltaE2000 medio, imagem toda | 0,000 | 1,182 | 2,001 | 4,000 | 0,905 | - |
| F1 de bordas @2 px | **1,0000** | 1,0000 | 1,0000 | 0,9984 | 0,9125 | >= 0,95 |
| precisao / revocacao | 1 / 1 | 1 / 1 | 1 / 1 | 0,9986 / 0,9983 | 0,9746 / 0,8578 | - |
| Espessura mediana (px) ref -> teste | 1,83 -> 1,83 | 1,83 -> 1,83 | 1,83 -> 1,83 | 1,83 -> 1,83 | 1,83 -> 3,00 (+64%) | +-5% |
| Espessura, media das amostras | 3,20 -> 3,20 | idem | idem | 3,20 -> 3,18 | 3,20 -> 4,11 (+28%) | - |
| Pior DeltaE por cor da paleta (6 grupos) | 0,00 | 0,03 | 0,08 | 5,26 | 0,05 | <= 3,0 |
| Camera (dx, dy) px, pico | (0, 0) 1,00 | (**1,00**, 0) 1,00 | (**2,00**, 0) 1,00 | (0, 0) 1,00 | (0, 0) 0,29 | <= 0,5% da largura = 9,6 px |
| Veredito (todas as metricas por quadro) | passa | falha (FLIP) | falha (FLIP, SSIM) | falha (FLIP, paleta) | falha (F1, espessura) | |

As metricas acusam o esperado:
- identica = perfeito em todas (FLIP 0, SSIM 1, DeltaE 0, F1 1, camera 0);
- o deslocamento e medido com exatidao pela correlacao de fase (1,00 e 2,00 px, sinal correto: dx > 0 = conteudo do teste mais a direita) e o FLIP/SSIM crescem de 1 para 2 px;
- a mudanca de cor aparece no DeltaE (4,025 nas regioes chapadas) e no FLIP/paleta, mas nao no SSIM de luma nem no F1 (como esperado);
- o desfoque derruba F1 (revocacao 0,86) e a espessura, quase sem mexer no DeltaE.

Validacao cruzada independente (scikit-image 0.26, mesmo quadro): SSIM e DeltaE2000 medio da imagem toda coincidem com o Rust em todas as colunas ate o 6o digito (por exemplo SSIM 0,955694 / 0,955694; DeltaE 1,1821 / 1,1821). O FLIP e o do C++ oficial (via `nv-flip`); nao ha segunda implementacao aqui para cruzar, apenas o teste de identidade/ordem.

## Achados que o supervisor precisa saber (regua do protocolo)

1. **A regua FLIP <= 0,05 e mais estrita que as demais.** Um deslocamento de **1 px** do quadro inteiro ja da FLIP 0,0587 (reprova) enquanto SSIM (0,956), F1 de bordas (1,0 a 2 px) e DeltaE chapado (0,28) passam. Um deslocamento de cor de DeltaE2000 = 4 (que a regra de DeltaE <= 5 aceitaria) da FLIP 0,149, acima ate do teto duro de 0,10. Na pratica, FLIP medio <= 0,05 exige cor com DeltaE bem abaixo de ~2 e posicao dos tracos com erro bem menor que 1 px. O PPD usado (67, valor padrao do FLIP e o fixado no protocolo) e conservador para um monitor Full HD de ~24" a 0,7 m (~46 PPD fisico), que seria ainda mais estrito. Decisao de limiar fica com o supervisor; o crate expoe `Thresholds` e `ppd` configuraveis e nada foi afrouxado.
2. **Tolerancia de 2 px no F1 de bordas aceita deslocamento de 2 px** (F1 = 1,0). O SSIM/FLIP e quem pega o erro de posicao; o F1 mede cobertura e forma dos tracos.
3. **Espessura de linha: a mediana por transformada de distancia e discreta.** Neste quadro as linhas tem ~2 px e a mediana so assume valores como 1, 1,83, 3; 10% de diferenca (0,18 px) e inalcancavel com mascara binaria. A mediana segue o protocolo (tolerancia +-5%) e o teste sintetico de linha 10% mais grossa (aneis de 20 x 22 px) passa, mas para linhas finas a media das amostras (campo `mean`) e a distribuicao (p10/p90) devem ser lidas junto. Uma tentativa de largura por area/comprimento de esqueleto foi descartada por ruido de ~5% do proprio comprimento do esqueleto digital. Um estimador sub-pixel (por cobertura em tons de cinza) fica como melhoria.
4. **Palette:** `match_palette(a, b)` (dois k-means independentes) cai em particoes diferentes mesmo para um simples deslocamento de 1 px (viu-se DeltaE 5-10 espurio). O `compare` usa `palette_shift`: agrupa a referencia e compara, grupo a grupo, a cor media da referencia com a do teste nos MESMOS pixels.

Imagens reduzidas (QA, 640 px de largura): [`sanidade-shift2.png`](sanidade-shift2.png) (referencia | deslocada 2 px | heatmap FLIP | overlay de bordas) e [`sanidade-outros.png`](sanidade-outros.png) (FLIP da cor DeltaE=4, FLIP do desfoque, overlay do desfoque, FLIP de 1 px). Dados completos: [`metricas-sanidade.json`](metricas-sanidade.json). Relatorio humano lado a lado (Python, com os numeros do Rust): `calibracao/sanity/py_shift2/report.png|html`.
