# S12 — Animação: timeline, timesheet, cels, onion skin, intercalação

## Objetivo
Animar como no anime: cels por camada, timesheet com exposição por referência (1s/2s/3s), timeline, onion skin, mesa de luz, papéis key/breakdown/inbetween, intercalação vetorial automática (estilo CACANi), keyframes de transformação com easing, ciclos, colunas de boca e olhos, playback em tempo real com cache e áudio.

## Pesquisa obrigatória
Timesheet japonês (colunas, 144 quadros por folha de 6 s, notação de exposição), OpenToonz xsheet (BSD-3), Toei timesheet/XDTS, CACANi (correspondência de traços, intercalação com controle de trajetória), papers de intercalação vetorial (stroke correspondence, "AnimeInbet" só como leitura — sem licença), Harmony (ciclos, deformação), gráfico de spacing do genga.

## Tarefas
- [ ] Timeline: trilhas = camadas animáveis; cels como blocos; arrastar para mudar exposição; cabeçote, zoom de tempo, marcadores, faixa de trabalho.
- [ ] Timesheet (widget próprio, denso): colunas A/B/C…/BG/BOOK/câmera/efeito/diálogo/boca, linhas = quadros, células com número do cel ou "mantém" (traço vertical); seleção de bloco, preencher em 2s/3s, inserir/remover quadros, copiar colunas; sincronizado com a timeline.
- [ ] Cels: criar, duplicar, renumerar, papel (key/breakdown/inbetween), nota de spacing (gráfico de espaçamento entre chaves).
- [ ] Onion skin (N antes/depois, cores, opacidade decrescente) e mesa de luz (escolher cels quaisquer como fundo).
- [ ] Intercalação vetorial: correspondência automática de traços entre duas chaves (por ordem, proximidade e forma) com edição manual da correspondência; interpolação de pontos e larguras; trajetória (linear, arco) e ease (gráfico de spacing); gera cels intermediários editáveis marcados como `tool:inbetween`.
- [ ] Keyframes de transformação por camada/grupo (posição, escala, rotação, opacidade) com editor de curvas simples (o graph editor completo vem na S13).
- [ ] Ciclos: repetir sequência de cels (piscar, caminhada, cabelo ao vento) com contagem.
- [ ] Coluna de boca: cels de boca do personagem (do model sheet) escolhidos por quadro, alimentados pelo lip-sync (S17); coluna de olhos com piscar automático configurável.
- [ ] Playback: cache de quadros renderizados (RAM com limite, disco opcional), tocar a 23,976 fps com áudio mestre (sincronia pelo relógio de áudio), scrub com áudio, loop na faixa.
- [ ] MCP: `aiai_cel` (criar/duplicar/papel), `aiai_exposure` (definir células do timesheet em lote, ex.: "A1–A4 em 2s a partir do quadro 0"), `aiai_inbetween` (chaves, quantidade, ease, correspondência opcional), `aiai_keyframe`, `aiai_cycle`, `aiai_playback` (render de faixa em vídeo de prévia).

## Testes
- Unitários: notação de exposição ↔ células; 0-based interno vs. exibição 1-based.
- Golden: intercalação de traço simples (linha que gira), de rosto virando 15° com correspondência editada.
- Playback: 3 camadas + BG em 1080p sem perder quadro (medir), áudio sincronizado (desvio < 1 quadro em 60 s).
- Mouse: arrastar exposição na timeline, preencher bloco no timesheet, tocar/pausar com espaço.

## Critério de pronto
- [ ] Prova de uso: o agente anima via MCP o personagem de teste (S10) virando a cabeça e piscando em 2 s (2 chaves desenhadas, intercalação + correções, piscar por ciclo, boca falando uma frase de teste por coluna de boca), pintado; prévia MP4 em evidências.
- [ ] Mapa de exposição do resultado lido de volta e conferido com o timesheet.
