# Estado atual da execução

> Atualizar antes de qualquer operação longa e ao fim de cada fatia. Depois de um resumo de contexto ou de uma retomada, ler este arquivo primeiro.

## Agora

- **Fase:** execução autônoma iniciada em 2026-10-15 22:50 (usuário liberou).
- **Sprint atual:** produto (plano em `sprints/registro/PLANO-PRODUTO.md`). Calibração: **corte D (#218) aprovado** na régua perceptual (decisão 25) com 86/87 quadros (98,9%; q82 só espessura/paleta), resumo em `registro/calibracao/D218/aprovado/r25_resumo.txt`. Cortes A (#138) e C (#131) pausados (C: q15 F1 0,864 a 4 px; A: q12 F1 0,808 na régua antiga); não retomar sem nova orientação.
- **Pendências sugeridas pelo B2:** perfil dev com opt-level 3 para aiai-render/aiai-paint; escala em `RenderOptions` (reprodução); taper da G-pen contado no `gap` do balde; arrastar para reordenar camadas; edição de pontos.
- **Fatia atual (2026-10-16, orientação do usuário):** calibração pixel a pixel encerrada como frente principal; foco no produto. **P1 feito** (S16 no MCP: aiai_script, aiai_beats, aiai_storyboard, aiai_animatic, aiai_cuts_from_storyboard) e **P2 feito** (S19 no MCP: aiai_production, aiai_correction, aiai_retake) e **P3 feito** (satsuei/câmera/cor tipados: aiai_effects, aiai_grade, aiai_camera, aiai_layer_motion; multiplano conferido no render) e **P4 feito** (prévia/entrega: aiai_render_sheet, aiai_edit_preview, aiai_edit_mix, aiai_tts_regenerate, .srt de entrega) — 63 ferramentas MCP. P5 micro anime (S22) **feito**; próxima: P6 interface (S20).
- **S23 em andamento (2026-10-19):** auditoria externa `AUDITORIA.md` virou o backlog `registro/S23-AUDITORIA-BACKLOG.md`. **P0 feito** (balde, lint, persistência/autosave, prévia da caneta, camada com movimento, pincéis, vetor nítido no zoom; 1.6 não reproduziu, conferido com mouse real). **Curta repintado sem o remendo** (lint 0 com a régua nova por sementes explícitas + `close_leftovers`; consistência medida contra o model sheet; MP4 reexportado). **P1 feito** (desempenho 4K, tamanho do corte, curva de pressão, som ao vivo, forma de onda, Play na Edição, importar WAV/imagem, exportar quadro/PSD/XDTS, folha de e-conte, miniaturas/ajustes de pincel, borracha vetorial por trecho/interseção, seletor de cor, linha/curva/forma e edição de nós na caneta, varinha, alças do Mover, Sub View; 68 ferramentas MCP). Regressão achada no teste (cabelo da Hana em 3/4 cor de pele) corrigida na raiz no FILL (`skip_seeds_on_line`, `only_unpainted`) e curta repintado. Pendentes do P1 que pedem hardware ou motor novo: WinTab/inclinação (mesa física), Rosas/Nuvem (carimbo bicolor), OTIO/SVG. **Próximo:** S23 original (licenças, docs, ensaio do zero, release, relatório).
- **Próximo passo concreto (plano anterior da S23, segue valendo depois do backlog):** **S23** (`sprints/S23-ENDURECIMENTO-APRESENTACAO.md`). Começar pelo que não exige build pesado: (1) revisão de licenças (`cargo about`/`cargo deny` instalados com cargo install só se houver ≥ 8 GiB livres; senão `cargo metadata` + script Python que lista licença por crate) e dos modelos/vozes/FFmpeg; (2) `docs/` do usuário; (3) desempenho medido (fps do canvas, render por quadro) com o app atual. Depois o ensaio do zero pela interface com mouse (projeto novo → corte curto → export) e o build release do zero, que precisa de disco: medir antes (hoje ~8,0 GiB livres; o release/ ocupa ~0,6 GB); se não couber, limpar `target/debug` inteiro antes do release (o debug se reconstrói depois). Relatório final e página de apresentação por último; apagar a tarefa agendada ao concluir. **S20 concluída em 2026-10-19**: P6a–P6h, roteiro de mouse real 57+10 passos sem falha, tela inicial, preferências/atalhos, contraste AA, marca final, prancha final e 0 estouros em 192 combinações.
- **S22 concluída (2026-10-17):** micro anime 「最後の一輪」 em `producoes/micro-anime/` (MP4 33,2 s, SRT, WAV −24 LKFS, projeto, `DIARIO.md`, `revisao.json`); scripts `tools/curta/c00..c21`; evidências `registro/evidencias/S22/`.
- **Bloqueios:** nenhum. Obs.: `aiai-core` mudou (Ease aceita texto); `aiai-render` ganhou `layer_motion` (balde usa cel cru); `aiai-mcp` release recompilado, o `aiai-studio` ainda não (recompilar antes de abrir o app).
- **Disco C: livre na última medição:** 43 GB (2026-10-15).

## Painel das sprints

| Sprint | Estado | Evidências | Observações |
|---|---|---|---|
| S00 Fundação | concluída (2026-10-15) | evidencias/S00 (4 capturas) | shell egui, tema, i18n, ícones, prefs; teste de mouse feito pelo agente |
| S01 Núcleo | concluída (2026-10-15) | evidencias/S01/command_schema.json | 8 testes + proptest 10k; exemplo .aiai será gerado pela CLI da S02 |
| S02 Bridge/MCP/harness | em andamento | evidencias/S02 (headless, ao vivo, mouse real) | A e B1 concluídas (21 ferramentas MCP, bridge no app, harness SendInput validado); B3 (demais ferramentas) em andamento |
| S03 Canvas e camadas | feito (B2) | evidencias/S03/ui/ | editor por mouse: canvas pan/zoom/rotação, ferramentas, painéis Projeto/Camadas/Cores/Timeline/Propriedades; 60+1 testes; mouse real sem desalinhamento |
| S04 Olho do agente | concluída (2026-10-16) | evidencias/S04 (catálogo, sanidade, candidatos) | 327 cortes; candidatos A#138 B#245 C#131 D#218 E#292; piso de ruído FLIP medido (decisão 20); COMPARE na bridge e no MCP |
| S05 Vetorial | em andamento | evidencias/S07/rosto_linha_balde.png | motor de traço + curvas por MCP prontos; falta edição de pontos/borracha vetorial e UI |
| S06 Raster e texturas de pincel | em andamento | evidencias/S06/estudo_ceu.png | motor + 10 presets + operação; falta UI |
| S07 Preenchimento e paletas | em andamento | evidencias/S07 | balde com fechamento, paleta indexada, lint; falta UI |
| S08 Seleção e transformação | em andamento | evidencias/S08 | motor + operações; falta UI |
| S09 Réguas, grades, guias 3D | em andamento | evidencias/S09/layout_guia3d.png | guia 3D + régua de perspectiva; falta snap no traço e UI |
| S10 Personagens e settei | em andamento | evidencias/S10/model_sheet_guias.png | model sheet + turnaround do manequim + checagem de proporção; falta UI |
| S11 Cenários e texturas | em andamento | evidencias/S11/cenario_externo.png | 15 texturas, céu, padrão em perspectiva, pincéis de cenário; falta UI |
| S12 Animação e timesheet | em andamento | evidencias/S12/intercalacao_tira.png | keyframes, intercalação, propagação do balde; falta UI timeline/timesheet e playback |
| S13 Câmera e composição | em andamento | evidencias/S13/contact_sheet.png | câmera/multiplano + 25 efeitos implementados; falta graph editor e UI |
| S14 Partículas e FX 2D | em andamento | evidencias/S14/corte_gag.png | partículas, onomatopeia, linhas, fundos de gag; falta UI |
| S15 Cor | em andamento | evidencias/S13 (grade, LUT, scopes) | grade, LUT .cube, scopes prontos; falta HSL qualificado e UI |
| S16 Pré-produção | em andamento | evidencias/S16 (e-conte, animatic) | motor pronto; no MCP desde 2026-10-16 (roteiro, beats, storyboard, animatic, cortes); falta UI |
| S17 Áudio | em andamento | evidencias/S17 (mix -24,0 LUFS, lip-sync) | motor, VOICEVOX, Kokoro, SFX, BGM, mix, loudness prontos; falta UI e saída ao vivo |
| S18 Edição e entrega | em andamento | evidencias/S18 | edição (transições, legendas, créditos) + export MP4 prontos; falta UI |
| S19 Produção e revisão | em andamento | evidencias/S19 | tracker, etapas, lint do corte, correções, retakes, relatório no MCP desde 2026-10-16; falta UI |
| S20 UI/UX e identidade | concluída (2026-10-19) | evidencias/S20 | P6a (roteiro/storyboard), P6b (produção), P6c (edição, entrega, áudio), P6d (composição, cor), P6f (personagens, navegador), prancha e varredura de estouro (0 em 192 combinações) feitas; nenhum painel vazio |
| S21 Calibração (A, B, C obrigatórios; D, E meta) | pausada (frente secundária) | registro/calibracao/D218, C131 | D aprovado (decisão 25, 86/87); C q15 F1 0,864 a 4 px; A q12 |
| S22 Micro anime | concluída (2026-10-17) | evidencias/S22 | 「最後の一輪」 33,2 s; aceite §7 com evidência; lacunas 8, 10, 11, 14, 15 abertas |
| S23 Endurecimento e apresentação | em andamento | evidencias/S23/auditoria | backlog da auditoria em registro/S23-AUDITORIA-BACKLOG.md; P0 feito |

Estados: pendente · em andamento · bloqueada (motivo) · concluída (data) · reaberta (lacuna nº).

## Calibração

| Corte | Eixo | N1 | N2 | Tentativas | Última métrica |
|---|---|---|---|---|---|
| — | A personagem | — | — | 0 | — |
| — | B cenário | — | n/a | 0 | — |
| — | C efeitos | — | n/a | 0 | — |
| #218 | D gag (meta) | aprovado (2026-10-16, decisão 25) | n/a | r25 | 86/87 quadros; F1 ~0,97 / SSIM ~0,96 |
| — | E completo (meta) | — | n/a | 0 | — |

## Medidas registradas

| Data | Medida | Valor |
|---|---|---|
| 2026-10-15 | Disco livre | 43 GB |
| 2026-10-16 | Disco livre | 33 GB (VOICEVOX 2,1 GB + modelos em sidecars/) |
| 2026-10-17 | Disco livre | 22 GB |
| 2026-10-17 03:30 | Disco livre | 17 GB |
| 2026-10-17 10:20 | Disco livre | 9,5 GB (pdbs limpos, 424 MB) |
| 2026-10-17 11:55 | Disco livre | 8,5 GB (target 6,95 GB) |
| 2026-10-19 00:30 | Disco livre | 8,1 GB (duplicatas de deps removidas: +4,2 GB; rebuild 37 min com builds do Smart Canvas em paralelo no PC) |
| 2026-10-17 08:10 | Disco livre | 9,5 GB (target 7,3 GB; limpar pdbs antes do próximo build se cair abaixo de 8) |
| 2026-10-16 | Piso de ruído FLIP | A#138 0,015–0,017; C#131 0,0065–0,0083; D#218 0,0047 |
