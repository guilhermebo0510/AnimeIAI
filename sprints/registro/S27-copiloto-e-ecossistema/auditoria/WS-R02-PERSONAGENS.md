# WS-R02 — Workspace Personagens: auditoria completa (rodada 1)

> **Host:** Claude (o único que implementa) · **Revisor:** Codex (ChatGPT) · **Protocolo:** `WS-R00-PROTOCOLO.md` (leia antes).
> **Entrega:** `sprints\registro\S27-copiloto-e-ecossistema\auditoria\WS-CX02-PERSONAGENS.md`
> **Código:** commit `{{COMMIT}}` · release recompilado a partir dele.

## 0. O que é o workspace

Painéis: Personagens (lista com paleta, cabeças e voz), Pasta do personagem (materiais), Folha de modelo, canvas, Camadas e Propriedades (abas Ficha, Paleta, Consistência, Voz, Material). Referência: **folhas de modelo de estúdio (settei), Clip Studio (material), VOICEVOX/Kokoro (voz)**. Foco de produção: o personagem como base do programa inteiro: design sheet, paleta de settei por parte, voz, consistência; é a base da geração controlada pelo personagem da fase 4. Leia também a minha auditoria interna, feita antes desta rodada: `WS-CLAUDE-02-PERSONAGENS-INTERNO.md` (só leitura de código; os itens dela são hipóteses a conferir).

## 1. Regras de convivência (as do WS-R00)

- Só o seu projeto de teste: `C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-personagens.aiai` (uma cópia de um episódio real;
  pode estragar). Nunca `producoes\`.
- Não edite código, não rode `cargo` nem `git` que altere. App **sem** `--bridge`; feche só o seu PID.
- Um só Copiloto na GPU (não faz parte desta rodada; não ligue).

Abrir:

```
& "C:\AnimeIAI\app\target\release\aiai-studio.exe" --no-session --project "C:\AnimeIAI\calibracao\tmp\revisao-codex-ws-personagens.aiai" --workspace personagens
```

## 2. Profundidade (R23 §2)

Auditoria completa, não a busca do próximo defeito: dezenas de itens verificados, todos de uma vez, cada um com id
(`WS2-01`…), gravidade (**bloqueia** / **não bloqueia** / **melhoria**), área, prova (arquivo:linha ou passos no app
com o esperado e o que aconteceu) e correção esperada; mais a cobertura declarada por área. Fim: "NÍVEL PROFISSIONAL:
SIM", ou "NÃO — bloqueios: …".

## 3. O que auditar (mínimo; vá além)

**A.** Criar um personagem do zero até a folha de modelo, a paleta de settei e a voz; usar o personagem no roteiro (fala), no áudio (TTS) e na animação (referência).

**B.** Materiais: importar imagens, renomear, abrir na Animação e voltar, apagar; o que acontece com o corte da folha.

**C.** Paleta de settei: cores por parte, variantes (normal, sombra, luz, noite); ligação com as Cores e a Pintura.

**D.** Consistência: medir um desenho contra a folha (pegar no canvas e "Do desenho"); o que ela mede e o que não mede.

**E.** Voz: escolher falante, ajustes, ouvir; créditos do VOICEVOX; Kokoro.

**F.** Integridade: apagar um personagem usado em falas, cortes e materiais.

**Comum a todo workspace (WS-R00):** cada botão, menu, atalho e painel; usos cruzados com os outros workspaces;
interface (organização, nomes, dicas, estados vazios, tema claro e escuro, pt e en, janela pequena, ícones sem emoji);
robustez (desfazer e refazer, salvar e abrir, projeto grande, entrada inválida, interromper no meio); desempenho com
números; o que falta comparado à referência, priorizado pelo que é essencial para produzir anime.

**MCP:** aiai_model_sheet, aiai_character_material, aiai_voice_profile, aiai_consistency: um agente consegue fazer o trabalho do workspace inteiro? Erros e validação.

**Código:** panels/characters.rs, char_folder.rs, char_ref.rs; aiai-tools character.rs, char_material.rs (caminhos a partir de `app/crates/aiai-studio/src/app/` quando não indicado).

## 4. O que entregar

1. o commit e o executável usado (tamanho, data, SHA-256);
2. a tabela de achados, sem limite de quantidade;
3. a cobertura por área;
4. o que falta para o nível do programa de referência, separando "agora" de "depois";
5. o veredito.
