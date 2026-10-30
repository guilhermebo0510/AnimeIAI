# WS-R03 — Workspace Storyboard: auditoria completa (rodada 1)

> **Host:** Claude (o único que implementa) · **Revisor:** Codex (ChatGPT) · **Protocolo:** `WS-R00-PROTOCOLO.md` (leia antes).
> **Entrega:** `sprints\registro\S27-copiloto-e-ecossistema\auditoria\WS-CX03-STORYBOARD.md`
> **Código:** commit `{{COMMIT}}` · release recompilado a partir dele.

## 0. O que é o workspace

Painéis: Planos (lista), Quadro de painéis (grade), Propriedades e Linha do tempo. Referência: **Storyboard Pro, Clip Studio (storyboard), e-conte japonês**. Foco de produção: painéis, tempo, câmera, falas e SE, animatic e a passagem para os cortes. Leia também a minha auditoria interna, feita antes desta rodada: `WS-CLAUDE-03-STORYBOARD-INTERNO.md` (só leitura de código; os itens dela são hipóteses a conferir).

## 1. Regras de convivência (as do WS-R00)

- Só o seu projeto de teste: `C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-storyboard.aiai` (uma cópia de um episódio real;
  pode estragar). Nunca `producoes\`.
- Não edite código, não rode `cargo` nem `git` que altere. App **sem** `--bridge`; feche só o seu PID.
- Um só Copiloto na GPU (não faz parte desta rodada; não ligue).

Abrir:

```
& "C:\AnimeIAI\app\target\release\aiai-studio.exe" --no-session --project "C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-storyboard.aiai" --workspace storyboard
```

## 2. Profundidade (R23 §2)

Auditoria completa, não a busca do próximo defeito: dezenas de itens verificados, todos de uma vez, cada um com id
(`WS3-01`…), gravidade (**bloqueia** / **não bloqueia** / **melhoria**), área, prova (arquivo:linha ou passos no app
com o esperado e o que aconteceu) e correção esperada; mais a cobertura declarada por área. Fim: "NÍVEL PROFISSIONAL:
SIM", ou "NÃO — bloqueios: …".

## 3. O que auditar (mínimo; vá além)

**A.** Fazer um storyboard de uma cena do roteiro: painéis, ação, fala, SE, segundos, tipo de plano.

**B.** Desenhar os painéis: onde se desenha, quanto troca de tela, próximo e anterior.

**C.** Câmera do painel (pan, zoom, T.U./T.B.) e transições.

**D.** Animatic: gerar, tocar, com as falas; a folha de e-conte (formato, paginação).

**E.** Gerar os cortes a partir do storyboard (episódio, cenas, numeração) e a ida e volta com o roteiro.

**Comum a todo workspace (WS-R00):** cada botão, menu, atalho e painel; usos cruzados com os outros workspaces;
interface (organização, nomes, dicas, estados vazios, tema claro e escuro, pt e en, janela pequena, ícones sem emoji);
robustez (desfazer e refazer, salvar e abrir, projeto grande, entrada inválida, interromper no meio); desempenho com
números; o que falta comparado à referência, priorizado pelo que é essencial para produzir anime.

**MCP:** aiai_storyboard, aiai_animatic, aiai_cuts_from_storyboard: um agente consegue fazer o trabalho do workspace inteiro? Erros e validação.

**Código:** panels/board.rs, shots.rs; aiai-tools preprod.rs (storyboard) (caminhos a partir de `app/crates/aiai-studio/src/app/` quando não indicado).

## 4. O que entregar

1. o commit e o executável usado (tamanho, data, SHA-256);
2. a tabela de achados, sem limite de quantidade;
3. a cobertura por área;
4. o que falta para o nível do programa de referência, separando "agora" de "depois";
5. o veredito.
