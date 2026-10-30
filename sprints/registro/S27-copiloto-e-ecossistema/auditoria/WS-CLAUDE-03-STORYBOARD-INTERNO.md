# WS-CLAUDE-03 — Storyboard: auditoria interna (leitura de código, antes da rodada com o Codex)

> 30/10 de manhã (ChatGPT no limite; sem app e sem GPU). Base `ec0a63a`. Arquivo lido: `aiai-studio/src/app/panels/board.rs`
> (358 linhas). Falta `shots.rs` (lista de planos e propriedades do painel) e o MCP `aiai_storyboard`/`aiai_animatic`.

| Id | Gravidade | Achado | Prova | Correção esperada |
|---|---|---|---|---|
| I-S01 | bloqueia (fluxo) | Desenhar um painel **tira o artista do Storyboard**: duplo clique abre o corte do painel no workspace Layout. No Storyboard Pro e no Clip Studio se desenha no próprio painel, passando de um para o outro sem trocar de tela. | `open_drawing` → `want_workspace = Layout`; `cell` (duplo clique) | Desenhar no painel dentro do Storyboard (canvas do painel selecionado ao lado da grade), com próximo e anterior por atalho. |
| I-S02 | não bloqueia | O animatic é gravado sempre em `saida/animatic.mp4` e a folha de e-conte em `saida/econte.png`, **substituindo** o anterior sem perguntar. Não há tocar o animatic no app (com o som). | `make_animatic`, `make_sheet` | Nome com versão (ou perguntar), e um player do animatic no workspace (com o áudio das falas). |
| I-S03 | não bloqueia | A folha de e-conte sai em **um PNG**: um storyboard longo vira uma imagem enorme ou ilegível; o padrão é PDF com várias páginas (5 painéis por página, colunas de corte, ação, fala, tempo). | `make_sheet` → `STORYBOARD sheet` | PDF paginado no formato de e-conte japonês. |
| I-S04 | não bloqueia | "Gerar cortes" usa sempre o **primeiro episódio** do projeto. | `make_cuts` (`roots.episodes.first()`) | Escolher o episódio (o do roteiro selecionado, por padrão). |
| I-S05 | não bloqueia | Na grade não se reordena painel arrastando nem se divide/junta painel; o movimento de câmera do painel (pan, zoom, PAN/T.U./T.B.) não aparece na miniatura. | `show` (grade só com clique e duplo clique) | Arrastar para reordenar; marcas de câmera (setas e quadros de início e fim) na miniatura. |
| I-S06 | melhoria | A miniatura só mostra o número do corte, o tipo de plano, os segundos e a ação. Faltam a fala e o SE (estão na folha). | `cell` | Fala e SE no cartão (resumidos). |

**Para a rodada WS-R03:** conferir no app, ler `shots.rs` e o MCP, e comparar com o Storyboard Pro (camadas por painel,
câmera, transições, animatic com som).
