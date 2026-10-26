# S25 — Comparativo com o Clip Studio Paint (pedido do usuário de 2026-10-20)

Objetivo: o resultado das ferramentas de desenho (canetas, pincéis, aerógrafo, decoração, borracha, mistura, balde, seleção) o mais
próximo possível do Clip Studio Paint, e a organização dos painéis de desenho inspirada nele (mais ícones, menos texto), com a
nossa identidade visual. Não é cópia de interface nem de código: é ver o funcionamento e o resultado e fazer parecido ou melhor.
Foco em anime (mangá fica de fora). Onde o Photoshop for melhor (fundos e ilustração), ele também entra.

## Acesso

- **Clip Studio Paint EX 5.1.5** (pt-BR), em `C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe`.
- **MCP do Clip Studio: não instalado.** O único existente (A2pha/clip-studio-paint-5.1.2-production-mcp, não oficial, 7 estrelas)
  só funciona com a versão 5.1.2 em chinês e com escala de tela de 120 DPI, e se recusa a rodar fora disso. Além disso, o próprio
  autor documenta que o Clip Studio não tem API de camadas ou pincéis: o MCP só tira capturas e manda mouse e teclado, o mesmo que
  o nosso harness faz. Não vale o risco de instalar código de terceiros.
- **Controle do Clip Studio:** `tools/harness/input.ps1 -Process CLIPStudioPaint -TitlePrefix CLIP` (foco, cliques, teclas, captura).
  A captura da janela maximizada começa em (-9,-9): coordenada de tela = coordenada na imagem − 9.
- **Caneta com pressão:** `tools/harness/pen.ps1` injeta uma caneta Windows Ink sintética, com pressão de 0 a 1, a partir de um
  arquivo de pontos. A mesma entrada vai para os dois apps.
  - No nosso app a pressão chega: o traço afina nas pontas.
  - No Clip Studio ainda não. Ele está no modo **Wintab**, e não há mesa nem driver Wintab no PC, então ele trata a caneta como
    mouse (pressão 100%). A opção **Tablet PC** aparece desabilitada porque nenhuma caneta Windows Ink existia quando ele abriu.
    Para ligar a pressão é preciso reiniciar o Clip Studio com a caneta sintética ativa e escolher Tablet PC em
    Preferências > Tablet.
- **Nosso app:** cópia de teste `calibracao/tmp/ui_copy.aiai`, com `input.ps1 -ProcessId <pid>`, sob o `ui.lock`. As capturas do
  Clip Studio ficam em `calibracao/csp/`, fora do git.

## Método

1. **Levantamento por grupo de ferramentas**, na ordem da prateleira do Clip Studio. Em cada sub-ferramenta:
   - abrir as propriedades e o detalhe da sub-ferramenta (os números de tamanho, pressão, espaçamento, textura, mistura);
   - desenhar a folha de teste padrão (abaixo) e guardar a imagem de referência;
   - anotar o que temos, o que falta e o que muda na interface na tabela deste arquivo.
2. **Folha de teste padrão** (pontos em coordenadas do canvas, os mesmos nos dois apps):
   - linha com rampa de pressão 5% → 100% → 5%;
   - linha com pressão constante (30%, 60% e 100%);
   - círculo; curva em S; rabisco aleatório com semente fixa; traços rápidos (com afinamento);
   - traço sobre traço (cruzamento), traço com cor diferente por cima (mistura), borracha atravessando;
   - os mesmos traços em 3 tamanhos de pincel;
   - capturas a 100% de zoom para comparar pixel a pixel.
3. **Calibração no nosso app:** os agentes repetem os mesmos pontos pelo nosso app (a bridge aceita caneta com pressão) e comparam
   com a referência do Clip Studio (largura ao longo do traço, perfil da borda, opacidade acumulada, textura, espaçamento). O ajuste
   para quando a diferença medida é pequena e a comparação visual lado a lado confere.
4. **Em lotes:** cada grupo levantado vira um lote de mudanças para os agentes, enquanto eu sigo levantando o grupo seguinte. Só eu
   uso o mouse no Clip Studio; os agentes não precisam dele porque trabalham sobre as imagens de referência.

## Primeiras observações (capturas 00–08)

Organização do Clip Studio (padrão de fábrica do usuário):

- **Prateleira de ferramentas** estreita, só ícones. Cada ferramenta abre um **grupo de sub-ferramentas** com abas: Caneta | Marcador,
  Lápis, Pincel, Aerógrafo, Decoração e outras.
- **Painel Sub-ferramenta:**
  - a lista mostra uma miniatura do traço e o nome de cada sub-ferramenta (G-pen, Real G-Pen, Mapping pen, Turnip pen, Calligraphy…);
  - no rodapé há ícones para adicionar dos baixados, importar, duplicar e apagar.
- **Painel Propriedades da ferramenta:**
  - no topo, a prévia do traço com o nome;
  - abaixo, controles deslizantes curtos, com ícone de pressão ao lado (Tamanho do pincel, Opacidade, Suavização de serrilhado em
    4 ícones, Estabilização);
  - um ícone de chave inglesa abre o detalhe completo.
- **Paleta Tamanho do pincel:** grade de tamanhos prontos (0,7 a 200) desenhados como bolinhas do tamanho real. Fica desabilitada,
  e não some, quando a ferramenta não tem tamanho.
- **Cores embaixo à esquerda:** roda de cores com abas em ícones (roda, controles, conjunto de cores, cor intermediária, cor
  aproximada, histórico).
- **Direita:**
  - Navegador no topo, com zoom e rotação em controles deslizantes, botões de girar, espelhar e redefinir;
  - Propriedades da camada;
  - Camada | Histórico | Ação automática em abas;
  - a coluna de ícones entre o canvas e a direita é a **barra de materiais**: cada ícone abre um painel por cima do canvas.
- **Barra de comandos no topo:** novo, abrir, salvar, desfazer, refazer, apagar, seleção (inverter, mostrar), transformar, encaixe na
  régua / régua especial / grade, entre outros. São comandos que não estão repetidos em outras barras.
- **Canvas girado e espelhado** pelo Navegador e por atalhos.
- **Barra flutuante de seleção** (o "lançador") aparece embaixo da seleção.
- **Novo documento:**
  - tipo de projeto em ícones (Ilustração, Webtoon, Quadrinho, Fanzine, Personalizado, Animação);
  - predefinições salváveis e unidade;
  - cor de papel, perfil de cor, "Criar ilustração animada", gravar timelapse;
  - o modo Animação tem quadro de saída, área segura, espaço em branco, taxa e tempo, cena, plano e linha do tempo.
- **Preferências:** Ferramentas, Tablet, Gestos de toque, Comando, Interface, Desempenho, Cursor, Camada/quadro, Animação,
  Régua/Unidade, Tela de pintura, Arquivo, Conversão de cores, Editor de história, 3D, Conexão com smartphone, Tabmate 2.

Nosso app hoje, no mesmo lugar:

- a barra do canvas tem muito texto ("Redondo duro ›", "Tamanho 24 px", "Opacidade", "Média móvel 4");
- o painel Ferramenta mostra o pincel com texto ("Todos (15)", "Lápis e canetas (1)", "Salvar como pincel novo…", "Criar pincel…",
  "Importar…");
- só há Pincel raster e Caneta vetorial, sem grupos (Lápis, Pincel, Aerógrafo, Decoração, Mistura);
- não há paleta de tamanhos, barra de materiais, abas Histórico e Ação automática nem lançador de seleção.

## Fonte numérica: o banco de ferramentas do Clip Studio

- Cada sub-ferramenta do Clip Studio é uma linha da tabela `Variant` (SQLite) com cerca de 507 parâmetros. Exemplos: tamanho,
  dureza, espaçamento, suavização, estabilização, entrada/saída por velocidade, espessura e ângulo da ponta, textura, mistura de
  cor, aquarela. As curvas de pressão e de velocidade ficam em blobs `*Effector`.
- Fábrica: `C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\Settings\PAINT\Tool\portuguese\EditImageTool.todb`.
- Do usuário: `%APPDATA%\CELSYSUserData\CELSYS\CLIPStudioPaintVer1_5_0\Tool\EditImageTool.todb`.
- Leitura só de cópia, em modo leitura. O leitor fica em `tools/csp/csp_tooldb.py`, feito pelo agente de calibração.
- Os números são fatos para reproduzir o comportamento. As imagens de ponta e textura da CELSYS não entram no nosso app: fazemos
  as nossas.
- Árvore de fábrica: 25 ferramentas, com grupos e cerca de 220 sub-ferramentas. Em desenho: Caneta (11), Lápis (6), Pincel
  (aquarela 11, pintura grossa 9, nanquim), Aerógrafo (5), Borracha (8), Mistura (7), Balde (5), Seleção automática (3), Seleção
  (8), Figura (8), Degradê (15), Decoração (efeito 9, fundo 12, roupas 8, granulação 9, linha com régua 7), Corrigir linha (10).

## Pincéis do usuário alterados (restaurar ao terminar a calibração)

Antes de capturar, restaurei a configuração inicial destes. No fim, devolver os valores do usuário:

- **G-pen:** suavização nenhuma; estabilização 100; tamanho 4; ímã vetorial ligado.
- **Mapping pen:** suavização forte; estabilização 8; tamanho 100; ímã vetorial ligado.
- **Real G-Pen:** suavização forte; estabilização 2; tamanho 3.
- **Caneta rugosa:** estabilização 2; tamanho 19 px.

## Referências capturadas (`calibracao/csp/ref/`)

- Folha padrão em `tools/csp/csp_sheet.py`. Captura em `tools/csp/csp_ref.py`, no documento "Calibracao 900x700" a 100%, origem
  do canvas em (374,185) na tela, com o painel de material aberto.
  - O modo `--mix` faz faixas vermelhas (220,40,40) e traços azuis (40,70,220) por cima, na mesma camada.
  - Pressão constante (mouse). A velocidade entra pelo espaçamento dos pontos: 3, 7 e 15 px a cada 8 ms.
- **Caneta** (`ref/caneta`, tamanhos 3, 10 e 30): g-pen, real-g-pen, mapping-pen, turnip-pen, calligraphy, textured-pen,
  caneta-rugosa, milli-pen, felt-pen, dot-pen, flat-marker.
- **Lápis** (`ref/lapis`, tamanhos 3, 10 e 30): pencil, mechanical-pencil, charcoal, crayon, pastel, chalk.
- **Pincel** (`ref/pincel`, tamanhos 10, 30 e 60, mais mistura no 30):
  - nanquim: brush-pen, dry-ink, watery-ink, wet-blotting-ink;
  - aquarela: round-watercolor, flat-watercolor, soft-bleed, wet-wash, rough-wash, watercolor-splash, watery-brush.

## Interface do Clip Studio (capturas 30–37)

- **Prateleira** (com atalho): Zoom (;), Mover, Operação, Mover camada (K), Área de seleção (M), Seleção automática (W),
  Conta-gotas (I), Caneta e Lápis (P), Pincel, Aerógrafo e Decoração (B), Borracha (E), Mistura e Liquefazer (J), Balde e Degradê
  (G), Figura, Borda de quadro e Régua (U), Texto e Balão (T), Corrigir linha (Y). A mesma letra alterna entre as ferramentas.
- **Barra de comandos:** Novo, Abrir, Salvar, Desfazer, Refazer, Excluir, Excluir seleção externa, Preencher, Redimensionar/girar,
  Cancelar seleção, Inverter seleção, Exibir borda da seleção, Ajustar às réguas / às réguas especiais / à grade.
- **Configurações avançadas da ferramenta:** categorias Tamanho, Tinta, Jitter de cor, Suavização, Forma, Ponta, Spray, Traçar,
  Textura, pincel duplo (2-…), Borda de aquarela, Apagar, Correção, Início e fim, Anti-sobrefluxo. Um ícone de olho em cada
  parâmetro decide se ele aparece no painel compacto.
- **Cores em abas de ícone:** roda, conjunto de cores, controles RGB/HSV, mistura de cores, histórico, cor aproximada, cor
  intermediária.
- **Direita:** Navegador; Propriedades da camada; abas Camada | Histórico | Ação automática.
- **Barra de materiais:** a coluna de ícones entre o canvas e a direita abre cada paleta de material (Basic, Monochromatic
  pattern…); clicar de novo recolhe e o canvas ganha o espaço.
- **Menu Filtro:** Filtros rápidos, Efeitos rápidos, Correção, Desfoque, Distorcer, Efeito, Renderizar, Tornar nítido.
- **Pixel pen e ferramentas sem tamanho:** a paleta de tamanhos fica desabilitada, não some.

## Lotes despachados

| Lote | Quem | Escopo |
|---|---|---|
| Calibração das canetas | agente Opus | leitor do banco, comparador numérico, motor (entrada/saída por velocidade, estabilização, suavização, ponta chata, bordas rugosas, pixel), predefinições com `group` |
| Layout esquerdo | agente Opus | prateleira de ícones e grupos, sub-ferramentas com miniatura e ícones, propriedades compactas e detalhe com olho, paleta de tamanhos, cores em abas |
| Layout direito e topo | agente Opus | Navegador com rotação, Camadas com ícones, Histórico, barra de materiais, barra de comandos, barra do canvas sem texto, barra flutuante de seleção, acesso aos filtros |

## Referências capturadas — parte 2

- **Pincel / pintura grossa** (`ref/pincel`, 10/30/60 + mistura no 30): round-mixing, gouache, dry-gouache, thin-gouache,
  oil-paint, thick-oil-paint, pointillism, paint-and-apply.
  - gouache-blender só espalha a cor que já existe: está em `ref/mistura`.
- **Aerógrafo** (`ref/aerografo`, 10/30/60): soft, spray, droplet, noise, running-color-spray.
- **Borracha** (`ref/borracha`, 10/30/60, sobre a camada preenchida de preto com `--fill-base`): hard, soft-eraser, kneaded,
  rough-eraser, vector-eraser, multiple-layers, snap-eraser, borracha-dinamica.
- **Mistura** (`ref/mistura`, 30, por cima da grade vermelha/azul feita com a G-pen 30 por `--grid-base`; base em `base_s30`):
  blend, blur, finger-tip, painterly-blender, wet-bleed-blender, textured-blender, watery-brush, gouache-blender.
  - Copy stamp fica de fora: precisa de Alt+clique na origem; testar à mão.
- **Decoração** (`ref/decoracao`, 30/60; só o que serve para anime):
  - Effect: sparkle, bloodstain, dappled-light, glitter, soft-circle, feather, petal, rose, cherry.
  - Background: leaves, leaves-2, trees, grassland, grass-patch, fallen-leaves, ground, crack, cloud, rubble, book, building.
  - Clothing: lace, simple-frill, braid, braid-2, button, zipper, chain, bead-chain.
  - Hatching e Ruled (retícula e linhas de mangá) ficam de fora.
  - Esses pincéis usam imagens de material da CELSYS: no nosso app, padrões próprios com o mesmo comportamento (espalhar,
    girar, seguir o traço, colorir a ponta).
- Pranchas para ver tudo de uma vez: `calibracao/csp/27_canetas_s10.png`, `38_ink.png`, `39_aquarela.png`,
  `40_pintura_grossa.png`, `41_aerografo.png`, `42_borracha.png`, `43_mistura.png`, `44_decoracao.png`.

## Ferramentas funcionais do Clip Studio (captura 45) × o que temos

| Ferramenta | Sub-ferramentas e opções no Clip Studio | Falta no nosso (a confirmar no lote) |
|---|---|---|
| Área de seleção | retângulo, elipse, laço, polilinha, caneta de seleção, apagar seleção, encolher seleção, laço magnético; modo novo/somar/subtrair/interseção; suavização | polilinha, caneta e borracha de seleção, encolher seleção, laço magnético, modos em ícone |
| Seleção automática | só na camada de edição / todas as camadas / camadas de referência; só pixels conectados, fechar intervalo, tolerância, expandir área | camadas de referência, fechar intervalo, expandir área |
| Conta-gotas | cor exibida / cor da camada | cor da camada |
| Balde | só na camada / outras camadas / cercar e preencher / pintar área não preenchida (+ laço de preenchimento); cor alvo, fechar intervalo, tolerância, expandir, camadas múltiplas | cercar e preencher, pintar área não preenchida, laço de preenchimento, fechar intervalo, expandir |
| Degradê | predefinições como sub-ferramentas (céus, arco-íris…), degradê de forma livre; editor, ajustar ângulo, onde criar (camada ou camada de degradê editável) | degradê de forma livre, camada de degradê editável |
| Figura | linha, curva, polilinha, curva contínua, Bézier, retângulo, elipse, polígono, laço de preenchimento; linhas de velocidade, linhas de foco | a conferir: figuras diretas, linhas de velocidade e de foco |
| Régua | linear, curva, figura, caneta de régua, especial, guia, perspectiva, simétrica | a conferir: curva, figura, caneta de régua, especial, simétrica |
| Texto | texto, texto circular | texto circular |
| Corrigir linha | ponto de controle, puxar linha, simplificar, conectar, ajustar largura, redesenhar linha e largura; remover partículas, preencher restante, selecionar poeira | a conferir com os nossos pontos vetoriais; remover partículas |
| Operação | objeto, selecionar camada, mesa de luz, editar linha do tempo | selecionar camada clicando no desenho |
| Mover | mão, girar, zoom | girar como sub-ferramenta (R) |

Próximo lote (depois dos lotes de layout, para não mexer nos mesmos arquivos): ferramentas funcionais que faltam.

## Preferências do Clip Studio (capturas `pref_00`–`pref_16`, pranchas `46_prefs_1/2`)

As 17 categorias, com o que interessa a cada uma:

- **Ferramentas:** troca temporária de ferramenta segurando o atalho (tempo em ms), distância mínima de arrasto, rolagem na
  borda, tamanho da alça, ajuste do tamanho do pincel arrastando na tela, prévia de linha com Shift.
- **Tablet:** Wintab ou Tablet PC, modo mouse do driver, área de operação, frequência de entrada.
- **Gestos de toque:** um dedo, dois dedos (mover, pinça, girar, toque = desfazer), três dedos = refazer.
- **Comando:** transformar (alvo e confirmação com duplo clique), colar SVG como vetor.
- **Interface:** IME, cor de interface (escuro/claro + densidade), interface para toque, layout tablet.
- **Desempenho:** memória (alocação %), níveis de desfazer (200) e intervalo, unidade de processamento de IA (GPU).
- **Cursor:** forma por tipo de ferramenta (pincel, caneta, borracha, figura, balde, visualização, correção); o que mostrar
  quando o pincel é pequeno; posição do cursor com estabilização.
- **Camada/quadro:** nome das cópias, modo atravessar para pastas novas, opacidade da seleção e do caminho vetorial, cor e
  opacidade da máscara, espaços da borda de quadro.
- **Animação:** colar em célula, criar e remover célula com camada, estender clipe, vincular operações, mesa de luz
  (opacidade, cor da camada e subcor).
- **As demais:** Régua/Unidade, Tela de pintura, Arquivo, Conversão de cores, Editor de história, 3D, Conexão com smartphone e
  Tabmate 2 (capturas `pref_09` a `pref_16`).

Lote futuro: comparar com as nossas Preferências e acrescentar o que faltar e servir para anime.

## Levantamento (preenchido grupo a grupo)

| Grupo | Sub-ferramenta do Clip Studio | O que faz / resultado | O que temos | Ação |
|---|---|---|---|---|
| Caneta | G-pen, Real G-Pen, Mapping pen, Turnip pen, Calligraphy, Textured pen… | (a medir) | Pincel raster / Caneta vetorial | (a definir) |

## Pendências para a integração (achadas pelos agentes)

- A barra flutuante de seleção (`sel_ops.rs`, Order::Foreground) desenha por cima de janelas como Preferências.
- `unique_layer_name` sempre acrescenta " 1" (corrigido só em Copiar para camada).
- Pincel duplicado no painel de sub-ferramentas ainda vira "Nome 2" e não segue o estilo de nome de cópia das Preferências.
- A aba Propriedades da direita ainda repete opções da ferramenta.
- Painéis da esquerda ficam apertados em 1080p (propriedades mostram cerca de 4 linhas, paleta de tamanhos cerca de 2).
- Coluna direita da Animação lotada em 1080p (Navegador pequeno).
- Calibração das canetas: Caligrafia 10 px cerca de 1,2× larga; Caneta rugosa (sobreposição 0,73); Feltro 3 px rápido fino demais.
- Fora do escopo das Preferências por enquanto: rolagem na borda, prévia de linha com Shift, cor das réguas.
