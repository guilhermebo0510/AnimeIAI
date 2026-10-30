# WS-R08 — Workspace Composição: auditoria completa (rodada 1)

> **Host:** Claude (o único que implementa) · **Revisor:** Codex (ChatGPT) · **Protocolo:** `WS-R00-PROTOCOLO.md` (leia antes).
> **Entrega:** `sprints\registro\S27-copiloto-e-ecossistema\auditoria\WS-CX08-COMPOSICAO.md`
> **Código:** commit `{{COMMIT}}` · release recompilado a partir dele.

## 0. O que é o workspace

Painéis: Projeto, Camadas, Histórico, viewer, Efeitos e predefinições, Propriedades, Texto, Controles de efeito, Linha do tempo da composição e a de exposição. Referência: **After Effects**. Foco de produção: satsuei: camadas, efeitos, chaves, gráfico, matte, câmera e multiplano, partículas 2D e 3D. Leia também a minha auditoria interna, feita antes desta rodada: `../fase3-estado-e-lacunas.md` (só leitura de código; os itens dela são hipóteses a conferir).

## 1. Regras de convivência (as do WS-R00)

- Só o seu projeto de teste: `C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-composicao.aiai` (uma cópia de um episódio real;
  pode estragar). Nunca `producoes\`.
- Não edite código, não rode `cargo` nem `git` que altere. App **sem** `--bridge`; feche só o seu PID.
- Um só Copiloto na GPU (não faz parte desta rodada; não ligue).

Abrir:

```
& "C:\AnimeIAI\app\target\release\aiai-studio.exe" --no-session --project "C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-composicao.aiai" --workspace composicao
```

## 2. Profundidade (R23 §2)

Auditoria completa, não a busca do próximo defeito: dezenas de itens verificados, todos de uma vez, cada um com id
(`WS8-01`…), gravidade (**bloqueia** / **não bloqueia** / **melhoria**), área, prova (arquivo:linha ou passos no app
com o esperado e o que aconteceu) e correção esperada; mais a cobertura declarada por área. Fim: "NÍVEL PROFISSIONAL:
SIM", ou "NÃO — bloqueios: …".

## 3. O que auditar (mínimo; vá além)

**A.** Compor um corte com BG, cels, sombra, efeitos de luz, câmera e partículas; renderizar.

**B.** Linha do tempo: chaves, easing, editor de gráfico, modos de mistura, matte; o que falta do After Effects (pré-composição, máscaras desenhadas, luma matte, gráfico de velocidade).

**C.** Efeitos: cada um, chaves das propriedades, posterize, camada de efeito.

**D.** Câmera 2D e multiplano; o que falta para câmera e camadas 3D.

**E.** Partículas: presets, ajustes, desempenho; o que falta para partículas 3D.

**Comum a todo workspace (WS-R00):** cada botão, menu, atalho e painel; usos cruzados com os outros workspaces;
interface (organização, nomes, dicas, estados vazios, tema claro e escuro, pt e en, janela pequena, ícones sem emoji);
robustez (desfazer e refazer, salvar e abrir, projeto grande, entrada inválida, interromper no meio); desempenho com
números; o que falta comparado à referência, priorizado pelo que é essencial para produzir anime.

**MCP:** aiai_effects, aiai_grade, aiai_camera, aiai_layer_motion, aiai_particles, aiai_anime_fx: um agente consegue fazer o trabalho do workspace inteiro? Erros e validação.

**Código:** comp_viewer.rs, panels/comp_timeline.rs, effect_controls.rs, effects_browser.rs, particles_ed.rs; aiai-fx, aiai-anim (caminhos a partir de `app/crates/aiai-studio/src/app/` quando não indicado).

## 4. O que entregar

1. o commit e o executável usado (tamanho, data, SHA-256);
2. a tabela de achados, sem limite de quantidade;
3. a cobertura por área;
4. o que falta para o nível do programa de referência, separando "agora" de "depois";
5. o veredito.
