# S15 — Cor: correção, LUT, grading, scopes

## Objetivo
Correção e grade de cor por corte e por episódio, LUTs 3D (.cube) importadas/criadas/exportadas, scopes, comparação lado a lado, com pipeline de cor linear consistente até a saída Rec.709.

## Pesquisa obrigatória
DaVinci Resolve (primárias lift/gamma/gain/offset, curvas, qualificador HSL, nós), formato Adobe/IRIDAS `.cube` (1D/3D, DOMAIN_MIN/MAX), interpolação tetraédrica de LUT, scopes (forma de onda, vetorscópio, parade), grade típica de anime (contraste suave, céu ciano, pele quente, sombras azuladas).

## Tarefas
- [ ] Nó de correção primária: exposição, lift/gamma/gain (rodas de cor), offset, contraste com pivô, saturação, temperatura/tint.
- [ ] Curvas RGB/luma e curvas HSL (matiz vs. matiz, matiz vs. saturação, luma vs. saturação).
- [ ] Qualificador HSL → máscara para correção secundária; máscara de forma (power window) animável.
- [ ] LUT 3D: importar `.cube` (17/33/65), aplicar com interpolação tetraédrica (GPU e CPU), exportar a grade atual como `.cube`, intensidade da LUT.
- [ ] Onde aplicar: camada de ajuste, corte inteiro, episódio (grade mestre) — ordem definida e visível.
- [ ] Scopes: histograma, forma de onda (luma e RGB parade), vetorscópio, em painel ao vivo.
- [ ] Comparação: dividir tela (antes/depois, corte A vs. corte B, app vs. referência), wipe.
- [ ] Presets de look próprios (dia claro, entardecer, noite azulada, flashback dessaturado).
- [ ] MCP: `aiai_grade` (parâmetros), `aiai_lut` (importar/aplicar/exportar), `aiai_scopes` (devolve dados numéricos e imagem).

## Testes
- LUT identidade = imagem idêntica; LUT exportada e reimportada = mesmo resultado (erro < 1/1023).
- Golden por operação.
- Mouse: girar roda de cor, arrastar ponto de curva, ver scopes mudarem.

## Critério de pronto
- [ ] Prova de uso: o corte da S13 com três looks (dia, entardecer, noite) feitos por MCP e uma LUT exportada e reaplicada; scopes em evidências.
- [ ] Ajuste de grade que aproxima a média de cor de um corte do episódio (ΔE médio antes/depois registrado).
