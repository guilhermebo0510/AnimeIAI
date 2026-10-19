# Estado atual da execução

> Atualizar antes de qualquer operação longa e ao fim de cada fatia. Depois de um resumo de contexto ou de uma retomada, ler este arquivo primeiro.

## Agora

- **Fase:** execução autônoma iniciada em 2026-10-15 22:50 (usuário liberou).
- **Sprint atual:** produto (plano em `sprints/registro/PLANO-PRODUTO.md`). Calibração: **corte D (#218) aprovado** na régua perceptual (decisão 25) com 86/87 quadros (98,9%; q82 só espessura/paleta), resumo em `registro/calibracao/D218/aprovado/r25_resumo.txt`. Cortes A (#138) e C (#131) pausados (C: q15 F1 0,864 a 4 px; A: q12 F1 0,808 na régua antiga); não retomar sem nova orientação.
- **Pendências sugeridas pelo B2:** perfil dev com opt-level 3 para aiai-render/aiai-paint; escala em `RenderOptions` (reprodução); taper da G-pen contado no `gap` do balde; arrastar para reordenar camadas; edição de pontos.
- **Fatia atual (2026-10-16, orientação do usuário):** calibração pixel a pixel encerrada como frente principal; foco no produto. **P1 feito** (S16 no MCP: aiai_script, aiai_beats, aiai_storyboard, aiai_animatic, aiai_cuts_from_storyboard) e **P2 feito** (S19 no MCP: aiai_production, aiai_correction, aiai_retake) e **P3 feito** (satsuei/câmera/cor tipados: aiai_effects, aiai_grade, aiai_camera, aiai_layer_motion; multiplano conferido no render) e **P4 feito** (prévia/entrega: aiai_render_sheet, aiai_edit_preview, aiai_edit_mix, aiai_tts_regenerate, .srt de entrega) — 63 ferramentas MCP. P5 micro anime (S22) **feito**; próxima: P6 interface (S20).
- **Próximo passo concreto:** P6 interface (S20; plano de fatias na seção Execução de `sprints/S20-UI-UX-IDENTIDADE.md`). **P6a feita** (Roteiro e Storyboard reais: `app/actions.rs` chama as ações da bridge no processo, autor `human`; painéis Estrutura/Roteiro/Cenas/Prancha + propriedades; evidencias/S20/p6a_*.png). **P6b feita** (Produção: Episódios/Tabela de cortes/Propriedades do corte via PRODUCTION/CORRECTION/RETAKE; evidencias/S20/p6b_*.png). P6c: **Edição feita** (`panels/edit.rs`: Mídia/Monitor/Linha do tempo/Propriedades via EDIT_SET). **Entrega feita** (`panels/deliver.rs`: export em segundo plano numa cópia da sessão) e **Áudio feito** (`panels/audio.rs`: mixer por trilha, loudness, WAV do corte e da edição). **P6c concluída.** **P6d feita** (`panels/satsuei.rs`: efeitos da camada, câmera, grade do corte; Navegador do Cor = monitor). P6e: **prancha feita** (`tools/ui/prancha.py`; evidencias/S20/prancha/). **P6f feita** (Personagens: lista/folha de modelo/ficha com voz, paleta, MODEL_SHEET e CONSISTENCY; Navegador dos workspaces de canvas; evidencias/S20/p6f_*.png; lacunas 20–22). **P6e varredura de estouro feita** (`GET_UI_STATE.overflow`, `set_window_size`, `tools/ui/estouro.py`: 192 combinações PT/EN × 4 larguras, 0 estouros depois das correções em Projeto e barra do canvas; evidencias/S20/estouro/; lacuna 23). **P6e roteiro de mouse real feito (2026-10-19)**: `tools/ui/mouse_roteiro.py` (SendInput + vigia WH_MOUSE_LL) — 57 passos, **0 falhas**, 0 eventos físicos: 13 abas, 9 botões de ferramenta, 9 atalhos (V B P E G L I H Z), traço da caneta, Ctrl+Z/Ctrl+Y conferidos por pixels do canvas, pan espaço+arrasto e mão, roda, enquadrar, árvore abre corte, setas/Home, 5 menus abrem e fecham com Esc, tela inicial; evidencias/S20/mouse/ (relatorio.json, ws_*.png, canvas_*.png, menu_*_aberto.png, inicio_*.png). Corrigido no caminho: Esc nos menus (lacuna 24), log de pânico em arquivo (lacuna 25), falso "janela oculta" da bridge em quadro longo (lacuna 26). **P6g tela inicial feita**: `app/start.rs` (Arquivo > Início; abre ao iniciar sem --project fora de --no-session): projeto novo com nome, 5 formatos (Full HD, 4K, 720p, vertical, quadrado) e 5 taxas (23,976/24/25/29,97/30) por NEW_PROJECT com settings; Recentes (prefs.recent, 10, some/desabilita se a pasta sumiu, aviso de alterações antes de abrir); "Mostrar ao abrir". **P6h preferências e atalhos feitos**: `keymap.rs` (23 ações, padrão CSP/Photoshop, texto "Ctrl+Shift+Z" nas prefs, conflito = a tecla sai da outra ação com aviso), atalhos do editor/documento/menus/dicas da prateleira pelo mapa, Preferências em abas verticais (Geral, Atalhos com captura de tecla, Pastas com pasta de projetos dos diálogos, Desempenho com GPU dedicada/integrada lida no main antes da janela); mouse real 10/10 (evidencias/S20/mouse/prefs_*.png, dica_balde_K.png, relatorio_prefs.json); 0 estouros PT/EN 1024x680 e maximizada. Motor de voz nas preferências **não** foi feito: nada no app escolhe motor por padrão (o MCP escolhe por chamada); entra quando houver TTS pela UI. **Contraste AA feito** (teste ampliado, 3 tokens ajustados, token `danger`, 4 cores soltas → tokens; evidencias/S20/contraste/). Próximas: auditoria de logomarca/ícones (grade 24 px, zero provisório); depois fechar a S20 (prancha final) e ir para a S23. Obs.: cor opcional `null` de efeito só pelo MCP, P6d composição/cor, P6e prancha PT/EN×tema e mouse real. App: `target/debug/aiai-studio.exe` (relanca.ps1). Depois: S23.
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
| S20 UI/UX e identidade | em andamento | evidencias/S20 | P6a (roteiro/storyboard), P6b (produção), P6c (edição, entrega, áudio), P6d (composição, cor), P6f (personagens, navegador), prancha e varredura de estouro (0 em 192 combinações) feitas; nenhum painel vazio |
| S21 Calibração (A, B, C obrigatórios; D, E meta) | pausada (frente secundária) | registro/calibracao/D218, C131 | D aprovado (decisão 25, 86/87); C q15 F1 0,864 a 4 px; A q12 |
| S22 Micro anime | concluída (2026-10-17) | evidencias/S22 | 「最後の一輪」 33,2 s; aceite §7 com evidência; lacunas 8, 10, 11, 14, 15 abertas |
| S23 Endurecimento e apresentação | pendente | — | |

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
