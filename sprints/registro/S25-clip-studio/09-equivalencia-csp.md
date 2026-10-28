# Equivalência das subferramentas do Clip Studio Paint 5.1.5 (28/10/2026)

Lista tirada do banco de ferramentas do próprio Clip Studio (`calibracao/csp/tooldb_arvore.json`, 203 subferramentas
em 30 grupos). Para cada uma: o que existe no AnimeIAI. **=** equivalente (calibrado contra a referência quando é
pincel/caneta), **~** parecido/parcial, **—** não existe (com o motivo).

## Calibração rodada de novo (seção 2 do plano)

Pranchas regeradas com o motor de hoje (`calib_render.py --factory`) e pelo app (`calib_app.py`, canetas) e comparadas
com as referências do Clip Studio (`calib_compare.py`, `calib_color.py`):

| Grupo | Fim da S25 | Hoje | Nota |
|---|---|---|---|
| Canetas (app) | 0,356 (26/10, mesma entrada) | 0,745 média; G-pen 3 0,367 | mesmo resultado da rodada de 26/10 pelo app; a régua Steger (4.9h) põe o centro a ≤0,61 px em todas e aponta w(s) fora em Pena G real 10/30, caligrafia 10/30, texturizada, rugosa, feltro e marcador chato 3 |
| Lápis | 2,641 | 2,532 | melhorou |
| Aerógrafo | 1,716 | 1,715 | igual |
| Borracha | 0,335 | 0,337 | igual |
| Pincel (cor) | 19,139 | 19,123 | igual, nenhuma folha pior |
| Misturadores (cor) | 12,58 | 12,56 | igual |
| Decoração (cor) | 50,30 | 50,26 | igual |

A Caneta de feltro foi testada com três variações (velocidade mais forte, borda mais macia): as três pioraram o
conjunto (o traço rápido longo afinou demais enquanto os curtos rápidos seguiram largos) e o preset ficou como estava.

## Tabela

| Grupo do Clip Studio | Subferramentas | AnimeIAI |
|---|---|---|
| Degradê > Degradê | Primeiro plano → transparente, → plano de fundo, Listra, Listra de cor de fundo | = prontos "Cor principal → transparente/secundária", "Listra", "Listra com a secundária" |
| | Fotosfera | = pronto "Fotosfera" (radial, 28/10) |
| | Azul céu, Céu de meio-dia, Pôr-do-sol, Céu noturno, Arco-íris | = "Céu azul", "Céu de dia", "Pôr do sol"/"Fim de tarde", "Céu noturno", "Arco-íris" |
| | Apagar degradê | ~ degradê numa máscara da camada (o degradê em si não apaga) |
| | Degradê de mangá | ~ degradê + Filtro > Plugins > Retícula (4.9g) |
| Degradê > Forma livre | Pintura normal, A sombra mais clara/escura | = Degradê livre (pontos de cor) |
| Mover | Mão, Girar, Ampliar, Diminuir | = Mão, Girar, Zoom (clique/Alt) |
| Caneta | G-pen, G-pen real, Mapeamento, Nanquim (nabo), Texturizada, Rugosa, Caligrafia, Milli, Feltro, Pixel, Marcador plano | = os 11, calibrados (folhas `calibracao/csp/ref/caneta`) |
| Corrigir linha | Ponto de controle, Sucção, Simplificar, Conectar, Ajustar largura, Redesenhar linha, Redesenhar largura | = os 7 (`LINE_FIX`) |
| Corrigir linha > Remover partículas | Remover partículas, Preencher restante, Selecionar poeira | = Remover poeira (com os modos) |
| Desenhar | Lápis, Lapiseira, Carvão, Giz de cera, Pastel, Giz | = os 6, calibrados |
| Pincel > Aquarela | 11 (redondo, plano, transbordo, lavagens, salpicos, aquoso, caneta pincel, tintas) | = os 11, calibrados (cor) |
| Pincel > Pintura grossa | 9 (mistura redondo, guaches, óleos, pontilhismo, combinador guache, pintar e aplicar) | = os 9, calibrados (cor) |
| Dissolver | Dissolver | ~ Borracha macia/Ruído (sem o modo de dissolver próprio) |
| Aerógrafo | Suave, Spray, Gotícula, Ruído, Spray colorido pingando | = os 5, calibrados |
| Borracha | Duro, Suave, Limpa-tipos, Rugoso, Dinâmica, Vetor, Várias camadas, Ajustar | = os 8 (Ajustar = encaixe), calibrados |
| Combinar | Combinar, Desfoque, Ponta do dedo, Misturador de pintura, Molhado, Texturizado | = os 6, calibrados (cor) |
| | Copiar carimbo | ~ Seleção + Copiar/Colar e o pincel de mistura (sem o carimbo com origem) |
| Preencher | Referir outras camadas, Só a camada, Envolver e preencher, Laço de preenchimento, Caneta restante | = os 5 (`FILL_AREA`) |
| Seleção automática | Todas as camadas, Só a camada, Camadas de referência | = os 3 |
| Área de seleção | Laço, Retângulo, Elipse, Polilinha, Caneta de seleção, Apagar seleção, Encolher seleção, Laço magnético | = os 8 |
| Figura > Desenho direto | Linha, Curva, Polilinha, Curva contínua, Bézier, Retângulo, Elipse, Polígono | = os 8 |
| Operação | Objeto, Selecionar camada, Mover camada | = Mover/Selecionar camada |
| | Mesa de luz, Editar linha do tempo | = Mesa de luz e Linha do tempo (painéis) |
| | Mover padrão de retículas, Mover grade | — retícula de mangá como camada não existe (anime) |
| Conta-gotas | Cores exibidas, Cor da camada | = os 2 |
| Decoração > Efeito | Faísca, Luz dispersa, Glitter, Círculo suave, Pluma, Pétala, Rosa, Cereja | = Brilhos, Luz entre folhas, Purpurina, Círculos suaves, Penas, Flores/pétalas, Rosas, Flores de cerejeira |
| | Mancha de sangue | = "Mancha de sangue" (28/10) |
| Decoração > Plano de fundo | Folhas, Grama, Árvores, Gramado, Placa de grama, Folhas caídas, Chão, Rachadura, Nuvem, Cascalho | = Folhagem/Folhas soltas, Grama, Árvores, Gramado, Folhas caídas, Chão, Rachadura, Nuvem, Cascalho |
| | Livro, Prédio | = "Livros", "Prédios" (28/10) |
| Decoração > Roupas | Renda, Floreado simples, Trança, Corrente fina, Botão, Zíper, Corrente, Corrente de contas | = "Renda", "Floreado simples", "Trança", "Corrente fina", "Botões", "Zíper", "Corrente", "Contas" (28/10, multi-ponta em sequência) |
| Decoração > Granulação e areia | Gaze, Nuvem de gaze, Granulação cruzada x1/x4, Travessas, Fuligem, raspagem de tom | ~ "Textura de papel" e o grão dos pincéis; os padrões de raspagem de retícula são de mangá |
| Decoração > Linha com régua | Ondulada, Bordas rugosas, Irregular, Tracejada, Pontilhada, Estrela, Melodia | = "Linha ondulada", "Bordas rugosas", "Linha irregular", "Linha tracejada", "Linha pontilhada", "Linha de estrelas", "Melodia" (28/10) |
| Texto | Texto, Texto circular | = os 2 |
| Régua | Linear, Curva, Figura, Caneta de régua, Especial, Guia, Perspectiva, Simétrica | = os 8 (os 15 tipos na prateleira) |
| Quadrinho | Balões, Borda do quadro, Flash, Linhas radiais, Linhas de velocidade | ~ Linhas de foco e de velocidade (Figura e Filtro > Renderizar); balões e quadros de mangá ficam de fora (o AnimeIAI faz anime, a legenda vem da Edição) |

## O que ficou de fora e por quê

- Retícula de mangá como camada (mover padrão de retículas, granulação para raspar tom), balões e bordas de quadro:
  ferramentas de mangá impresso. Para anime, a retícula sai pelo plugin Retícula e a legenda pela Edição.
- Copiar carimbo e Dissolver como modos próprios: os resultados são alcançáveis com o que existe; ficam na lista do
  criador de pincéis (seção 3) se o estudo mostrar que valem.
