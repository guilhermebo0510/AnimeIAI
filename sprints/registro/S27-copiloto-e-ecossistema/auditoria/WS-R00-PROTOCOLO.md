# WS-R00 — Protocolo da auditoria dos workspaces (fase 2)

> Host: Claude (o único que implementa). Revisor: Codex (ChatGPT). Começa quando o Copiloto fechar em "NÍVEL
> PROFISSIONAL: SIM" (fase 1). Um workspace por rodada, ou dois pequenos juntos. Os arquivos ficam nesta pasta:
> `WS-Rnn-…` (pedido do Claude) e `WS-CXnn-…` (auditoria do Codex).

## Regras (iguais às do Copiloto)

- **Só projetos de teste próprios** do revisor, em `C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-*.aiai`. Nunca
  `producoes\`.
- **Não editar código, não rodar `git` que altere nada, não rodar `cargo`** (disco cheio). O app abre sem `--bridge`, e
  o revisor fecha só o PID dele.
- **Um só Copiloto ligado na GPU** (4 GB).
- **Rodada profunda** (R23 §2): dezenas de itens verificados, todos os achados de uma vez, cada um com gravidade
  (bloqueia, não bloqueia ou melhoria), prova (arquivo:linha ou passos no app) e correção esperada, mais a cobertura
  declarada por área. O revisor compara com o programa de referência e diz o que falta para o nível profissional.
- **Fim:** "NÍVEL PROFISSIONAL: SIM" do revisor e a auditoria interna do Claude concordando. O alvo é fechar em 3 a 5
  rodadas por workspace.

## O que auditar em todo workspace

1. **Ferramentas:** cada botão, menu, atalho e painel funciona? Usos cruzados com outros workspaces e ferramentas.
2. **Fluxo de trabalho de estúdio:** o caminho que um profissional faz, de ponta a ponta. Onde trava, onde perde dado,
   onde exige passos demais.
3. **Interface:** organização, nomes, dicas, estados vazios, tema claro/escuro, pt/en, janelas pequenas, ícones (sem
   emoji), consistência com o resto do app.
4. **Robustez:** desfazer/refazer, salvar/abrir, projeto grande, entradas inválidas, interromper no meio.
5. **Desempenho:** atrasos perceptíveis, com números.
6. **O que falta** comparado à referência abaixo, priorizado: o que é essencial para produzir anime.

## Ordem e referência de cada workspace

| # | Workspace | Referência | Foco de produção |
|---|---|---|---|
| 1 | Roteiro | Celtx, Final Draft, Fountain | roteiro → cenas → falas → ligação com storyboard e áudio |
| 2 | Personagens | folhas de modelo de estúdio | design sheet, paleta de settei, material ligado ao programa inteiro (base do personagem controlado da fase 4) |
| 3 | Storyboard | Storyboard Pro, Clip Studio | painéis, tempos, câmera, animatic |
| 4 | Layout | Clip Studio, Storyboard Pro | layout de corte, câmera, BG/cel, guias |
| 5 | Cenário | Clip Studio, Photoshop | BG, perspectiva, pintura de fundo |
| 6 | Animação | Clip Studio, TVPaint, OpenToonz | genga/douga, timesheet, mesa de luz, Copiloto, referência 3D |
| 7 | Pintura | RETAS PaintMan, OpenToonz | pintura por paleta, preencher, sombra e luz |
| 8 | Composição | After Effects | camadas, efeitos, chaves, partículas 2D/3D, câmera |
| 9 | Biblioteca | Clip Studio Assets | materiais, modelos, paletas, texturas |
| 10 | Modelagem | Blender, ANIGO (nosso) | modelar, riggar, posar, animar como referência para o 2D, câmera |
| 11 | Áudio | Audition, Reaper | falas (TTS), efeitos, trilha, mixagem |
| 12 | Edição | Premiere, DaVinci Resolve | montagem, legendas, transições |
| 13 | Entrega | Media Encoder | exportação, formatos, pacote de entrega |
| 14 | Produção | Shotgrid, ftrack | tarefas, cortes, retakes, gráficos |

## Estado

| # | Workspace | Rodadas | Veredito |
|---|---|---|---|
| 1–14 | — | — | aguardando a fase 1 |
