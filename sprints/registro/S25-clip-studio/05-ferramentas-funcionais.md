# S25 — Ferramentas funcionais à moda do Clip Studio

O agente que fez este lote parou no limite de uso antes de escrever o relatório. Este resumo foi montado a partir dos
commits e das capturas em `evidencias/funcionais/`.

| Ferramenta | Clip Studio | Feito no AnimeIAI | Commits |
|---|---|---|---|
| Área de seleção | retângulo, elipse, laço, polilinha, caneta de seleção, apagar seleção, encolher seleção, laço magnético; modos em ícone | caneta e borracha de seleção, encolher seleção (o laço encolhe até o contorno do desenho), laço magnético com prévia; modos novo/somar/subtrair/intersectar em ícones | 936dee0, cdc0668 (`aiai-paint::area`) |
| Seleção automática | só a camada / todas / camadas de referência; fechar intervalo, tolerância, dimensionar área | as três sub-ferramentas, fechar intervalo e dimensionamento de área, refazendo o último clique | 936dee0 |
| Conta-gotas | cor exibida / cor da camada | as duas | 936dee0 |
| Balde | só a camada / outras camadas / cercar e preencher / pintar área não preenchida / laço de preenchimento | as cinco, respeitando a seleção; ação `FILL_AREA` na bridge e `aiai_fill_area` no MCP | 931023a, 936dee0 |
| Degradê | predefinições como sub-ferramentas, degradê livre, onde criar | degradê por arrasto, camada de degradê editável, degradê livre (campo de vários pontos), "Onde criar" em botões; `aiai_figure` | cdc0668, 20283fc, 1d3f2b2 |
| Figura | linha, curva, polilinha, curva contínua, Bézier, retângulo, elipse, polígono, laço de preenchimento; linhas de velocidade e de foco | desenho direto completo, linhas de velocidade e de foco como ferramenta (refazem só a camada do mesmo tipo) | cdc0668, 1d3f2b2 |
| Régua | linear, curva, figura, caneta de régua, especial, guia, perspectiva, simétrica | linear, curva, figura (retângulo, elipse, polígono), caneta de régua, círculos concêntricos e guia, com encaixe medido no Clip Studio (40 px); os 15 tipos na prateleira | ca6e168, de3ffb8 |
| Corrigir linha | ponto de controle, puxar, simplificar, conectar, ajustar largura, redesenhar linha e largura; remover poeira | todas, com prévia ao vivo, no app, na bridge (`LINE_FIX`) e no MCP (`aiai_line_fix`) | 7e54e65, d61b94e, 1104dd7 |
| Mover / Operação | mão, girar, zoom; selecionar camada | Girar (o arrasto gira a visão) e Selecionar camada (o clique escolhe a camada desenhada sob o cursor) | 936dee0 |
| Texto | texto, texto circular | texto circular (o arrasto dá centro e raio) | 936dee0 |

## Capturas

- `figura_*`: linhas, formas, velocidade/foco.
- `degrade_*`: céu noturno, seleção, camada editável, ponta movida, livre.
- `regua_*`: réguas criadas, encaixe da caneta e do pincel, encaixe medido no Clip Studio.

## Ficou para depois

- Régua simétrica e régua especial paralela/radial: a prateleira lista os tipos, mas o encaixe foi conferido só nos
  tipos do commit ca6e168.
- Conferência com mouse real das sub-ferramentas de seleção e do balde ao lado do Clip Studio: os testes cobrem as
  máscaras e as regiões, e falta a captura lado a lado.
