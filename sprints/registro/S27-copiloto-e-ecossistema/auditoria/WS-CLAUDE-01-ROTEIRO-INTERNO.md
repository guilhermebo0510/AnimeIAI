# WS-CLAUDE-01 — Roteiro: auditoria interna (leitura de código, antes da rodada com o Codex)

> Feita em 30/10 de madrugada, enquanto o Codex fazia a rodada 2 do Copiloto (sem app e sem GPU). Base: `1beb5ee`.
> Arquivos: `aiai-studio/src/app/panels/script.rs` (515 linhas), `outline.rs`, `properties.rs`, `aiai-tools/src/preprod.rs`
> (ScriptOp, BeatOp, Fountain). Falta o uso no app: entra na rodada WS-R01.

| Id | Gravidade | Achado | Prova | Correção esperada |
|---|---|---|---|---|
| I-R01 | bloqueia (fluxo) | O roteiro é editado como **cartões de formulário** (tipo e personagem em listas, texto em caixa), não como **página de roteiro** (cabeçalho de cena, nome do personagem centralizado, fala recuada). Quem escreve no Final Draft, no Celtx ou no WriterDuet digita sem tirar a mão do teclado: Tab troca o tipo do elemento, Enter cria o próximo. Aqui só Ctrl+Enter cria o próximo; trocar o tipo exige o mouse. | `script.rs:162-293` (cartão), `:298` (só Ctrl+Enter) | Modo **Página** (padrão) com a formatação de roteiro e o teclado do Final Draft (Tab cicla o tipo, Enter cria o próximo pelo `next_element`, que já existe); o modo cartões fica para tradução, emoção e direção. |
| I-R02 | bloqueia (produção) | O cabeçalho de cena é **texto livre**: não há INT./EXT., **local** nem **hora do dia**. Sem isso não sai a lista de cenários (BG) nem a quebra de produção (cena → cortes → fundos), que é o elo do roteiro com o Storyboard, o Layout e o Cenário. | `preprod.rs:298` `UpdateScene { scene, heading }`; `ScriptScene { id, heading, elements }` | Campos `place` (INT/EXT), `location` (ligado a um cenário da Biblioteca) e `time` (dia/noite/…), lidos do cabeçalho Fountain ao importar; uma lista de locais no painel. |
| I-R03 | não bloqueia | Idiomas do roteiro e da tradução são **texto livre** ("ja", "pt-BR"): um "JP" ou "japonês" passa e quebra a voz (TTS) e a legenda. | `script.rs:366-378` | Lista de idiomas (BCP-47) com o nome legível. |
| I-R04 | não bloqueia | Sem **busca e substituição** no roteiro (trocar o nome de um personagem, achar uma fala). | `script.rs` inteiro | Ctrl+F e Ctrl+H no painel, também nas traduções. |
| I-R05 | não bloqueia | Sem **revisões** (rascunho, 決定稿/versão final, páginas coloridas de revisão) nem bloqueio de numeração de cena: depois do storyboard, mudar o roteiro desloca tudo sem aviso. | `ScriptDoc` não tem estado de revisão | Estado da revisão e numeração travada; marcar o que mudou desde a versão travada. |
| I-R06 | não bloqueia | A fala com `speaker` (texto) sem `character` (ligado) aparece com o nome, mas não entra no elenco ligado nem na voz do personagem. O painel não avisa. | `script.rs:183`, `:401` | Aviso "fala sem personagem ligado" com um botão para ligar (ou criar o personagem). |
| I-R07 | melhoria | Exportar só em Fountain. Falta o **PDF formatado** (o que se manda para o diretor e para os dubladores) e o roteiro de dublagem (só as falas, por personagem, com o tempo). | `poll_fountain` | PDF de página de roteiro e "roteiro por personagem". |
| I-R08 | melhoria | Os segundos estimados por elemento são bons para anime, mas não há **meta** por cena nem comparação com o animatic (o tempo real dos cortes). | `script.rs:213-226`, `properties` | Coluna "no animatic" ao lado de "estimado", com a diferença. |
| I-R09 | melhoria | A cena não sabe **quais cortes** a contam (ida e volta roteiro ↔ storyboard ↔ corte). `aiai_cuts_from_storyboard` cria cortes, mas o roteiro não mostra os cortes da cena. | `ScriptScene` sem ligação | Mostrar os cortes da cena e abrir o corte com um clique. |

**Para a rodada WS-R01 com o Codex:** conferir estes nove no app, e o que eu não li (Esboço/beats em `outline.rs`,
Propriedades, Áudio/TTS a partir da fala, desfazer e salvar, Fountain de ida e volta com acentos e japonês).
