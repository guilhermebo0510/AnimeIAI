# S26 — Prévia de intercalação de um quadro (R16, MVP-0)

Implementação do material aceito pelos três revisores (`ferramenta de Ia/revisao-claude/R16-MATERIAL-DE-IMPLEMENTACAO-MVP-0-FINAL.md`),
na ordem do §13. Decisões do dono aplicadas: D-P07 (MCP **e** interface), D-P09 (base congelada), DEC-03 (b), DEC-04 (b),
DEC-05 (b: aceitar só pelo botão, sem atalho).

## 1. P-1 — base congelada

- `manifesto-base-6655c06.sha256`: SHA-256 dos 713 arquivos de `app/crates/` **no commit** `6655c06` (conteúdo do
  commit, não da cópia de trabalho). Hash da lista: `e37292d93f157d408bc6c26e619f6458192868b6e70542016a17411c580e4d4a`.
  Conferir um arquivo: `git show 6655c06:<caminho> | sha256sum`.
- Os 2 arquivos locais que estavam fora do escopo (`panels/colors.rs`, `layout.rs`) já foram commitados depois
  (fazem parte dos 58 commits seguintes); nenhum deles é tocado pelo MVP-0.
- O registro de cada cel aceito guarda o commit-base como versão do motor (`ENGINE_BASE`).
- O motor (`aiai_anim::inbetween`) **não mudou de comportamento**: ganhou só uma leitura (`pair_path`,
  `resampled_len`) que diz como ele trata cada par (direto/reamostrado, inversão, deslocamento) para a prévia avisar; o
  `align` passou a usar a mesma escolha (`align_choice`), mesmo resultado (testes do motor e da intercalação em lote
  passam iguais).

## 2. Origem da entrada (REQ-P01…P03)

- `LogEntry.source: InputSource` (`ui_platform_unattributed | bridge_injected | direct_tool_call | import | mixed |
  unknown_legacy`), ausente = `unknown_legacy`. A sessão tem `source` (como `author`) e cada apply/undo/redo grava a dela.
- App: a fila de entrada (`input_queue.rs`) marca, a cada quadro, quem emitiu os eventos que **disparam** ação (botão do
  ponteiro, roda, tecla, texto, toque que começa/termina); movimento não conta. O gesto (apertar até soltar) junta as
  fontes: apertar injetado e soltar de verdade = `mixed`. Antes da interface rodar, `core.source` recebe a fonte do quadro.
- Bridge/MCP: toda chamada direta grava `direct_tool_call` (o app troca a fonte só durante a chamada; o headless nasce
  assim). Importar arquivo pela interface = `import`. `merge_last` juntando fontes diferentes = `mixed`.
- Nada transforma o autor `"human"` em prova de pessoa: ação da interface sem vínculo com evento injetado fica
  `ui_platform_unattributed`.

## 3. Prévia (REQ-D01, D02, C01, C02, §5)

`aiai-tools/src/inbetween_preview.rs`:

- `inbetween_preview(p, envelope, req)` — função pura: camada vetorial com faixa no timesheet (sem faixa → "configure o
  timesheet desta camada"); chaves pelos **começos de bloco** (sem quadros no pedido: o bloco-chave mais próximo antes e
  depois; com quadros: confere que começam blocos e que expõem `key_a`/`key_b`); pares manuais por `Stroke.id` (id
  inexistente, repetido no cel, em dois pares ou par incompatível = erro; o pedido não tem campo de pares por índice);
  tabela do §5 inteira (preserva / aproxima / recusa); limites **antes** do motor; candidato só sem recusa, validado
  (`Stroke::validate` + finitude). Nada é gravado.
- Limites (meta, a fixar em P-3): 200 traços por chave, 20 000 pontos somados, 1 000 pontos por traço num par (o
  reamostrado é `2 × max(N_A, N_B, 8)`; num fechado o alinhamento é ~2n²), 60 000 pontos na saída.
- Sentido invertido no caminho direto: recusa quando alinhar reduziria a distância média a menos da metade e em mais de
  1 px (abaixo disso é ruído de desenho quase simétrico) — limiar a confirmar no E3.
- `ProposalStore` fica **na própria sessão** (`CoreSession::ext_mut`): uma proposta viva por vez, ids que não se repetem
  entre processos, `document_epoch` da sessão (muda ao abrir/trocar o projeto), revisão, geração; mudar pares deixa a
  proposta `stale` (aceite bloqueado) até Atualizar, que gera id novo; qualquer mudança de revisão (inclusive
  desfazer/refazer) derruba a proposta com o motivo, sem recalcular.

## 4. Aceite (REQ-A01…A05, P04)

- `inbetween_frame_command`: cel novo (próximo número), papel intercalação, transformação identidade, os traços do
  candidato guardado, `inbetween_origin`; `frame` → cel novo; se `frame+1` está no corte e não tem entrada própria, grava
  nele o que estava exposto (ou vazio). Sem `AddTrack`. Um `Batch`, um passo de desfazer, rótulo
  "Intercalar no quadro N entre A e B".
- `accept`: precondições (recusa, desatualizada, época, revisão, geração, consumida, camada vetorial/travada/faixa,
  quadros), `apply_as(..., Some(revisão))`, consumo único ("proposta N já usada").
- `CelData.inbetween_origin` (opcional): fonte do aceite, mecanismo, versão do motor, opções, chaves, cels e traços fonte,
  quadros, t, revisão de origem. Desfazer/refazer, salvar/reabrir e cópias levam junto.
- Botão: "Criar desenho A4 no quadro 12 (o quadro 13 continua mostrando A3)" (quadros como a interface mostra, a partir de 1).

## 5. Bridge e MCP (REQ-M01…M04)

- Ações `INBETWEEN_PREVIEW`, `INBETWEEN_ACCEPT`, `INBETWEEN_DISCARD`; `RENDER with_preview` (o quadro como está + o
  candidato na cor da prévia, no lugar exato da camada, canvas sem câmera; transitório: nada é gravado).
- Ferramentas MCP `aiai_inbetween_preview`, `aiai_inbetween_accept`, `aiai_inbetween_discard`; `aiai_render` aceita
  `with_preview`.
- Dono da proposta: o MCP lembra quem calculou cada prévia (app ou headless). Aceitar/descartar/renderizar vão só para
  ele; app fora do ar = erro explícito ("ela não existe em outra sessão"), sem cair no headless. O headless reaberto (o
  arquivo mudou por fora) é outra sessão: "não encontrada nesta sessão".

## 6. Interface (REQ-U01…U05)

- Comando **Prévia de intercalação** no painel Mesa de luz e no menu da célula da Linha do tempo (o app não tem menu
  Animação na barra de cima), ligado só quando o quadro está entre dois blocos-chave da camada (senão o motivo na dica).
- Sobreposição no canvas: o candidato na cor da prévia (magenta, diferente do vermelho/verde da mesa de luz) com a largura
  dos traços, as chaves A (vermelho) e B (verde) finas, linha entre os centros de cada par (cinza preserva, amarelo
  aproxima, vermelho recusa; par manual mais grosso). Projeção: movimento da camada no quadro alvo com a transformação
  identidade do candidato, depois a vista (zoom, rotação, espelho não mudam a prévia).
- Barra no rodapé do canvas: Aceitar (o alcance escrito; desligado com recusa, prévia desatualizada ou camada travada,
  com o motivo), Atualizar, Corrigir pares, Descartar e a lista de recusas/avisos/observações. Sem atalho de teclado.
- Corrigir pares: clique num traço de A e depois num de B; clique num traço de A com par manual e Delete desfaz.
- Modo diretor: a prévia pedida pelo MCP aparece na tela (a interface vai ao quadro dela).
- `GET_UI_STATE.editor.ibp` mostra a proposta viva (id, quadros, desatualizada, aceitável, pares manuais).

## 7. Testes

- `aiai-tools/tests/inbetween_preview.rs` (12): AT-01, 03, 04, 06, 07, 08, 09, 10, 16, 17, 18, 19, 20, 21.
- `aiai-core/tests/suite.rs::log_entries_carry_input_source_and_merge_mixes` (REQ-P01/P03).
- `aiai-studio` `input_queue::frame_source_links_actions_to_the_events_that_fired_them` (AT-15: injetado, plataforma,
  tecla real com movimento injetado, gesto misto).
- `tools/ibp/ibp_e2e.py` (AT-13, 14, 15 e a tela), evidências em `sprints/registro/evidencias/S26/`:
  - **MCP headless de vida longa** (servidor stdio): prévia (A1 em 0, B em 12, t = 0,5), render com prévia
    (`h_with_preview.png`), descartar, aceitar após descartar falha, aceitar, segundo aceite "proposta … já usada", log
    com `direct_tool_call`, `inbetween_origin` completo no cel; prévia num servidor e aceite noutro: "não encontrada nesta
    sessão" (sem cair em outra sessão).
  - **App**: a prévia pedida pela bridge aparece na tela no quadro dela (`a_preview_bar.png`); Corrigir pares por cliques
    injetados (olho esquerdo de A com o direito de B) deixa a proposta desatualizada e o aceite pela bridge falha com "os
    pares mudaram: Atualizar antes de aceitar" (`a_pairs_stale.png`); Atualizar gera id novo; Delete tira o par;
    **Aceitar por clique injetado grava `bridge_injected`** no log e no registro do cel; o quadro 7 ganha a entrada que
    mantém A3; **a prévia cai exatamente onde o cel aceito cai** (camada com movimento em ease, quadro alvo exposto com
    transformação de cel +40 px e chaves sem transformação): caixa da prévia = caixa do aceito, `[492, 213, 717, 476]`;
    mudar a chave com a prévia aberta derruba a prévia e a barra mostra o motivo com "Gerar de novo"
    (`a_invalidated.png`), que refaz a prévia.

### E3 — fidelidade (`aiai-tools/tests/inbetween_e3.rs`)

Em todo caso aceito o cel gravado é exatamente o candidato, o render do cel aceito é igual pixel a pixel ao desenho da
prévia e nenhum atributo muda sem aviso. Os casos que o motor não preserva são recusados antes de gravar.

| caso | classe do par | avisos/recusas | resultado |
|---|---|---|---|
| aberto, direto, cores diretas diferentes | preserva | — | aceito; 0 pixel diferente; nenhuma perda |
| fechado, direto, mesmo sentido | preserva | — | aceito; 0 pixel; nenhuma perda |
| contagens diferentes (reamostrado) | aproxima | Resampled | aceito; 0 pixel; nenhuma perda |
| perfil igual, direto | preserva | — | aceito; 0 pixel; nenhuma perda |
| perfil igual, reamostrado e invertido | recusa | Resampled, ProfileReparametrized | recusado antes de gravar |
| largura variável por ponto | recusa | PointWidth | recusado antes de gravar |
| alça (handle) | recusa | PointHandle | recusado antes de gravar |
| Bézier, direto | aproxima | BezierDirect | aceito; 0 pixel; nenhuma perda |
| Bézier, reamostrado | recusa | BezierResampled | recusado antes de gravar |
| cores de paleta diferentes | aproxima | ColorSwitch | aceito; 0 pixel; nenhuma perda |
| ponta quadrada igual | preserva | TipPreviewSimplified (observação) | aceito; 0 pixel; nenhuma perda |
| pontas diferentes | aproxima | TipDiffers | aceito; 0 pixel; nenhuma perda |
| traço sem par (some) | aproxima | Unmatched | aceito; 0 pixel; nenhuma perda |
| quadro da chave A (t = 0) / da chave B (t = 1) | — | — | não oferecido: a prévia só existe entre as chaves |

Leitura: a tabela de suporte do §5 se confirma no protótipo; nenhum caso pede mudar de classe. O limiar de "sentido
diferente" (§3) não foi exercitado por desenhos de pessoas — fica para o teste com pessoa.

### E1 — custo na interface e do cálculo (`tools/ibp/ibp_e1.py`, release)

Máquina: Intel Core i7-10750H, NVIDIA GTX 1650 (+ Intel UHD), Windows 11, build release, janela 1500×980.
Repetição: cursor em círculos sobre o canvas e zoom pela roda (nada que mude o projeto: mudar o projeto derruba a
prévia), 3 000 quadros por condição, 2 repetições, prévia desligada × ligada. Amostras brutas em
`evidencias/S26/e1.json`, resumo em `e1_resumo.json`. `ui_ms` = CPU de montar o quadro inteiro (não inclui a
tesselação do egui nem a apresentação na GPU); `paint` = montar a sobreposição.

| desenho | cálculo da prévia (10×) | quadro, desligada p50 / p95 / p99 / máx | quadro, ligada p50 / p95 / p99 / máx | sobreposição p50 / p95 |
|---|---|---|---|---|
| normal (4 traços, 43 pontos na saída) | 0,08 ms (máx 0,35) | 3,25 / 3,92 / 4,51 / 7,58 ms | 3,46 / 4,13 / 4,69 / 7,49 ms | 0,18 / 0,27 ms |
| no limite (190 traços por chave, 19 000 pontos; 9 500 na saída) | 2,00 ms (máx 4,47) | 3,73 / 4,54 / 5,41 / 10,78 ms | 4,97 / 6,37 / 8,31 / 13,34 ms | 1,71 / 2,46 ms |

O primeiro ensaio (build de teste, antes de guardar a geometria) mostrou a sobreposição custando ~11 ms por quadro no
caso no limite, porque reamostrava todas as curvas a cada quadro; agora a geometria é feita uma vez por proposta e o
candidato vai numa malha só (1,7 ms). A prévia continuou viva até o fim de todas as repetições.

Leitura para a decisão do dono (P-3 / E1): o cálculo é síncrono e só roda no pedido (botão ou MCP), nunca por quadro; no
limite meta ele leva até 4,5 ms, abaixo de um quadro de 60 Hz. Com estes números o **síncrono basta** e o worker (REQ-C03)
não é necessário; os limites e o orçamento ainda precisam ser fixados pelo dono.

## 8. O que fica aberto

- P-3 (dono): fixar os limites e o orçamento de tempo da prévia síncrona.
- E1 e E3 rodados (acima); a decisão síncrono × worker e o fechamento da tabela ficam com o dono, com estes números.
- Teste com pessoa (§11) — precisa do dono.
