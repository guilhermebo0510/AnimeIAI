# Estado atual da execução

> Atualizar antes de qualquer operação longa e ao fim de cada fatia. Depois de um resumo de contexto ou de uma retomada, ler este arquivo primeiro.

## Agora

- **Fase:** execução autônoma iniciada em 2026-10-15 22:50 (usuário liberou).
- **Sprint atual:** S21 calibração, corte D (#218). Aprovados na régua da decisão 24: **desenhos 1–7** — o 7 (redesenho de tremor) em todos os seus quadros (q60–62, 69–71, 78–80) com o pipeline parametrizado por desenho (`D218_DES`, cels e trilhas nas camadas fixas), campo de deslocamento medido (`d218_campo.py`), escolha entre estimativas e releituras por varredura; repetições do 6 (q66/75/84) passam. **Desenho 8 pausado** (q63: F1 0,927 / SSIM 0,957 contra limites ~0,928–0,933 / 0,961 nos quadros 63/72/81): o atributo por elemento (`d218_atrib.py`) e o Canny replicado (`canny_pair.py`) mostram perda espalhada (gola, ponta da franja, faixa esquerda, peças do laço). Corte C (#131) em andamento no quadro 15 (desenho final): F1 0,19 → 0,792, SSIM 0,71 → 0,910, FLIP 0,54 → 0,213 (limites 0,923 / 0,944 / 0,240 — FLIP já passa). O que mais rendeu: fundo elíptico medido, balde com mediana por região e folga 8 (pétalas fechadas), satsuei medido pela borda (σ≈1), cache de medidas + escolha da variante de medição por linha pela régua (`c131_opt.py`, +0,08), fechos ocultos nas fronteiras de pétala sem tinta (`region_split.py`, `edge_grid.py`) e linhas finas lidas por varredura. Faltam ~0,13 de F1 e ~0,034 de SSIM; depois os desenhos 0–12 (moldura cresce). Desenho 8 do D: F1 0,9297 (passa no q63) mas SSIM 0,9578 < 0,9611 e F1 abaixo nos q72/81. Limites dos outros cortes medidos: A ≈ 0,956, B ≈ 0,92 (SSIM 0,82), E ≈ 0,94 de F1.
- **Pendências sugeridas pelo B2:** perfil dev com opt-level 3 para aiai-render/aiai-paint; escala em `RenderOptions` (reprodução); taper da G-pen contado no `gap` do balde; arrastar para reordenar camadas; edição de pontos.
- **Fatia atual:** (retomada 2026-10-16 18:27) passada 2 do `c131_opt.py` (corte C) rodando desde 17:33 — não mexer em `c131.aiai` até terminar (~19:50). Em paralelo, **corte A (#138) iniciado** no quadro 12 (`a138_bg.py`, `a138_orn.py`, `a138_char.py`): fundo por faixas ajustadas (o fundo não tem borda na régua, lição 53), ~170 linhas digitadas + paquímetro (olhos sem medição), fechos ocultos, balde em 2 passadas com rótulos por watershed. q12: F1 0,747 / SSIM 0,878 / FLIP 0,2123 (limites 0,956 / 0,935 / 0,2124 — FLIP passa).
- **Próximo passo concreto:** (1) quando o `c131_opt.py` terminar: rodar c131_lines → c131_fill → c131_compare 15, commitar `c131_variantes.json` com a métrica; seguir nas fronteiras de pétala (edge_grid/region_split) até F1 0,923 / SSIM 0,944; depois desenhos 0–12. (2) corte A q12: escolher variante de medição por linha pela régua (como `c131_opt.py`), gola/babados, bordas do laço, friso das lapelas, brilhos do cabelo, faixa rosa da testa à direita, orelhas; ver `miss.py`-style (verde = falta) por célula. Depois quadros 0–35 (desenhos 0,3,6,9,12 + boca 21–27).
- **Bloqueios:** nenhum.
- **Disco C: livre na última medição:** 43 GB (2026-10-15).

## Painel das sprints

| Sprint | Estado | Evidências | Observações |
|---|---|---|---|
| S00 Fundação | concluída (2026-10-15) | evidencias/S00 (4 capturas) | shell egui, tema, i18n, ícones, prefs; teste de mouse feito pelo agente |
| S01 Núcleo | concluída (2026-10-15) | evidencias/S01/command_schema.json | 8 testes + proptest 10k; exemplo .aiai será gerado pela CLI da S02 |
| S02 Bridge/MCP/harness | em andamento | evidencias/S02 (headless, ao vivo, mouse real) | A e B1 concluídas (21 ferramentas MCP, bridge no app, harness SendInput validado); B3 (demais ferramentas) em andamento |
| S03 Canvas e camadas | feito (B2) | evidencias/S03/ui/ | editor por mouse: canvas pan/zoom/rotação, ferramentas, painéis Projeto/Camadas/Cores/Timeline/Propriedades; 60+1 testes; mouse real sem desalinhamento |
| S04 Olho do agente | concluída (2026-10-16) | evidencias/S04 (catálogo, sanidade, candidatos) | 327 cortes; candidatos A#138 B#245 C#131 D#218 E#292; piso de ruído FLIP medido (decisão 20); falta ação COMPARE na bridge |
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
| S16 Pré-produção | em andamento | evidencias/S16 (e-conte, animatic) | motor e ferramentas prontos; falta UI |
| S17 Áudio | em andamento | evidencias/S17 (mix -24,0 LUFS, lip-sync) | motor, VOICEVOX, Kokoro, SFX, BGM, mix, loudness prontos; falta UI e saída ao vivo |
| S18 Edição e entrega | em andamento | evidencias/S18 | edição (transições, legendas, créditos) + export MP4 prontos; falta UI |
| S19 Produção e revisão | em andamento | evidencias/S19 | tracker, correções, retakes, relatório prontos; falta UI |
| S20 UI/UX e identidade | pendente | — | |
| S21 Calibração (A, B, C obrigatórios; D, E meta) | em andamento | registro/calibracao/D218, C131 | D: desenhos 1–7 aprovados; 8 quase (SSIM); C: q15 F1 0,79 / SSIM 0,91 |
| S22 Micro anime | pendente | — | |
| S23 Endurecimento e apresentação | pendente | — | |

Estados: pendente · em andamento · bloqueada (motivo) · concluída (data) · reaberta (lacuna nº).

## Calibração

| Corte | Eixo | N1 | N2 | Tentativas | Última métrica |
|---|---|---|---|---|---|
| — | A personagem | — | — | 0 | — |
| — | B cenário | — | n/a | 0 | — |
| — | C efeitos | — | n/a | 0 | — |
| — | D gag (meta) | — | n/a | 0 | — |
| — | E completo (meta) | — | n/a | 0 | — |

## Medidas registradas

| Data | Medida | Valor |
|---|---|---|
| 2026-10-15 | Disco livre | 43 GB |
| 2026-10-16 | Disco livre | 33 GB (VOICEVOX 2,1 GB + modelos em sidecars/) |
| 2026-10-16 | Piso de ruído FLIP | A#138 0,015–0,017; C#131 0,0065–0,0083; D#218 0,0047 |
