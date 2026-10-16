# S06 — Pincéis raster, pintura e texturas de pincel (referência: Photoshop, CSP)

## Objetivo
Motor de pincel raster por dabs com a expressividade do Photoshop (ponta, dinâmica, textura, pincel duplo, mistura), rápido na GPU e determinístico na CPU, com presets e ferramentas de pintura.

## Pesquisa obrigatória
Documentação de pincéis do Photoshop (Brush Settings: Shape Dynamics, Scattering, Texture, Dual Brush, Color Dynamics, Transfer, Wet Edges, Build-up, Smoothing), modelo de dab do libmypaint (ISC, pode ler e reaproveitar ideias), artigos sobre pincel em GPU (stamping em compute, acumulação), aquarela digital (bordas escuras, "wet edges").

## Tarefas
- [ ] Ponta: procedural (diâmetro, dureza, redondeza, ângulo) e por imagem (máscara em escala de cinza desenhada no app ou importada).
- [ ] Dinâmicas: espaçamento, curvas de pressão/velocidade/inclinação → tamanho, opacidade, fluxo, ângulo, redondeza; dispersão (eixos, quantidade); jitter de cor (matiz, saturação, brilho, frente/fundo) com RNG semeado.
- [ ] Textura de pincel (modo, escala, profundidade, por dab ou por traço) e pincel duplo.
- [ ] Mistura: borrar, misturar com a cor da tela, aquarela (borda escura, acumulação), aerógrafo (acúmulo por tempo).
- [ ] Ferramentas: pincel, lápis (sem AA), aerógrafo, borracha (macia/dura), borrar, degradê (linear, radial, angular, ruído) com editor de paradas, balde raster simples (a versão completa vem na S07).
- [ ] Execução GPU (dabs em lote por tile) e CPU (mesma ordem, mesmo resultado dentro da tolerância).
- [ ] Undo por traço com os tiles afetados guardados (decidido na S01).
- [ ] Presets de pincel no projeto (JSON + máscaras em blob); conjunto inicial: lápis de rascunho, pincel redondo duro, macio, aerógrafo, aquarela, guache seco, carvão, pincel de textura de papel, borrador.
- [ ] Editor de pincel (UI) com prévia ao vivo do traço.
- [ ] MCP: `aiai_paint_stroke` (pontos/curva + preset + parâmetros sobrescritos + pressão por ponto), `aiai_brush_preset` (criar/editar), `aiai_gradient`.

## Testes
- Golden por dinâmica (um traço por parâmetro), aquarela, pincel duplo, textura.
- GPU×CPU FLIP ≤ 0,02.
- Desempenho: traço de 2000 dabs em 1080p sem atraso perceptível (< 16 ms por quadro de entrada).
- Mouse: pintar com arrasto real em 3 presets; ajustar tamanho por atalho ([ e ]).

## Critério de pronto
- [ ] Prova de uso: o agente pinta via MCP um estudo de céu com nuvens (degradê + pincel macio + aquarela) e uma parede com textura de reboco; render em evidências.
- [ ] Comparação visual de um recorte de céu do episódio com o estudo (métricas S04 registradas como linha de base, sem exigir aprovação ainda).
