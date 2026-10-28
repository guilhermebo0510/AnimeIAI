# Revisão das workspaces Edição, Áudio e Roteiro (seção 5 do plano, 28/10/2026)

Passeio visual numa cópia do projeto do usuário (`tools/csp/workspace_tour.py`, o original não é tocado), comparando
com o que um editor espera do Premiere/DaVinci (Edição), do Audition (Áudio) e do Final Draft/WriterDuet (Roteiro).

## Edição

| Achado | Feito |
|---|---|
| Os clipes de som na linha do tempo tinham só uma linha reta no lugar da forma de onda (sincronizar fala e corte no olho) | forma de onda de verdade: picos por quadro feitos numa thread, guardados por fonte (o som do corte troca de chave quando os clipes dele mudam), desenhados com a velocidade e o volume do clipe |
| Sem marcadores na linha do tempo | marcadores: M põe/tira no cabeçote, Shift+M vai ao próximo, Ctrl+Shift+M ao anterior, clique direito tira; bandeirinha com nome na régua, linha pelas trilhas, o ímã gruda neles; ficam no documento (`EditDoc.markers`) |
| Monitor de programa preto no quadro 0 | não é defeito: o C001 começa com fade do preto |

## Áudio

| Achado | Feito |
|---|---|
| Na multipista, a quarta faixa (Ambiente) ficava fora do painel, sem rolagem, e os controles dela eram desenhados por cima da barra de status | as faixas encolhem até caber no painel; o cabeçalho da faixa fica preso ao painel |
| O zoom automático aparecia como "zoom 0 px/s" | aparece "zoom ajustar ao corte" |

## Roteiro

| Achado | Feito |
|---|---|
| Criar o próximo elemento exigia o mouse ("+ Fala"); o Final Draft e o WriterDuet seguem no teclado | Ctrl+Enter num campo cria o próximo elemento logo abaixo, com o cursor nele e a lista rolando até ele: ação puxa ação, fala puxa a fala do outro personagem da conversa, parentético puxa a fala de quem falava, transição/nota puxam ação |
| Fountain (formato aberto que o Final Draft, Highland, WriterDuet e Fade In leem) só existia pela ponte/MCP | "Exportar Fountain…" e "Importar Fountain…" nas Propriedades (diálogo de arquivo numa thread; importar cria um roteiro novo e abre) |
| Nada sobre o elenco | quadro de personagens nas Propriedades: falas e tempo estimado de cada um (orçamento de voz e elenco) |

## Conferência

`tools/csp/workspace_tour.py`, `script_flow_test.py`, `edit_markers_test.py`; fotos em
`sprints/registro/evidencias/S25/revisao/`. Testes: `aiai-tools/tests/edit.rs::markers_toggle_sort_and_navigate`,
`script.rs::ctrl_enter_alternates_the_speakers_and_follows_the_screenplay_order`.
