# S27 — Roteiro: Copiloto profissional, auditoria dos workspaces, ferramentas de IA

> Pedido do dono em 29/10/2026, à noite. Trabalho contínuo, sem esperar validação. O plano vivo é este arquivo: atualize
> o estado de cada item ao terminar e faça commit a cada item. O hook de Stop (`tools/overnight_stop_hook.py`) só deixa
> parar quando `sprints/.overnight-done` existir.

## Método (vale para todas as fases)

- **Claude é o host e o único que implementa.** O ChatGPT (app do ChatGPT, modo Codex) audita: lê o código, usa o app
  com mouse e teclado e tenta quebrar a ferramenta com usos cruzados. Ele escreve um arquivo por rodada, e o Claude
  responde com as correções.
- **Rodada profunda:** o revisor não para no primeiro achado. Ele entrega todos de uma vez (o alvo é a casa das
  dezenas de itens verificados), cada um com gravidade, prova com a linha do arquivo e a correção esperada, e declara a
  cobertura por área. Nas rodadas seguintes ele confere as correções e o que elas possam ter quebrado. Um achado novo
  numa área já declarada coberta precisa dizer por que não apareceu antes. O alvo é fechar em 3 a 5 rodadas.
- **Vigiar o ChatGPT a cada 10 minutos** com `tools/revisao/peek.ps1`, que não traz a janela para a frente. Se aparecer
  erro do servidor ou "Tentar novamente", clicar e retomar (`tools/revisao/drive.ps1`).
- **Limite de 5 horas do ChatGPT:** se ele acabar, anote o horário em que volta (na tabela "Estado"), pause as rodadas
  e siga com o que não depende dele: a sua auditoria interna, o relatório de tempos, correções já conhecidas, a fase
  seguinte. No horário anotado, retome a rodada com ele.
- **Fim de uma fase:** o revisor escreve "NÍVEL PROFISSIONAL: SIM", e o Claude faz a própria auditoria interna e
  concorda. Só então a fase seguinte começa.
- **Disco C: cheio:** compilar e testar só em release (`cargo test --release`), `CARGO_INCREMENTAL=0 -j 2`.
- **GPU de 4 GB:** um só serviço do Copiloto por vez. Nunca dois apps com o Copiloto ligado: cada um carrega cerca de
  2,9 GB, e juntos transbordam e ficam de 10 a 50 vezes mais lentos.
- Referências permitidas: o Clip Studio aberto (para calibrar), a internet (libs, modelos, papers), o Blender (código
  aberto), o ANIGO (código nosso, pode ser copiado) e outros programas de código aberto de composição, edição e áudio.

## Fase 1 — Copiloto de traço em nível profissional  ⟵ AGORA

**Foco** (não perder): **geração parcial de IA que ajuda a desenhar**. O traço do usuário ou do LLM vira sugestões
fantasma (Melhorar e Completar) em caixas, guiadas pelo prompt, e o usuário aceita. Não é ferramenta de intercalação.

1. [x] Fechar o trabalho em andamento: testes, commit e release (painel no dock, foco, sobreposição, intensidade,
       camada limpa, Tab = um passo, cores, simplificação).
2. [x] Pacote da rodada 1 para o ChatGPT (`ferramenta de Ia/copiloto/revisao/CP-R01…`): auditoria completa (código,
       app, QA cruzado, UI, fluxo de trabalho, desempenho), com projetos de teste próprios dele.
3. [ ] **Relatório de tempo das gerações:** por intensidade, tamanho de caixa, resolução e passos, a primeira carga e as
       seguintes, na GTX 1650 e estimado para uma GPU melhor. O dono notou "minutos" nas gerações completas: investigar
       (a disputa de memória de vídeo entre dois apps é a principal suspeita) e criar perfis **Rápido** e **Qualidade**.
4. [ ] Rodadas CP-CX01… até SIM, e depois a auditoria interna do Claude.
5. [ ] Ideias do revisor e de outros modelos que valham adotar (registrar o que foi adotado e o que foi descartado, e
       por quê).

## Fase 2 — Auditoria de todos os workspaces (ChatGPT, rodadas profundas)

Um workspace por rodada, ou dois pequenos juntos. A ordem segue o pipeline: **Roteiro → Personagens → Storyboard →
Layout → Cenário → Animação → Pintura → Composição → Biblioteca → Modelagem → Áudio → Edição → Entrega → Produção.**
Para cada um: bugs, ferramentas quebradas, comportamento fora do esperado, usos cruzados e fluxo de trabalho, UI
profissional, e o que falta comparado ao programa de referência (Clip Studio, After Effects, Blender, Premiere ou
Audition). O resultado é uma lista priorizada por workspace em `sprints/registro/S27-copiloto-e-ecossistema/auditoria/`.

## Fase 3 — Correções e melhorias dos workspaces

O que a fase 2 levantar, por prioridade (bloqueios primeiro). Já pedido pelo dono:

- **Modelagem:** ferramentas organizadas e as que faltam (Blender e ANIGO como referência). Criar, personalizar e
  **riggar** personagens a partir dos modelos do Clip Studio. Animação 3D do personagem como **referência de movimento**
  para desenhar os cels, câmera e posicionamento. Não é anime 3D: o 3D é referência visual e de animação para o 2D.
- **Animação:** importar o personagem 3D, animar, usar como referência por baixo do desenho; animação de câmera.
- **Composição:** chegar perto do After Effects no que importa para anime: composições complexas, partículas 2D e
  3D, efeitos e chaves.
- Fluxo de trabalho e QA, não só o que quebra.

## Fase 4 — Ferramentas de IA (depois das fases 1–3)

- **Personagem controlado:** escolher um personagem do elenco (de 30, por exemplo). A geração usa a folha de
  modelo (design sheet), a paleta e o material dele, mais um LoRA próprio ou um controle (IP-Adapter, referência ou
  ControlNet). Ao desenhar, o copiloto desenha **aquele** personagem, não um qualquer. Depende de melhorar o sistema de
  design sheet para o material ficar ligado no programa inteiro.
- **Pintura assistida:** colorir a animação usando as paletas do settei, de forma mais automática. Rever os modelos de
  pintura da lista do dono (Paint Transformer, Stylized Neural Painting e outros) para ver o que serve.
- **Cenários com IA controlada:** o usuário define o ângulo e as partes, e a IA ajuda sem ser aleatória.
- **Animação por referência:** a IA anima, ou um vídeo do celular vira referência de movimento (estimativa de pose,
  rig).
- **Modelos próprios:** carregar outros modelos e LoRAs, com o gerenciador de modelos. Treinar um modelo interno mais
  tarde: só ele leva o Copiloto a "muito bom".

## Estado

| Item | Estado | Commit / arquivo |
|---|---|---|
| 1.1 | feito (bateria no app 38/39; a falha restante era do teste e foi confirmada à parte) | `2461b3d` |
| 1.2 | enviado ao ChatGPT em 30/10 às 00:18 (GPT-6.1 Sol, esforço Extra alto) | `ferramenta de Ia/copiloto/revisao/CP-R01-…` |
| 1.3 | tabela de tempos feita; Velocidade Rápido/Equilibrado/Qualidade no painel, aviso de memória (resolução reduzida) e de GPU limitada (R2) | `tools/copiloto/tempos.py`, COPILOTO-COMO-FUNCIONA.md |
| 1.4 | rodada 1: o Codex fechou com 35 achados (15 bloqueios). **Limite de 5 h do ChatGPT esgotado às 01:17, volta às 04:47.** Até lá: corrigir os achados do CX01 e os da auditoria interna (I-01 a I-17). A rodada 2 sai às 04:47. | `CP-CX01-…`, `CP-CLAUDE-AUDITORIA-INTERNA.md` |
| 1.5 | R2: correções de 32 dos 35 achados e de I-01 a I-17 (menos CP1-14, CP1-17 e CP1-34, declarados). Bateria R2 no app 12/13 (a 13ª era do teste, corrigida). | `CP-R02-…` |
| 1.6 | rodada 2 enviada ao Codex às 04:48 (commit `1beb5ee`; testes de unidade 765/765; bateria R2 15/15, completa 39/39). Enquanto ele audita: CP1-17 (caixas salvas com o projeto) no ramo `copiloto-r3` (`77b6ecd`, ainda sem build: o release está em uso pelo Codex), pesquisa de qualidade (`pesquisa-qualidade-copiloto.md`), auditoria interna do Roteiro e o pedido WS-R01 prontos | `CP-R02-…`, `auditoria/WS-R01-ROTEIRO.md` |
| 1.7 | rodada 2 voltou às 05:32: NÃO (CP2-01 a 05, 07 e CP1-10; 24 dos 35 confirmados). Rodada 3 enviada às 07:40 (commit `ec0a63a`): os 12 CP2 corrigidos, a geração parte do desenho do artista (img2img: limpeza fiel e ~2x mais rápida, grade de 18 casos em `calibracao/tmp/qualidade-r3/18`), caixas salvas no projeto. Baterias: R3 13/13, R2 15/15, completa 39/39 | `CP-CX02-…`, `CP-R03-…` |
| 1.8 | **Limite do ChatGPT esgotado de novo às ~08:10 (rodada 3 pela metade: o Codex viu menos invenção e mais aderência nas faixas baixas e estava conferindo a assinatura por região). Volta às 09:48.** Até lá: auditoria interna de workspaces (só leitura de código, sem GPU: o app do Codex segura o Copiloto) e o resultado dos testes de unidade no CP-R03. Às 09:48: pedir que ele continue a rodada 3 de onde parou | `CP-R03-…` |
| 1.9 | 09:49: o Codex retomou a rodada 3 (limite renovado). Enquanto isso: auditorias internas de Roteiro, Personagens e Storyboard conferidas no app (três itens meus estavam errados e foram marcados), fase 3 conferida no app, pedidos WS-R02 a WS-R14 prontos | `auditoria/` |
| 1.10 | **Limite do ChatGPT de novo às ~11:00; volta às 14:49.** Antes de parar, o Codex achou: (a) com o cel deslocado a mesma linha não gera sugestão (falha de leitura); (b) falha forçada ao gravar o `copilot.json` + Salvar ao fechar = o app fecha e perde o prompt novo. Até 14:49: corrigir (a) e (b) num ramo, sem mexer no executável auditado (`ec0a63a`, cópia em `aiai-studio.ec0a63a.keep`). Às 14:49: o Codex termina a rodada 3 no `ec0a63a` | `CP-CX03-…` |
| 1.11 | **Decisão com o dono (30/10, ~12:20): uma caixa por vez.** Autocrítica: a maior parte dos bloqueios do Codex vinha de várias caixas (sobreposição, ímã, caixa IA + foco, pedidos de uma caixa sobre o aceite de outra), o painel enchia de prompts e, numa GPU de 4 a 6 GB, as caixas só esperavam na fila. No ramo `copiloto-r4`: uma caixa; traço dentro atualiza; fora com sugestão é só desenho; aceitar tudo, descartar, ímã que consome e nada sugerido tiram a caixa; caixa à mão e agente usam a mesma. Também: leitura com o cel deslocado, Salvar que falha não fecha, solda na borda. Baterias: R3 16/16 (uma passada falhou com o serviço sem memória durante a compilação dos testes; refazer), R2 15/15, completa 41/41, ímã 5/5. Às 14:50 o Codex foi pedido a fechar a rodada 3 no `ec0a63a` | `copiloto-r4` |
| 1.12 | 14:50: rodada 3 voltou: NÃO (bloqueios CP3-01, CP3-02, CP3-03b, CP3-04 e CP1-10; CP3-03a e a prioridade entre caixas "mudam no desenho de uma caixa"). **Rodada 4 enviada às 18:46** (commit `0a1711e8`, `copiloto-r4` juntado ao `master`): assinatura pela curva inteira (alças), fixture +1200 em teste, ímã renova o restante, sidecar (ids u32, ajustes por projeto, erro de leitura, uma caixa), painel com fonte longa, aceite MCP no corte da sugestão, descrição MCP, prévia = vetor final; qualidade: linha "tubo" vira uma linha só (serviço) e largura sem nó (vetor), grade em `calibracao/tmp/qualidade-r4/18`; achado meu: ímã medido em px de tela com zoom afastado (corrigido). Testes 910/0; baterias R3 16/16, R2 15/15, completa 41/41, ímã 6/6 | `CP-R04-…` |
| 1.14 | **Rodada 4 voltou NÃO (~19:55; relatório escrito antes de terminar a régua): bloqueios CP4-01 (ímã: resto velho sem nova geração), CP4-02 (fantasma de outro quadro prende a caixa), CP4-03 (contexto sem orçamento com a camada muito reduzida), CP4-04 (MCP não recompilado) e CP1-10/CP4-05 (qualidade); confirmou CP3-01, CP3-02 e CP3-04. ~20:15: o Codex esgotou o uso SEMANAL — volta em 4/11 às 11:54.** Rodada 5 no ramo `copiloto-r5`: CP4-01, 02, 03, 06, 07 e I-22 corrigidos; consolidação só onde a entrada tinha tinta; MCP recompilado junto. O app de teste do Codex continua aberto (fechá-lo não foi permitido), então as baterias com GPU esperam | `CP-CX04-…` |
| 1.15 | **PAUSA pedida pelo dono (30/10, ~22:10); o laço noturno (hook de parada) foi desligado e o `cargo clean` rodado.** O dono decidiu seguir sem o Codex (auditoria própria). Rodada 5 no ramo `copiloto-r5` (não juntado): CP4-01/02/03/04/06/07, I-22 e I-24 corrigidos; baterias no release do ramo: R5 7/7, ímã 6/6, R3 16/16, R2 15/15, completa 41/41; unidade copilot 22/22 e copilot_ui 3/3; a suíte inteira foi interrompida pela pausa. Auto-auditoria em `ferramenta de Ia/copiloto/revisao/CP-R05-AUTOAUDITORIA.md` (falta preencher commit e suíte). **Ao retomar:** recompilar (o target foi limpo), rodar a suíte, juntar `copiloto-r5` ao `master`; depois compilar e testar o ramo `fase3-c2` (modos de matte e núcleo das máscaras desenhadas, escritos sem build) e seguir a fase 3 pelos desenhos `fase3-M1-…` e `fase3-C2-…` | `CP-R05-…` |
