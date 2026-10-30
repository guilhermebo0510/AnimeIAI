# WS-R07 — Workspace Pintura: auditoria completa (rodada 1)

> **Host:** Claude (o único que implementa) · **Revisor:** Codex (ChatGPT) · **Protocolo:** `WS-R00-PROTOCOLO.md` (leia antes).
> **Entrega:** `sprints\registro\S27-copiloto-e-ecossistema\auditoria\WS-CX07-PINTURA.md`
> **Código:** commit `{{COMMIT}}` · release recompilado a partir dele.

## 0. O que é o workspace

Painéis: Pincéis, Propriedades da ferramenta, Tamanho, Cores/Projeto, canvas com Linha do tempo, Navegador, Propriedades da camada/Subvista, Camadas, Histórico e Propriedades. Referência: **RETAS PaintMan, OpenToonz (pintura por paleta)**. Foco de produção: pintar os cels por paleta: balde que respeita a linha, sombra e luz, propagar pelos quadros, conferir.

## 1. Regras de convivência (as do WS-R00)

- Só o seu projeto de teste: `C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-pintura.aiai` (uma cópia de um episódio real;
  pode estragar). Nunca `producoes\`.
- Não edite código, não rode `cargo` nem `git` que altere. App **sem** `--bridge`; feche só o seu PID.
- Um só Copiloto na GPU (não faz parte desta rodada; não ligue).

Abrir:

```
& "C:\AnimeIAI\app\target\release\aiai-studio.exe" --no-session --project "C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-pintura.aiai" --workspace pintura
```

## 2. Profundidade (R23 §2)

Auditoria completa, não a busca do próximo defeito: dezenas de itens verificados, todos de uma vez, cada um com id
(`WS7-01`…), gravidade (**bloqueia** / **não bloqueia** / **melhoria**), área, prova (arquivo:linha ou passos no app
com o esperado e o que aconteceu) e correção esperada; mais a cobertura declarada por área. Fim: "NÍVEL PROFISSIONAL:
SIM", ou "NÃO — bloqueios: …".

## 3. O que auditar (mínimo; vá além)

**A.** Pintar um corte inteiro por paleta: balde, fechar falhas, linhas de cor (vermelha/azul), sombra e luz.

**B.** Propagar cores pelos quadros e conferir o que ficou sem cor.

**C.** Trocar a cor de uma parte na paleta e ver o corte inteiro mudar.

**D.** Colorir por dicas e as ferramentas de preenchimento.

**Comum a todo workspace (WS-R00):** cada botão, menu, atalho e painel; usos cruzados com os outros workspaces;
interface (organização, nomes, dicas, estados vazios, tema claro e escuro, pt e en, janela pequena, ícones sem emoji);
robustez (desfazer e refazer, salvar e abrir, projeto grande, entrada inválida, interromper no meio); desempenho com
números; o que falta comparado à referência, priorizado pelo que é essencial para produzir anime.

**MCP:** aiai_fill, aiai_fill_area, aiai_propagate_fills, aiai_colorize_hints, aiai_probe_color: um agente consegue fazer o trabalho do workspace inteiro? Erros e validação.

**Código:** aiai-tools fill.rs, fill_area.rs, hints.rs; panels/colors.rs (caminhos a partir de `app/crates/aiai-studio/src/app/` quando não indicado).

## 4. O que entregar

1. o commit e o executável usado (tamanho, data, SHA-256);
2. a tabela de achados, sem limite de quantidade;
3. a cobertura por área;
4. o que falta para o nível do programa de referência, separando "agora" de "depois";
5. o veredito.
