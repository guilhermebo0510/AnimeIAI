# S22 — Micro anime original (30–60 s)

## Objetivo
Produzir dentro do app, só via MCP (e mouse quando for o caminho natural), um curta original de 30–60 s com história, personagens, cenários, animação, efeitos, cor, vozes em japonês com legenda em português, SFX, BGM, edição e export, seguindo o `08-FLUXO-DE-PRODUCAO-NO-APP.md` e o aceite do `07-PROTOCOLO-DE-VALIDACAO.md` §7.

## Diretrizes criativas
- História simples e completa, em estilo anime (pode se inspirar no tom do episódio de referência — comédia romântica de época, jardim de rosas, chá — sem copiar personagens, cenas ou falas).
- Escopo cabível: 6–10 cortes, 2–3 personagens, 3–4 cenários, um momento de animação de personagem mais rica, um momento de efeito (pétalas, brilho ou gag), diálogo curto.
- Exemplo de ponto de partida (o agente pode trocar): uma jovem jardineira e um aprendiz de confeiteiro disputam a última rosa do jardim para um concurso; descobrem que querem dá-la à mesma pessoa, a avó que cuidou dos dois. Final com pétalas ao vento.

## Tarefas (na ordem do fluxo)
- [x] Bíblia, beats, roteiro (JA + PT), fichas de personagem.
- [x] Model sheets, expressões, bocas, settei de cor de cada personagem.
- [x] Perfis de voz e teste de falas.
- [x] Cenários-base (blocos 3D → linha → pintura com pincéis e texturas).
- [x] Storyboard/e-conte, animatic com TTS, ajuste de tempo, geração dos cortes.
- [x] Para cada corte: layout, genga, douga (intercalação + correção), lip-sync e piscar, shiage com lint = 0, BG/BOOK do corte, satsuei (câmera, difusão, luz, partículas, FX), cor, render.
- [x] Som final: falas, SFX, BGM, mix, loudness.
- [x] Edição: montagem, transições, legendas PT, créditos (vozes VOICEVOX, texturas CC0, "produzido no AnimeIAI").
- [x] Export MP4 e verificação de entrega.
- [x] Revisão (S19): checar consistência de cada corte (proporção, cor, lint), retakes até passar.
- [x] Diário de produção: tempo por etapa, dificuldades, ferramentas que faltaram (→ LACUNAS e correção nas sprints de origem).

## Critério de pronto
- [x] Todos os itens do aceite §7 do protocolo com evidência.
- [x] `producoes/micro-anime/` com o projeto `.aiai`, o MP4 final, WAV, SRT, folhas de model sheet/settei/e-conte exportadas e o diário.

## Execução (2026-10-16/17)

Concluída. Entregáveis e etapas em `producoes/micro-anime/DIARIO.md`; evidências em `registro/evidencias/S22/`
(`mp4_final_folha.png`, tiras de cada corte, `c006_virada.png`, `grade_antes_depois.png`, `edicao_previa.png`,
`revisao.json`). Aceite §7: 33,2 s H.264/AAC 1920×1080 23,976 fps 48 kHz; 2 personagens com model sheet, expressões,
bocas e settei; 7 cortes do storyboard com animatic; 3 cenários (guia 3D na varanda, 2 pontos de fuga no jardim,
texturas e pincéis); C006 com 8 desenhos (chaves + intercalação), piscar e lip-sync nas 4 falas; pan e tilt com
multiplano, partículas, glow, difusão, god rays; grade por corte; VOICEVOX com legenda PT, SFX e BGM do app,
−24,0 LKFS / −5,0 dBTP; proporções ≤ 1,5%, ΔE 0, lint 0; créditos de voz e de texturas (procedurais).
Lacunas abertas que nasceram aqui: 8, 10, 11, 14, 15.
