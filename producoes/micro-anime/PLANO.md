# Micro anime — 「最後の一輪」 (A última rosa)

Curta original de ~35 s produzido dentro do AnimeIAI pelo MCP (S22). Nenhuma imagem gerada por IA, nenhum material do
episódio de referência. Vozes VOICEVOX (créditos no fim), legendas em português.

## História

Fim de tarde num jardim. Hana (jardineira, 16) e Ren (aprendiz de confeiteiro, 17) estendem a mão para a mesma rosa — a
última da estação. Discutem: cada um quer levá-la para alguém. Descobrem que é a mesma pessoa: a avó que cuidou dos
dois. Riem e decidem entregá-la juntos; o vento leva pétalas para o céu.

## Personagens

| | Hana | Ren |
|---|---|---|
| Visual | cabelo castanho-avermelhado curto com presilha verde, avental de jardinagem verde-musgo, olhos verdes | cabelo preto bagunçado, bandana branca, avental creme de confeiteiro, olhos âmbar |
| Altura | 5,5 cabeças | 6 cabeças |
| Voz | VOICEVOX, voz feminina jovem (definir no teste de vozes) | VOICEVOX, voz masculina jovem |
| Traço | determinada, impulsiva | gentil, teimoso |

## Cortes (storyboard)

| C | Plano | Ação | Fala (JA / PT) | s | Itens do aceite |
|---|---|---|---|---|---|
| 1 | EST | Jardim ao pôr do sol; câmera faz pan lateral até a roseira | — | 4,0 | cenário 1 (perspectiva), multiplano |
| 2 | MS | Duas mãos alcançam a mesma rosa e se tocam | (SE: farfalhar) | 3,0 | — |
| 3 | CU | Hana, brava | それ、私が先に見つけたの！ / Eu vi primeiro! | 3,0 | lip-sync, piscar |
| 4 | CU | Ren, firme | 僕だって、ばあちゃんに渡したいんだ。 / Eu também quero dar pra vovó. | 4,0 | lip-sync |
| 5 | MS | Os dois, choque cômico: fundo de gag, linhas de foco, tremor | …ばあちゃん？ / …Vovó? | 2,5 | efeito gag, tremor |
| 6 | MS | Varanda da casa; Hana vira o rosto para Ren e sorri (animação de verdade, ≥ 8 desenhos) | じゃあ、一緒に渡そう。 / Então vamos entregar juntos. | 5,0 | cenário 2 (guia 3D), animação, lip-sync |
| 7 | WIDE | Vento; pétalas sobem; câmera inclina para o céu | — | 5,0 | cenário 3 (céu), partículas, glow/difusão |
| — | — | Créditos | — | 5,0 | créditos de voz, "produzido no AnimeIAI" |

Total ≈ 31,5 s de cortes + 5 s de créditos.

## Pipeline (ferramentas)

1. Pré-produção: bíblia (`aiai_apply SetProjectProps`), `aiai_beats`, `aiai_script` (JA + PT, `apply_timing`), personagens
   (`create_node character`), `aiai_model_sheet` + desenho dos turnarounds, `aiai_voice_profile` + teste de falas.
2. `aiai_storyboard` (painéis desenhados), `aiai_animatic`, `aiai_cuts_from_storyboard`.
3. Cenários: `aiai_guide3d`/`aiai_perspective` → linhas → `aiai_fill_shape`/`aiai_paint`/`aiai_pattern_fill`/`aiai_sky`.
4. Por corte: layout/genga (`aiai_draw_curves`), douga (`aiai_inbetween`), shiage (`aiai_fill`, `aiai_propagate_fills`,
   `aiai_production lint_cut`), `aiai_tts_line` + `aiai_lipsync`, satsuei (`aiai_camera`, `aiai_layer_motion`,
   `aiai_effects`, `aiai_particles`), `aiai_grade`; conferência com `aiai_render_sheet`.
5. Som: `aiai_sfx`, `aiai_bgm`, `aiai_mix`; edição `aiai_edit` (legendas PT, créditos), `aiai_edit_preview`,
   `aiai_edit_mix`, `aiai_export_video` (`preset: arib`, `sidecar_srt`).
6. Revisão: `aiai_production` (cut_table, lint_cut, report), `aiai_consistency`, retakes.

Cada ferramenta que faltar ou falhar vira entrada em `sprints/registro/LACUNAS.md` e é corrigida na hora.
