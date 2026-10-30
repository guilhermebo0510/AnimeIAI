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
