# WS-R14 — Workspace Produção: auditoria completa (rodada 1)

> **Host:** Claude (o único que implementa) · **Revisor:** Codex (ChatGPT) · **Protocolo:** `WS-R00-PROTOCOLO.md` (leia antes).
> **Entrega:** `sprints\registro\S27-copiloto-e-ecossistema\auditoria\WS-CX14-PRODUCAO.md`
> **Código:** commit `{{COMMIT}}` · release recompilado a partir dele.

## 0. O que é o workspace

Painéis: Episódios, Tarefas, Gráficos, Propriedades, Gargalos e IA da produção. Referência: **ShotGrid, ftrack**. Foco de produção: acompanhar a produção: tarefas por corte e etapa, retakes, prazos, gráficos e gargalos.

## 1. Regras de convivência (as do WS-R00)

- Só o seu projeto de teste: `C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-producao.aiai` (uma cópia de um episódio real;
  pode estragar). Nunca `producoes\`.
- Não edite código, não rode `cargo` nem `git` que altere. App **sem** `--bridge`; feche só o seu PID.
- Um só Copiloto na GPU (não faz parte desta rodada; não ligue).

Abrir:

```
& "C:\AnimeIAI\app\target\release\aiai-studio.exe" --no-session --project "C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-producao.aiai" --workspace producao
```

## 2. Profundidade (R23 §2)

Auditoria completa, não a busca do próximo defeito: dezenas de itens verificados, todos de uma vez, cada um com id
(`WS14-01`…), gravidade (**bloqueia** / **não bloqueia** / **melhoria**), área, prova (arquivo:linha ou passos no app
com o esperado e o que aconteceu) e correção esperada; mais a cobertura declarada por área. Fim: "NÍVEL PROFISSIONAL:
SIM", ou "NÃO — bloqueios: …".

## 3. O que auditar (mínimo; vá além)

**A.** Acompanhar um episódio: tarefas por corte e etapa, estados, responsáveis, prazos.

**B.** Retakes: abrir, anotar, comparar, fechar.

**C.** Gráficos e gargalos: o que mostram e se ajudam a decidir.

**D.** Ligação com os outros workspaces (abrir o corte da tarefa).

**Comum a todo workspace (WS-R00):** cada botão, menu, atalho e painel; usos cruzados com os outros workspaces;
interface (organização, nomes, dicas, estados vazios, tema claro e escuro, pt e en, janela pequena, ícones sem emoji);
robustez (desfazer e refazer, salvar e abrir, projeto grande, entrada inválida, interromper no meio); desempenho com
números; o que falta comparado à referência, priorizado pelo que é essencial para produzir anime.

**MCP:** aiai_production, aiai_correction, aiai_retake, aiai_compare: um agente consegue fazer o trabalho do workspace inteiro? Erros e validação.

**Código:** panels/production.rs, prod_*.rs; aiai-tools production*.rs (caminhos a partir de `app/crates/aiai-studio/src/app/` quando não indicado).

## 4. O que entregar

1. o commit e o executável usado (tamanho, data, SHA-256);
2. a tabela de achados, sem limite de quantidade;
3. a cobertura por área;
4. o que falta para o nível do programa de referência, separando "agora" de "depois";
5. o veredito.
