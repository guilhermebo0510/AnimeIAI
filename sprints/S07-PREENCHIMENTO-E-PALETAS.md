# S07 — Preenchimento, tinta-e-pinta, paletas indexadas

## Objetivo
Pintar cels como no shiage: balde que fecha buracos e respeita linhas de outra camada, paleta indexada com troca global de cor, pintar atrás da linha, lint de regiões não pintadas, sombras e realces de cel.

## Pesquisa obrigatória
Krita "Close Gaps" (algoritmo, GPL: só ler), CSP "Close gap" e "Refer multiple", LazyBrush (Sýkora et al.), OpenToonz tinta-e-pinta e paleta (BSD-3), RETAS PaintMan (fluxo de cor), Harmony "paint behind".

## Tarefas
- [ ] Balde raster e vetorial: região por cor ou por fronteira de linha, tolerância, **fechamento de gap** (dilatação de linha + preenchimento + expansão de volta, ou distância geodésica), "referir camadas" (linha numa camada, preenchimento noutra), expansão para baixo da linha (sem halo branco), preencher sem AA.
- [ ] Camada de preenchimento ligada à camada de linha do mesmo cel; regiões guardadas com índice de paleta (recolorir não exige repintar).
- [ ] Paleta: cores nomeadas, grupos por personagem/parte (normal, sombra 1, sombra 2, realce, linha), dia/noite como variantes; trocar a cor de um índice atualiza todo o projeto.
- [ ] Linhas de cor (色トレス): traços vetoriais com cor de índice que delimitam sombra/realce e somem ou viram cor da região no render final, conforme ajuste.
- [ ] Paint behind; preencher todas as regiões fechadas com cor de "falta" para checagem.
- [ ] Lint: regiões não pintadas, pinceladas fora de linha, buracos de gap acima da tolerância, cores fora da paleta; relatório com localização.
- [ ] Propagação (P1): preencher a mesma região em cels seguintes por correspondência (centróide + forma).
- [ ] MCP: `aiai_fill` (ponto ou lista de pontos, índice, tolerância, gap, camadas de referência), `aiai_palette` (criar/editar índices, variantes), `aiai_lint`.

## Testes
- Golden: personagem simples com gaps de 1–6 px; linha com AA vs. sem AA; paint behind; troca de cor global.
- Lint encontra 100% das regiões deixadas propositalmente sem cor num caso de teste.
- Mouse: clicar com o balde em várias regiões; trocar cor na paleta e ver o canvas atualizar.

## Critério de pronto
- [ ] Prova de uso: o busto da S05 pintado por MCP com settei (pele, cabelo, olhos, roupa; normal + sombra + realce), sombras delimitadas por linhas de cor, lint = 0; depois troca da cor do cabelo por índice num único comando.
- [ ] Recorte comparado com um personagem do episódio (paleta extraída pela S04): ΔE das cores ≤ 3 quando o settei usa as cores medidas.
