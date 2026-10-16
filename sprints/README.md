# AnimeIAI — base de sprints

Programa único de produção de anime, do roteiro à entrega, operado por IA (Claude) através de um servidor MCP próprio e, quando necessário, por mouse e teclado reais. **Não usa geração de imagem**: todo desenho, cenário, pintura e efeito é feito com as ferramentas do programa, como um estúdio faria (ver `01-VISAO-PRODUTO.md`).

## Ordem de leitura (obrigatória para quem executa)

| # | Arquivo | Para quê |
|---|---|---|
| 1 | `00-RUNBOOK-AUTONOMO.md` | Como executar a rodada inteira sozinho: laço, retomada, quando parar |
| 2 | `00-REGRAS-INVIOLAVEIS.md` | O que nunca pode acontecer |
| 3 | `00-LICOES-DE-IMPLEMENTACAO.md` | Erros já pagos neste PC (disco, link, harness, MCP). Leitura bloqueante antes de cada sprint |
| 4 | `00-ESTADO-ATUAL.md` | Onde a execução está agora. Atualizar a cada passo |
| 5 | `01-VISAO-PRODUTO.md` | Produto, pipeline, princípios |
| 6 | `02-ADR-001-STACK.md` | Stack e por que mudou em relação à pesquisa |
| 7 | `03-ADR-002-MODELO-DE-DADOS.md` | Projeto → episódio → cena → corte → camadas/cels/timesheet; Command Bus |
| 8 | `04-ADR-003-MOTOR-DE-DESENHO.md` | Vetor + raster + preenchimento + texturas + render de referência |
| 9 | `05-ADR-004-MCP-BRIDGE-HARNESS.md` | Como a IA enxerga e opera o programa |
| 10 | `06-MAPA-DE-FERRAMENTAS.md` | Catálogo completo de ferramentas por fase, com referência de mercado e sprint |
| 11 | `07-PROTOCOLO-DE-VALIDACAO.md` | A régua dos 5%, níveis de calibração, escolha das cenas, aceite do micro anime |
| 12 | `08-FLUXO-DE-PRODUCAO-NO-APP.md` | Como um corte é produzido de ponta a ponta pelo Claude dentro do app (o roteiro do teste final) |
| 13 | `S00` … `S23` | As sprints, em ordem |
| — | `registro/` | Lacunas descobertas, decisões tomadas durante a execução, relatórios de calibração |

## Sprints

| Sprint | Tema | Bloco |
|---|---|---|
| S00 | Fundação do workspace, janela, tema, i18n, guardas de disco | Fundação |
| S01 | Núcleo do documento, Command Bus, undo, persistência | Fundação |
| S02 | Bridge, servidor MCP, harness de mouse, render headless | Fundação (MCP primeiro) |
| S03 | Motor de canvas e camadas, compositor de referência, golden tests | Motor |
| S04 | O "olho" do agente: referência, mesa de luz, métricas, diff | Motor |
| S05 | Desenho vetorial (referência: Clip Studio) | Desenho |
| S06 | Pincéis raster, pintura e texturas de pincel (referência: Photoshop/CSP) | Desenho |
| S07 | Preenchimento, tinta-e-pinta, paletas indexadas | Desenho |
| S08 | Seleção, transformação, deformação | Desenho |
| S09 | Réguas, grades espaciais/angulares, perspectiva, guias 3D | Layout |
| S10 | Personagens: model sheet, turnaround, expressões, settei de cor | Pré-produção visual |
| S11 | Cenários: texturas, pincéis especiais, BOOK/parallax, céu, luz | Cenário |
| S12 | Animação: timeline, timesheet, cels, onion skin, intercalação | Animação |
| S13 | Câmera e composição (referência: After Effects) | Satsuei |
| S14 | Partículas e efeitos 2D | Satsuei |
| S15 | Cor: correção, LUT, grading, scopes | Satsuei |
| S16 | Pré-produção: roteiro, storyboard/e-conte, animatic, lista de cortes | Pré-produção |
| S17 | Áudio: vozes TTS locais, SFX, BGM, mixagem, loudness, lip-sync | Som |
| S18 | Edição e entrega: montagem, legendas, render, FFmpeg | Entrega |
| S19 | Produção e revisão: tracker de cortes, correção, retakes, proveniência | Gestão |
| S20 | UI/UX profissional: identidade, ícones, workspaces, atalhos | Produto |
| S21 | Calibração contra o episódio de referência (mín. 3 cortes, meta 5) | Validação |
| S22 | Micro anime original de 30–60 s | Validação final |
| S23 | Endurecimento, ensaio do zero e apresentação | Entrega ao usuário |

As sprints não são uma cascata rígida: a S21 devolve lacunas para as sprints de ferramenta (`registro/LACUNAS.md`) e o ciclo se repete até a régua passar.
