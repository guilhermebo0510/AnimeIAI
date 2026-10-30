# WS-R06 — Workspace Animação: auditoria completa (rodada 1)

> **Host:** Claude (o único que implementa) · **Revisor:** Codex (ChatGPT) · **Protocolo:** `WS-R00-PROTOCOLO.md` (leia antes).
> **Entrega:** `sprints\registro\S27-copiloto-e-ecossistema\auditoria\WS-CX06-ANIMACAO.md`
> **Código:** commit `{{COMMIT}}` · release recompilado a partir dele.

## 0. O que é o workspace

Painéis: Pincéis, Propriedades da ferramenta, Tamanho, Cores, Referência do personagem, Informação da cena, canvas, Linha do tempo, Navegador, Propriedades da camada, Mesa de luz, Camadas, Histórico e Propriedades (e o Copiloto, sob demanda). Referência: **Clip Studio, TVPaint, OpenToonz (genga/douga, timesheet)**. Foco de produção: genga e douga: desenhar quadro a quadro, timesheet, mesa de luz, intercalação, referência 3D e o Copiloto.

## 1. Regras de convivência (as do WS-R00)

- Só o seu projeto de teste: `C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-animacao.aiai` (uma cópia de um episódio real;
  pode estragar). Nunca `producoes\`.
- Não edite código, não rode `cargo` nem `git` que altere. App **sem** `--bridge`; feche só o seu PID.
- Um só Copiloto na GPU (não faz parte desta rodada; não ligue).

Abrir:

```
& "C:\AnimeIAI\app\target\release\aiai-studio.exe" --no-session --project "C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-animacao.aiai" --workspace animacao
```

## 2. Profundidade (R23 §2)

Auditoria completa, não a busca do próximo defeito: dezenas de itens verificados, todos de uma vez, cada um com id
(`WS6-01`…), gravidade (**bloqueia** / **não bloqueia** / **melhoria**), área, prova (arquivo:linha ou passos no app
com o esperado e o que aconteceu) e correção esperada; mais a cobertura declarada por área. Fim: "NÍVEL PROFISSIONAL:
SIM", ou "NÃO — bloqueios: …".

## 3. O que auditar (mínimo; vá além)

**A.** Animar um corte curto do genga ao douga: chaves, intervalos, timesheet, mesa de luz (onion skin).

**B.** Linha do tempo e timesheet (exposição, 1s/2s/3s, cels, ciclos).

**C.** Ferramentas de linha: caneta vetorial, Corrigir linha, Editar pontos, borracha vetorial, estabilização.

**D.** Referência 3D (manequim) e o Copiloto no fluxo (já auditado à parte; conferir só a integração).

**E.** Exportar o timesheet (XDTS) e a passagem para a Pintura.

**Comum a todo workspace (WS-R00):** cada botão, menu, atalho e painel; usos cruzados com os outros workspaces;
interface (organização, nomes, dicas, estados vazios, tema claro e escuro, pt e en, janela pequena, ícones sem emoji);
robustez (desfazer e refazer, salvar e abrir, projeto grande, entrada inválida, interromper no meio); desempenho com
números; o que falta comparado à referência, priorizado pelo que é essencial para produzir anime.

**MCP:** aiai_draw_curves, aiai_edit_points, aiai_line_fix, aiai_inbetween*, aiai_export_xdts, aiai_model3d: um agente consegue fazer o trabalho do workspace inteiro? Erros e validação.

**Código:** panels/timeline.rs, light_table.rs, app/tools.rs, canvas.rs (caminhos a partir de `app/crates/aiai-studio/src/app/` quando não indicado).

## 4. O que entregar

1. o commit e o executável usado (tamanho, data, SHA-256);
2. a tabela de achados, sem limite de quantidade;
3. a cobertura por área;
4. o que falta para o nível do programa de referência, separando "agora" de "depois";
5. o veredito.
