# S08 — Seleção, transformação, deformação, ajustes

## Objetivo
Ferramentas para corrigir e reaproveitar desenhos: selecionar, transformar (inclusive perspectiva e malha), liquify, copiar entre cels/cortes, máscaras e camadas de ajuste. Para o agente, é o que permite ajustar uma parte desenhada até casar com o alvo sem redesenhar.

## Tarefas
- [ ] Seleções: retângulo, elipse, laço livre, laço poligonal, varinha mágica (tolerância, contígua, referir camadas), faixa de cor; somar/subtrair/intersectar; expandir/contrair/suavizar borda; seleção a partir de camada/região de paleta; salvar seleção.
- [ ] Máscara de camada (raster), máscara vetorial, clipping (já no compositor) com UI.
- [ ] Transformar livre: mover, escala, rotação, cisalhamento, perspectiva (4 cantos), espelhar; pivô; interpolação bicúbica/Lanczos para raster; vetor transformado sem perda (pontos e larguras, opção de escalar largura).
- [ ] Deformação por malha (grade N×M com Bézier) e liquify (empurrar, inchar, encolher, girar) para raster e vetor.
- [ ] Copiar/colar/duplicar entre camadas, cels, cortes e projetos; colar no lugar.
- [ ] Repetição: duplicar em grade, em arranjo radial e **ao longo de curva** (com escala/rotação seguindo a curva e a régua de perspectiva) — essencial para arquitetura ornamentada (colunas, balaústres, janelas, telhas) e padrões de roupa (babados, rendas) sem desenhar cada elemento.
- [ ] Camadas de ajuste: níveis, curvas, matiz/saturação/luminosidade, equilíbrio de cor, brilho/contraste, mapa de degradê, inverter, limiar, posterizar.
- [ ] MCP: `aiai_select`, `aiai_transform` (matriz ou parâmetros, perspectiva por 4 pontos), `aiai_warp` (malha com pontos de controle), `aiai_liquify`, `aiai_adjustment`.

## Testes
- Golden: transformação de raster em 6 casos; vetor escalado 3× sem serrilhado; warp de malha; liquify; cada ajuste.
- Mouse: laço desenhado por arrasto real, transformar arrastando alças, confirmar com Enter.

## Critério de pronto
- [ ] Prova de uso: pegar a cabeça do busto (S05–S07), duplicar, girar levemente e deformar por malha para uma pose 3/4 aproximada; render antes/depois.
- [ ] Varinha mágica com referência a camada de linha selecionando exatamente uma região de cel (comparação de máscara).
