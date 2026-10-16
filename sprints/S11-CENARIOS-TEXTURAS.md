# S11 — Cenários: texturas, pincéis especiais, BOOK/parallax, céu, luz

## Objetivo
Pintar fundos de anime (美術) com qualidade: biblioteca de texturas procedurais e importadas, pincéis de cenário, padrões mapeados em perspectiva, céu, luz e sombra pintadas, organização em camadas para parallax (BG, BOOK, sobreposições) e fluxo 3D em blocos → linha → pintura.

## Pesquisa obrigatória
Fluxo de bijutsu (fundos pintados no Photoshop, guache digital), tutoriais de pintura de fundo de anime (céu, nuvens cumulus, folhagem, arquitetura, luz de janela), texturas procedurais (Perlin/simplex/Worley, fBm, domain warping), Vanishing Point do Photoshop (homografia), uso de 3D→pintura em estúdios.

## Tarefas
- [ ] `TextureAsset`: procedural (gerador + parâmetros + semente, renderizado em qualquer resolução, tileável) e raster (importado ou pintado no app); biblioteca inicial: papel, tela, ruído (Perlin, simplex, Worley, fBm), aquarela/manchas, nuvem, granulado, pedra, mármore, madeira, tijolo, telha, reboco, grama, folhagem, tecido.
- [ ] Usos: overlay de camada (mistura + opacidade + máscara), preenchimento de padrão (escala, rotação, deslocamento, tile), textura de pincel (S06), papel do canvas.
- [ ] Padrão em perspectiva: aplicar textura num plano definido pela régua de perspectiva ou por 4 pontos (homografia), com máscara.
- [ ] Pincéis de cenário (presets com ponta por imagem desenhada no app + dinâmicas): folhagem (3 tipos), copa de árvore, grama, flores/rosas, nuvem (macia + borda definida), pedra/tijolo, cerca/grade, brilho de água, estrelas, poeira de luz.
- [ ] Céu: gerador de degradê de céu por hora do dia + nuvens procedurais (cumulus estilizado com borda) + pincel de nuvem para acabamento.
- [ ] Luz: camadas de luz/sombra pintadas (multiplicar/adicionar/sobrepor), raios de luz, gradiente de iluminação ambiente, reflexo de janela.
- [ ] Organização de BG: camadas BG (fundo), BOOK (sobreposição que fica na frente do personagem, ex.: mesa, folhagem), com profundidade para o multiplano (S13), cenário maior que o quadro para pan.
- [ ] Fluxo 3D: blocos da S09 → render de linhas → camada de layout → pintura raster por cima → descartar a guia no render final.
- [ ] MCP: `aiai_texture` (criar procedural/importar), `aiai_pattern_fill` (inclusive perspectiva), `aiai_sky`, `aiai_paint_stroke` com presets de cenário, `aiai_bg_layers`.

## Testes
- Golden: cada textura procedural com semente fixa (determinismo), tile sem emenda, padrão em perspectiva de piso.
- Desempenho: cenário 4K×2K com 20 camadas navegável.
- Mouse: escolher textura na biblioteca e arrastar para uma camada; pintar folhagem por arrasto.

## Critério de pronto
- [ ] Prova de uso: o agente pinta via MCP um fundo externo completo (céu com nuvens, prédio simples em perspectiva de 2 pontos com textura de tijolo/telha mapeada, árvores e arbustos com pincéis de folhagem, canteiro de flores, luz de fim de tarde), com BG + BOOK separados; render em evidências.
- [ ] Linha de base de métrica contra um cenário do episódio (S04), sem exigir aprovação ainda.
