# ADR-003 — Motor de desenho, pintura e texturas

Status: aceito como direção; técnicas finais escolhidas nas S03–S07 com medição.

## 1. Por que vetor **e** raster

Em anime, as partes animáveis (personagens, props que se mexem) são desenhadas com **pincel vetorial**: linha limpa, espessura editável depois, correção de traço, troca de cor de linha, fechamento confiável para o balde e intercalação. O Clip Studio é a referência. Cenário, texturas, sombras suaves e pintura de BG são **raster**, com pincéis especiais e texturas, com o Photoshop como referência. O motor tem os dois, na mesma árvore de camadas, com o mesmo compositor.

## 2. Vetor

- **Stroke** = sequência de amostras `{x, y, pressão, inclinação, tempo}` → curva suavizada (ajuste de Bézier com tolerância) + **perfil de largura** (largura por parâmetro ao longo da curva) + ponta (redonda, afinada, "G-pen", "maru-pen", marcador) + cor por índice de paleta + opacidade.
- Renderização: tesselação do contorno com largura variável (offset de curva com junções/pontas corretas) em triângulos para GPU, e rasterização com cobertura analítica na CPU de referência. Opção: `vello`/`vello_cpu`. Decidir na S05 comparando qualidade de borda, velocidade e determinismo.
- Antisserrilhado configurável: **sem AA para linha de cel** (cor chapada, compatível com balde e com "seleção por cor" do satsuei) ou com AA para arte final.
- Edição: pontos de controle, mover/afinar/engrossar trecho, "corrigir espessura", conectar traços, apagador vetorial (traço inteiro, até interseção, área), simplificar, suavizar.
- Estabilização (média móvel/"lazy brush") e pós-correção para traço vindo do mouse/caneta.
- **Formas**: linha, polilinha, curva (Bézier, spline Catmull-Rom com pontos de passagem — essencial para o agente), elipse, retângulo, polígono, arco.
- **Comandos MCP de alto nível** (o agente desenha com eles): `stroke` com pontos e pressões; `curve` por pontos de passagem com tensão e perfil de largura (início/meio/fim); `shape`; `symmetry`; `stroke_edit`.

## 3. Raster

- Camadas em tiles de 256×256 (esparsas; tile vazio não existe). Trabalho em float16 linear na GPU; CPU de referência em f32.
- **Motor de pincel por carimbos ("dabs")**: forma da ponta (procedural: dureza, redondeza, ângulo; ou imagem), espaçamento, curvas de pressão → tamanho/opacidade/fluxo/cor, dispersão, jitter (RNG determinístico), rotação pela direção, **textura de pincel** (modulação por textura em espaço de tela ou de traço), **pincel duplo**, mistura de cor (smudge, misturar com a cor da tela, aquarela com borda escurecida), aerógrafo (acumulação temporal).
- Modos: normal, apagar, atrás ("paint behind"), multiplicar, tela, sobrepor, adicionar, cor, luminosidade (os mesmos do compositor).
- Ferramentas: pincel, lápis, aerógrafo, borracha, borrar, misturar, degradê (linear, radial, ruído), balde raster, carimbo de padrão.

## 4. Texturas e pincéis especiais (cenário)

- **Biblioteca de texturas**: procedurais (papel, tela, ruído Perlin/simplex/Worley, granulado, aquarela, nuvem, pedra, madeira, tijolo, folhagem, grama), desenhadas no próprio app e importadas (imagens CC0 permitidas como textura de material, nunca como cena pronta).
- **Pincéis de cenário**: folhagem (dispersão de formas de folha com variação de cor), nuvem (macia + borda), grama, tijolo/pedra (padrão em perspectiva), árvore (galhos), flor/rosa (forma + dispersão), água/brilho, poeira.
- **Uso da textura**: como overlay de camada (modo de mistura + máscara), como preenchimento de padrão (com escala/rotação/perspectiva), como textura do pincel, como papel do canvas.
- **Mapeamento em perspectiva**: aplicar padrão (piso, parede, tijolo) seguindo a régua de perspectiva da cena (homografia).

## 5. Preenchimento (tinta e pinta)

- Balde por região com **fechamento de gap** (tolerância em pixels; ver Krita "Close Gaps" e literatura "LazyBrush"), referência a outras camadas (linha numa camada, cor noutra), expansão sob a linha, preenchimento por índice de paleta.
- **Paleta indexada**: trocar a cor do índice atualiza todas as regiões (settei de cor normal/sombra/realce por personagem).
- **Lint**: achar regiões não pintadas e pequenas lacunas antes do render.
- Sombra e realce de cel: regiões desenhadas com linha de cor (色トレス) e preenchidas com a cor de sombra do settei.

## 6. Render e determinismo

- Um grafo de composição, dois executores: GPU (wgpu, interativo) e CPU (referência, golden). Diferença GPU×CPU medida por FLIP com tolerância por backend.
- O agente avalia sempre o render CPU (determinístico) para métricas, e usa a captura do app para validar a interface.

## 7. Referências para engenharia reversa (ler, não copiar sem licença)

Clip Studio Paint (manual público: pincel vetorial, borracha vetorial, réguas, preenchimento com fechamento), Photoshop (motor de pincel, texturas, pincel duplo, modos), Krita (GPL, só leitura: brush engines, close gaps), MyPaint/libmypaint (ISC: modelo de dab), OpenToonz (BSD-3: tinta-e-pinta, paleta indireta), Graphite (Apache-2.0: Rust + wgpu + vetor), Vello/kurbo (Apache/MIT), papers de fechamento de gap e intercalação (lista em `anime-studio-pesquisa/pesquisa-bruta/A5`).
