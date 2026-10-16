# S18 — Edição e entrega (referência: Premiere, AE render queue)

## Objetivo
Montar o episódio/curta a partir dos cortes, com transições, legendas e créditos, e exportar com qualidade verificada (vídeo, áudio, loudness, formato).

## Tarefas
- [ ] Timeline de edição: trilhas de vídeo (cortes renderizados como clipes vinculados ao corte vivo — re-render quando o corte muda) e de áudio; trim, ripple, roll, slip; snapping; marcadores.
- [ ] Transições: corte seco, fade de/para preto e branco, dissolve, wipe (direção, suavidade), flash.
- [ ] Legendas: trilha de legenda (PT) com tempo, estilo (fonte, contorno, sombra, posição), importação das falas do roteiro (tradução) com tempos das falas TTS; exportar SRT/ASS.
- [ ] Títulos e créditos: camada de texto animável, rolagem de créditos (inclui créditos de voz VOICEVOX e lista de texturas CC0 automaticamente).
- [ ] Fila de render: corte, faixa, edição completa; cache por revisão; render em segundo plano com progresso; retomar após falha.
- [ ] Exportação via FFmpeg (processo externo): MP4 H.264 (CRF/2 passadas) + AAC 320k; H.264 NVENC se disponível (GTX 1650 tem NVENC Turing); ProRes 422 (prores_ks); sequência PNG 16 bits; WAV 48 kHz 24 bits; legenda embutida queimada ou como faixa.
- [ ] Verificação de entrega: resolução, fps exato (24000/1001), duração, loudness integrado e true peak, ausência de quadros pretos/duplicados não intencionais, teste simples de flashes (variação de luminância > limiar mais de 3 vezes por segundo).
- [ ] P1: export EDL/OTIO, XDTS do timesheet de um corte, PSD de um quadro (camadas raster).
- [ ] MCP: `aiai_edit` (inserir corte, trim, transição), `aiai_subtitle`, `aiai_credits`, `aiai_render_queue`, `aiai_export`, `aiai_delivery_check`.

## Testes
- Export de 10 s em cada formato; `ffprobe` confere parâmetros; loudness medido no arquivo final bate com o do mixer (±0,2 LU).
- Mouse: arrastar corte na timeline, aparar borda, aplicar transição por arrastar e soltar.

## Critério de pronto
- [ ] Prova de uso: montagem dos cortes de teste das S12–S17 (≈10 s) com transição, legenda PT e créditos, exportada em MP4 e verificada; relatório de entrega em evidências.
