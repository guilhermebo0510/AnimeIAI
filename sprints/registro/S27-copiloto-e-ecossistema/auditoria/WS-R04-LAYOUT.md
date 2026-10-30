# WS-R04 — Workspace Layout: auditoria completa (rodada 1)

> **Host:** Claude (o único que implementa) · **Revisor:** Codex (ChatGPT) · **Protocolo:** `WS-R00-PROTOCOLO.md` (leia antes).
> **Entrega:** `sprints\registro\S27-copiloto-e-ecossistema\auditoria\WS-CX04-LAYOUT.md`
> **Código:** commit `{{COMMIT}}` · release recompilado a partir dele.

## 0. O que é o workspace

Painéis: Pincéis e Réguas, Propriedades da ferramenta, Tamanho, Cores/Projeto/Subvista, canvas, Navegador, Propriedades da camada, Texto, Camadas, Histórico e Propriedades. Referência: **Clip Studio, Storyboard Pro (layout), folha de layout de estúdio**. Foco de produção: o layout de corte: enquadramento, câmera, separação BG/cel, guias de perspectiva e instruções de câmera.

## 1. Regras de convivência (as do WS-R00)

- Só o seu projeto de teste: `C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-layout.aiai` (uma cópia de um episódio real;
  pode estragar). Nunca `producoes\`.
- Não edite código, não rode `cargo` nem `git` que altere. App **sem** `--bridge`; feche só o seu PID.
- Um só Copiloto na GPU (não faz parte desta rodada; não ligue).

Abrir:

```
& "C:\AnimeIAI\app\target\release\aiai-studio.exe" --no-session --project "C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-layout.aiai" --workspace layout
```

## 2. Profundidade (R23 §2)

Auditoria completa, não a busca do próximo defeito: dezenas de itens verificados, todos de uma vez, cada um com id
(`WS4-01`…), gravidade (**bloqueia** / **não bloqueia** / **melhoria**), área, prova (arquivo:linha ou passos no app
com o esperado e o que aconteceu) e correção esperada; mais a cobertura declarada por área. Fim: "NÍVEL PROFISSIONAL:
SIM", ou "NÃO — bloqueios: …".

## 3. O que auditar (mínimo; vá além)

**A.** Fazer o layout de um corte a partir do painel do storyboard: quadro da câmera, área de segurança, BG e cel separados.

**B.** Réguas e perspectiva (1, 2 e 3 pontos), guia 3D, grade.

**C.** Instruções de câmera no layout (PAN, T.U., follow) e a ligação com a câmera do corte.

**D.** Folha de layout para imprimir/exportar.

**E.** Voltar ao storyboard e seguir para Cenário e Animação com o mesmo corte.

**Comum a todo workspace (WS-R00):** cada botão, menu, atalho e painel; usos cruzados com os outros workspaces;
interface (organização, nomes, dicas, estados vazios, tema claro e escuro, pt e en, janela pequena, ícones sem emoji);
robustez (desfazer e refazer, salvar e abrir, projeto grande, entrada inválida, interromper no meio); desempenho com
números; o que falta comparado à referência, priorizado pelo que é essencial para produzir anime.

**MCP:** aiai_ruler, aiai_perspective, aiai_guide3d, aiai_camera: um agente consegue fazer o trabalho do workspace inteiro? Erros e validação.

**Código:** app/rulers.rs, canvas.rs (guias), panels/subview.rs (caminhos a partir de `app/crates/aiai-studio/src/app/` quando não indicado).

## 4. O que entregar

1. o commit e o executável usado (tamanho, data, SHA-256);
2. a tabela de achados, sem limite de quantidade;
3. a cobertura por área;
4. o que falta para o nível do programa de referência, separando "agora" de "depois";
5. o veredito.
