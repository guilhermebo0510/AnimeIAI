# S23: backlog vindo da auditoria externa (`AUDITORIA.md`, 2026-10-19)

A auditoria foi lida inteira e cada afirmação de P0 foi conferida no código antes de mexer. Status abaixo:
**feito** (com teste e evidência), **não reproduziu** (com o porquê), **mantido** (decisão com motivo) ou **pendente**.

## P0: bugs, perda de dados e travamentos

| Item da auditoria | Status | O que foi feito / por quê |
|---|---|---|
| 1.6 Mover seleção parcial em camada vetorial apaga os outros traços | **não reproduziu** | `ReplaceStrokes` troca traço por id (`aiai-core/src/command.rs`), não a lista inteira. Conferido com mouse real no C002: 8 traços antes e depois, só o selecionado andou (`evidencias/S23/auditoria/01..04`). O que existia de fato: prévia e comando usavam o centro da caixa do traço, então um traço grande que mal tocava a seleção ia junto. Agora os dois usam a maioria dos pontos (`stroke_in_selection`), com teste de regressão. |
| 1.1 Balde: regiões fechadas estreitas sem rótulo + `label_near` pula a linha | **feito** | Componentes que sobram depois das ondas viram regiões próprias; `label_near` anda só por pixels de linha. Teste `narrow_closed_region_gets_its_own_label_and_never_leaks`. |
| 1.2 Lint: pontos cegos (borda, um pixel só, regiões estreitas) | **feito** | Cobertura por pixel (≥ 95%), regiões estreitas entram (efeito do 1.1), regiões da borda sem tinta vão para `border_unpainted` (fora a maior, que é o "fora"), contadas no `lint_cut` e no relatório. |
| 4.9.1 Afinamento duplo (perfil × taper) | **feito** | Vale o mais fino dos dois em vez do produto. |
| 1.7 C Catmull-Rom uniforme no pincel | **feito** | Centrípeta (α = 0,5). A quina medida no teste passava 12,3 px; agora passa ~2,6 px. |
| 1.7 C "Fechar ao voltar" ligado por padrão | **mantido** | Desligar faz o quadrado desenhado à mão vazar no balde (teste de mouse real da S03 quebrou). Continua como opção em Propriedades. |
| 1.7 A Prévia do arrasto ≠ traço final | **feito (caneta)** | Prévia e fantasma desenham a geometria do motor (`sample_stroke`). Pincel raster ainda é uma polilinha da largura do pincel (pendente: prévia de carimbos). |
| 1.7 B Cauda do estabilizador ao soltar | **mantido** | É o "alcançar o cursor" do estabilizador (média/corda); sem isso o traço termina antes de onde o usuário soltou. |
| **Achado novo no teste de mouse** | **feito** | Numa camada com movimento (C002, braços que entram por `LAYER_MOTION`) o traço caía deslocado do cursor. Caneta, pincel, borracha e balde agora trabalham no espaço do cel (`layer_placement`). Pendente: seleção/mover nessas camadas. |
| 4.3 `sync_all` por tile, WAL, GC, log, recuperação | **feito** | Salvar grava os blobs em paralelo; WAL sem sync por tile (a linha do WAL é sincronizada); coleta automática a cada 64 ações fora do histórico e ao salvar (idade mínima de 32 comandos para não levar tiles de um comando planejado); log guarda o comando só das últimas 5000 entradas; abrir com recuperação mantém o undo e fica "não salvo". |
| 4.3 Perda total antes do primeiro Salvar | **feito** | Autosave a cada 2 min de projeto sem pasta em `%LOCALAPPDATA%\AnimeIAI\recovery`; a tela inicial oferece Recuperar/Descartar depois de uma queda. |
| 1.8 Aquarela, borrador, guache | **feito** | Pigmento na borda de fora da poça; borrador com pegada 2D (camada + traço em andamento); direção com passa-baixa. Prancha em `evidencias/S23/auditoria/pinceis_prancha.png`. |
| 1.3 Vetor rasterizado antes da câmera | **feito** | Cel só de vetor com transformação é rasterizado já no lugar final. Teste com zoom 4×. |
| 1.8 Rosas e Nuvem (carimbo de uma cor, escamas) | pendente (P1) | Precisa de carimbo bicolor/multiponta (auditoria 4.9.3). |

## P1 feito

| Item | Status |
|---|---|
| 7 Pressão da mesa no 1º ponto | **feito** (`63b8cd2`): o 1º ponto pega a pressão do `Touch Start` do mesmo quadro (antes ou depois do botão); achado no teste: os pontos de alcance do estabilizador ao soltar viravam 1,0 (fim do traço grosso) e agora mantêm a última pressão; mouse volta a 1,0. Lido de volta no log do app (0,15 → … → 0,2) e `evidencias/S23/auditoria/pressao_1o_ponto.png`. WM_POINTER/inclinação/curva de pressão: pendente. |

## P1/P2: registrados, não começados

Desempenho em 4K (cache por camada, tiles locais, cache do balde), mesa digitalizadora (pressão no 1º ponto, WM_POINTER), áudio ao vivo e forma de onda, importar imagem e Sub View, miniaturas de pincel, edição de nós e borracha por interseção, onion skin, iro-tresse, 3D cel-shading, cor linear. A ordem segue a Parte 5 da auditoria, depois do curta.

## Curta (auditoria 3.2)

| Item | Status |
|---|---|
| Remendo em `curta.py`/`c21_sobras.py` que pintava sobras com a cor vizinha para zerar o lint | **feito**: removido. Sementes explícitas no rig para cada região que sobrou (mechas do topo do Ren, pele entre mechas da franja dos dois, aba da gola, avesso das mangas), alças do avental como forma (no 3/4 a alça distante é mais estreita que o fechamento do balde), frestas ≤ 300 px pela ferramenta `close_leftovers`. Os 7 cortes repintados (`c22_repintura.py`): lint 0 com a régua nova (cobertura por pixel, regiões estreitas, borda). Um erro meu no caminho: a semente de pele embaixo do vale da franja caiu na linha e pintou o cabelo da Hana de pele no C003; o lint não pega cor errada, o ΔE do `c20` pega; corrigido com o centroide do triângulo. |
| Teste de consistência que comparava 0,60 com 0,60 (`c20_revisao.py`) | **feito**: mede topo, olhos e queixo no render do corte e do model sheet (vista de frente) do mesmo jeito; C003 0,54%, C004 0,92%. |
| Model sheet: manequim em 90° com desenho de costas | **feito**: manequim nas vistas desenhadas [0, 30, 180]. Perfil (90°) desenhado: pendente (o rig não tem vista de perfil). |
| Corte de ensaio C999 dentro do projeto de produção | **feito**: removido; `ensaio.py` usa um projeto à parte. |
| Cabelo do Ren em 3/4 | avaliado: a silhueta parada é a do crânio (esfera), o lado distante mostra mais cabeça como no 3/4 real; a bochecha do lado próximo passa à frente do cabelo lateral. Melhorar com mais um estágio de giro do cabelo lateral: pendente. |
| Tronco que não gira em 3/4 | pendente (rig) |
| Rim light, para, blush nos personagens | pendente (satsuei por corte) |
