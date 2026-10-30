# WS-R10 — Workspace Modelagem: auditoria completa (rodada 1)

> **Host:** Claude (o único que implementa) · **Revisor:** Codex (ChatGPT) · **Protocolo:** `WS-R00-PROTOCOLO.md` (leia antes).
> **Entrega:** `sprints\registro\S27-copiloto-e-ecossistema\auditoria\WS-CX10-MODELAGEM.md`
> **Código:** commit `{{COMMIT}}` · release recompilado a partir dele.

## 0. O que é o workspace

Painéis: Outliner do modelo, Subvista, viewport de Modelagem, Propriedades, Modificadores e UV. Referência: **Blender (modelagem, rig, pose, animação), ANIGO (o manequim)**. Foco de produção: modelar adereços e blocagem, riggar, posar e animar como referência para o 2D, câmera 3D. Leia também a minha auditoria interna, feita antes desta rodada: `../fase3-estado-e-lacunas.md` (só leitura de código; os itens dela são hipóteses a conferir).

## 1. Regras de convivência (as do WS-R00)

- Só o seu projeto de teste: `C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-modelagem.aiai` (uma cópia de um episódio real;
  pode estragar). Nunca `producoes\`.
- Não edite código, não rode `cargo` nem `git` que altere. App **sem** `--bridge`; feche só o seu PID.
- Um só Copiloto na GPU (não faz parte desta rodada; não ligue).

Abrir:

```
& "C:\AnimeIAI\app\target\release\aiai-studio.exe" --no-session --project "C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-modelagem.aiai" --workspace modelagem
```

## 2. Profundidade (R23 §2)

Auditoria completa, não a busca do próximo defeito: dezenas de itens verificados, todos de uma vez, cada um com id
(`WS10-01`…), gravidade (**bloqueia** / **não bloqueia** / **melhoria**), área, prova (arquivo:linha ou passos no app
com o esperado e o que aconteceu) e correção esperada; mais a cobertura declarada por área. Fim: "NÍVEL PROFISSIONAL:
SIM", ou "NÃO — bloqueios: …".

## 3. O que auditar (mínimo; vá além)

**A.** Modelar um adereço simples (primitivas, extrusão, modificadores, UV) e usar como referência 3D num corte.

**B.** Rig: o manequim (55 ossos) e o que falta para riggar um modelo próprio (pesos automáticos, pintura de pesos), VRM.

**C.** Posar e animar: arrastar juntas, IK, espelhar, biblioteca de poses, chaves de pose por quadro, ciclos.

**D.** Câmera 3D: enquadrar, animar, ligar à câmera do corte.

**E.** Passagem para o 2D: guia de linha por quadro para o genga.

**Comum a todo workspace (WS-R00):** cada botão, menu, atalho e painel; usos cruzados com os outros workspaces;
interface (organização, nomes, dicas, estados vazios, tema claro e escuro, pt e en, janela pequena, ícones sem emoji);
robustez (desfazer e refazer, salvar e abrir, projeto grande, entrada inválida, interromper no meio); desempenho com
números; o que falta comparado à referência, priorizado pelo que é essencial para produzir anime.

**MCP:** aiai_modeling, aiai_model3d, aiai_guide3d, aiai_rig: um agente consegue fazer o trabalho do workspace inteiro? Erros e validação.

**Código:** app/modeling/*, app/model3d.rs; aiai-3d-*, aiai-guide3d (caminhos a partir de `app/crates/aiai-studio/src/app/` quando não indicado).

## 4. O que entregar

1. o commit e o executável usado (tamanho, data, SHA-256);
2. a tabela de achados, sem limite de quantidade;
3. a cobertura por área;
4. o que falta para o nível do programa de referência, separando "agora" de "depois";
5. o veredito.
