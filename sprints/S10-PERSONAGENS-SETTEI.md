# S10 — Personagens: model sheet, turnaround, expressões, settei de cor

## Objetivo
Ferramentas e fluxo para criar o design de personagem como um estúdio: ficha, model sheet com grade de proporção, turnaround em 5 vistas guiado pelo manequim, folha de expressões, bocas e olhos, settei de cor nomeado, e biblioteca de partes reutilizáveis que alimenta a animação.

## Tarefas
- [ ] Asset Character: ficha (nome, idade, altura, personalidade, voz), model sheet (documento de várias vistas em um canvas próprio), expressões, bocas, olhos, mãos, props.
- [ ] Template de model sheet: grade de cabeças com linhas horizontais compartilhadas (topo da cabeça, olhos, queixo, ombros, cintura, joelhos, pés) entre as vistas; proporção configurável (realista 7–8, anime 6–7, chibi 2–3).
- [ ] Turnaround guiado: manequim (S09) posado e ajustado às proporções do personagem em frente/3/4/perfil/3/4 costas/costas; render de guia em cada vista.
- [ ] Folhas: expressões (neutra, alegre, triste, raiva, surpresa, vergonha…), bocas (A, I, U, E, O, fechada, N), olhos (aberto, meio, fechado, piscada), com a mesma cabeça base duplicada e editada.
- [ ] Settei de cor: tabela de cores nomeadas por parte com normal/sombra/realce/linha e variantes de iluminação (dia, entardecer, noite); amostras visuais; exportar folha de settei como imagem.
- [ ] Partes reutilizáveis: marcar grupos de traços/camadas do model sheet como "parte" (cabeça 3/4, olho direito, mão aberta…) → inserir num cel com transformação/warp, mantendo vínculo de origem.
- [ ] Medidor de consistência: compara um desenho com o model sheet (altura em cabeças, posição dos olhos, largura de ombros) → erro percentual (usado no protocolo N2/N3).
- [ ] Workspace Personagens com painéis: lista de personagens, ficha, model sheet, settei, partes.
- [ ] MCP: `aiai_character` (criar/editar ficha), `aiai_model_sheet` (criar template, vistas), `aiai_part` (marcar/inserir), `aiai_settei` (cores), `aiai_consistency_check`.

## Testes
- Unitários do medidor de consistência com casos sintéticos.
- Mouse: criar personagem pela UI, mudar proporção no template, inserir parte num cel por arrastar e soltar.

## Critério de pronto
- [ ] Prova de uso: o agente cria um personagem original de teste completo (ficha, turnaround 5 vistas em vetor, 6 expressões, 7 bocas, 4 olhos, settei de cor pintado com balde), tudo via MCP; folha de model sheet e de settei renderizadas em evidências.
- [ ] Consistência entre vistas medida: erro ≤ 5% nas linhas de altura.
