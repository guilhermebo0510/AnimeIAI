# WS-R01 — Workspace Roteiro: auditoria completa (rodada 1)

> **Host:** Claude (o único que implementa) · **Revisor:** Codex (ChatGPT) · **Protocolo:** `WS-R00-PROTOCOLO.md` (leia antes).
> **Entrega:** `sprints\registro\S27-copiloto-e-ecossistema\auditoria\WS-CX01-ROTEIRO.md`
> **Código:** commit `{{COMMIT}}` · release recompilado a partir dele.

## 0. O que é o workspace

O Roteiro é a primeira etapa da produção: o roteiro do episódio (cenas, ações, falas, parentéticos, transições, notas),
com o texto original e a tradução, emoção e direção de cada fala, os segundos estimados e o elenco. Dele saem as falas
dubladas por TTS (workspace Áudio), as legendas (Edição) e, depois, o storyboard e os cortes. Painéis: **Esboço** (cenas e
beats), **Roteiro** (o texto) e **Propriedades** (título, idiomas, elenco, Fountain). O MCP tem `aiai_script` e
`aiai_beats`.

Referência: **Final Draft, Celtx, WriterDuet, Highland** (Fountain) e o roteiro de anime (脚本, com a numeração de cena
que o storyboard herda).

## 1. Regras de convivência (as do WS-R00)

- Só o seu projeto de teste: `C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-roteiro.aiai` (uma cópia de um episódio real,
  com roteiro, personagens e vozes; pode estragar). Nunca `producoes\`.
- Não edite código, não rode `cargo` nem `git` que altere. App **sem** `--bridge`; feche só o seu PID.
- Não ligue o Copiloto (não faz parte desta rodada).

Abrir:

```
& "C:\AnimeIAI\app\target\release\aiai-studio.exe" --no-session --project "C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-roteiro.aiai" --workspace roteiro
```

## 2. Profundidade (R23 §2)

Auditoria completa, não a busca do próximo defeito: dezenas de itens verificados, todos de uma vez, cada um com id
(`WS1-01`…), gravidade (**bloqueia** / **não bloqueia** / **melhoria**), área, prova (arquivo:linha ou passos no app com o
esperado e o que aconteceu) e correção esperada; mais a cobertura declarada por área. Fim: "NÍVEL PROFISSIONAL: SIM",
ou "NÃO — bloqueios: …".

## 3. O que auditar (mínimo; vá além)

**A. Escrever um roteiro do zero como um roteirista profissional.** Só com o teclado: criar cena, ação, fala,
parentético, transição; trocar o tipo; alternar personagens numa conversa; desfazer e refazer; salvar e reabrir. Compare
com o Final Draft (Tab/Enter) e diga o que atrapalha.

**B. Estrutura.** Cabeçalho de cena (INT./EXT., local, hora), numeração, mover e apagar cenas e elementos, cenas vazias,
roteiro com 100 cenas (desempenho), vários roteiros no projeto.

**C. Tradução e idiomas.** Texto original × tradução, idiomas (o que acontece com "JP" ou "japonês"), acentos, japonês
(kana/kanji), direita-para-esquerda não precisa.

**D. Falas, personagens e voz.** Fala ligada ao personagem × só com o nome, elenco (falas e segundos), emoção e
direção, a ponte para o TTS do workspace Áudio (gerar a fala a partir do roteiro), o que muda no roteiro depois da fala
gerada.

**E. Tempo.** Segundos estimados por elemento e por cena, "aplicar a estimativa", comparação com o animatic/cortes.

**F. Esboço e beats.** Cenas no Esboço, quadro de beats (atos, cartões, metas de tempo), ida e volta com o roteiro.

**G. Fountain e exportação.** Exportar e importar (ida e volta sem perda: acentos, japonês, parentéticos, transições,
notas, cenas forçadas com "."), o que falta (PDF formatado, roteiro de dublagem por personagem).

**H. Ligação com o resto da produção.** Roteiro → storyboard (`aiai_cuts_from_storyboard`), cenas ↔ cortes, lista de
cenários a partir das cenas, revisões depois de travar.

**I. Interface.** Organização, nomes, dicas, estados vazios, tema claro e escuro, pt e en, janela pequena, rolagem com o
elemento novo, foco do teclado.

**J. MCP.** `aiai_script` e `aiai_beats`: um agente consegue escrever e corrigir um roteiro inteiro? Erros e validação.

**K. Código.** `aiai-studio/src/app/panels/script.rs`, `outline.rs`, `properties.rs` (parte do roteiro),
`aiai-tools/src/preprod.rs` (ScriptOp, BeatOp, Fountain), `aiai-bridge/src/preprod_tools.rs`, `aiai-mcp/src/tools_preprod.rs`
e os testes `aiai-tools/tests/preprod.rs`.

## 4. O que entregar

1. o commit e o executável usado (tamanho, data, SHA-256);
2. a tabela de achados, sem limite de quantidade;
3. a cobertura por área (A–K);
4. o que falta para o nível de um programa de roteiro profissional, separando "agora" de "depois";
5. o veredito.
