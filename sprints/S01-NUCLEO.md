# S01 — Núcleo do documento, Command Bus, undo, persistência

## Objetivo
`aiai-core` com o modelo do ADR-002, Command Bus, undo/redo, consultas, serialização canônica, salvar/abrir pasta `.aiai`, WAL de recuperação e esquema autodescrito dos comandos. Sem GPU, sem UI.

## Tarefas
- [ ] Tipos: Project, Settings, Bible, Character, ColorSet, Script, Storyboard, Episode, Scene, Cut, XSheet (trilhas, células), LayerTree (todos os tipos de camada do ADR-002 com propriedades comuns), Cel, Camera, Asset (texture, brush, palette, lut, audio), EditTimeline. Campos que ainda não têm ferramenta existem como estrutura mínima, não como `todo!()`.
- [ ] Ids UUID v7, revisões, proveniência.
- [ ] `Command` (enum com `serde(tag="kind")`), validação de domínio, `CoreSession::apply → Outcome`, inverso gerado no apply, `batch` atômico, `if_revision` (concorrência otimista).
- [ ] `Query` (documento, nó por id, árvore de camadas, timesheet, paleta, log, schema).
- [ ] `command_schema`: JSON Schema de todos os comandos gerado com `schemars`.
- [ ] Persistência: `project.json` canônico (chaves ordenadas, f32 com casas fixas), `blobs/` por BLAKE3, WAL `wal.ndjson` com recuperação ao abrir, salvar atômico (escreve em temporário e renomeia), conferência de espaço em disco antes de salvar.
- [ ] Relógio e RNG injetáveis (`SessionClock`, `SeededRng`).
- [ ] Log da sessão (não perde entradas com undo).
- [ ] Comandos iniciais: criar projeto/episódio/cena/corte, criar/renomear/mover/apagar camada, propriedades de camada, criar cel, editar célula do timesheet, criar personagem/paleta/cor nomeada.

## Testes
- Unitários por comando: apply → inverso → estado idêntico (proptest em sequências aleatórias de comandos).
- Ida e volta: salvar → abrir → JSON byte a byte idêntico.
- Recuperação: matar no meio (simulado) → WAL restaura.
- Fuzz leve do parser de comando (cargo-fuzz se couber no disco; senão proptest com JSON aleatório).

## Critério de pronto
- [ ] ≥ 95% dos comandos com teste de inverso; proptest de 10 mil sequências sem divergência.
- [ ] Projeto de exemplo salvo em `producoes/exemplo-s01.aiai` e diff legível em git após uma edição.
- [ ] Schema exportado em `registro/evidencias/S01/command_schema.json`.
