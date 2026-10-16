# Calibração — corte A (#138, menino loiro em close, 36 quadros)

## Análise da referência
- Exposição: desenhos novos nos quadros 0, 3, 6, 9, 12 (leve movimento do personagem) e 21, 24, 27 (boca); o resto repete.
- Fundo estático e desfocado (σ≈3 medido nas transições de 10–90% ≈ 8 px): papel de parede lilás com damasco dourado (esquerda), filete dourado, parede creme, batente lilás com frisos, porta marrom, filete dourado (direita).
- **O fundo não tem nenhuma aresta no Canny da régua**: as 44 049 px de borda da referência no quadro 12 estão todas no personagem. Fundo só pesa em SSIM/FLIP (lição 53).
- Personagem: tinta marrom (#6a3533 no núcleo), linhas de ~2 px com halo claro; pálpebras grossas oliva (#6f5530, ~10 px); íris com contorno só em cima e dos lados.

## Ferramentas (todas pelo app, headless via `aiai-mcp call`)
- Fundo (`a138_bg.py`): faixas verticais DIGITADAS (bordas lidas no perfil por coluna); cor por banda de 60 linhas = mínimos quadrados contra o perfil medido com o mesmo desfoque simulado (resíduo ~2 níveis); FILL_SHAPE por faixa×banda; camada de efeito `bg_foco` (gaussian_blur raio 9 = σ 3) logo acima do BG.
- Damasco (`a138_orn.py`): peças digitadas (metade + espelho no eixo do motivo), MEASURE_EDGE com busca 7→3 (vãos de 8 px entre pétalas), FILL_SHAPE na cor do PROBE_COLOR. Só a flor em leque por enquanto.
- Personagem (`a138_char.py`): ~180 linhas digitadas lendo `ink_grid.py` (recorte só de tinta com grade); paquímetro de meia altura + âncora (busca longa, ou curta nas áreas escuras do laço); olhos e broche com geometria digitada sem medição (o miolo escuro engana o paquímetro) e cores medidas em perfis; nariz como forma chapada medida; fechos ocultos (íris, branco dos olhos, sombras do cabelo, pescoço×gola); balde em 2 passadas (gap 36 para as regiões grandes, gap 4 para as estreitas que ficam sem núcleo), rótulos calculados como o balde do app (núcleos D > gap/2 propagados por watershed, sem atravessar linha); fundo = regiões que contêm pontos digitados de fundo.
- `a138_opt.py`: variante de medição por linha (longa / curta / digitada) escolhida pela régua.

## Andamento (quadro 12; limites F1 0,956 / SSIM 0,935 / FLIP 0,2124)
| passo | F1 | SSIM | FLIP |
|---|---|---|---|
| só fundo | 0,082 | 0,841 | 0,380 |
| + queixo, cabelo (100 linhas) | 0,607 | 0,845 | 0,373 |
| + olhos, gola, laço, paletó + balde gap 36 | 0,697 | 0,874 | 0,227 |
| + fechos, 2ª passada, fundo por pontos | 0,716 | 0,876 | 0,217 |
| + franja, sombras, olhos por dentro, watershed | 0,747 | 0,878 | 0,2123 |
| + babados da gola, broche pelos perfis | 0,768 | 0,879 | 0,2105 |

## Tentativas que pioraram (desfeitas)
- Fechos das pontas brancas da camisa sob o laço + metade escura da pedra: F1 0,768 → 0,767 e a pedra ficou branca (posições lidas erradas). Reler com recorte só de cor antes de repetir.

## Faltando (mapa de faltantes por célula)
Franja sobre o olho esquerdo (cunha rosa cortada pela dobra grossa), faixa rosa da testa à direita, orelhas, bordas do laço (marinho×marinho), friso claro das lapelas, brilhos e sombras restantes do cabelo, damasco restante, satsuei do personagem (σ≈1?).
