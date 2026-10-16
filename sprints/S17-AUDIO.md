# S17 — Áudio: vozes TTS locais, SFX, BGM, mixagem, loudness, lip-sync

## Objetivo
Som suficiente para um anime curto, todo local e gratuito: vozes originais em japonês (VOICEVOX) e português/inglês (Kokoro), edição de entonação, SFX sintetizados e CC0, BGM composta num sequenciador com SoundFont livre, mixer com efeitos, loudness BS.1770 e lip-sync automático para o timesheet. Qualidade "mediana para cima": o foco do produto é a imagem.

## Pesquisa obrigatória
VOICEVOX Engine (API HTTP: `audio_query` com moras e tempos, `synthesis`, parâmetros de velocidade/tom/entonação/volume; termos de uso por personagem e crédito obrigatório), Kokoro-82M ONNX (vozes PT/EN, licença Apache-2.0), ITU-R BS.1770-4 e EBU R128 (gating, true peak), sfxr/jsfxr (síntese de SFX), SoundFonts livres com licença permissiva (ex.: GeneralUser GS — conferir licença), `rustysynth`.

## Tarefas
- [ ] `sidecars/voicevox/`: baixar a engine CPU oficial (conferir hash/assinatura), script para subir/parar como processo local; registrar termos e créditos das vozes usadas em `registro/licencas-voz.md`.
- [ ] Kokoro ONNX (via `ort` em Rust ou script Python em `tools/`), vozes PT-BR/EN.
- [ ] Perfil de voz por personagem: motor, voz/estilo, velocidade, tom, entonação, volume, pausas; prévia.
- [ ] Geração de fala: texto (+ leitura em kana quando necessário) → `audio_query` → edição de moras (tom e duração por mora, pausas) → síntese → clipe de áudio no projeto com texto e parâmetros de origem (regerável).
- [ ] Lip-sync: tempos das moras → vogal (a/i/u/e/o/n/fechada) → coluna de boca do timesheet nos quadros certos (regras de suavização: boca mínima de 2 quadros, fechar em pausas); para Kokoro, alinhar por fonemas (se a biblioteca expuser) ou por energia.
- [ ] Trilhas: diálogo, SFX, BGM, ambiente; clipes com ganho, fade, pan, corte; waveform; sincronizado com a timeline do corte e da edição.
- [ ] SFX procedural (síntese: ruído filtrado, osciladores, envelopes): whoosh, impacto, brilho (kira), passo, porta, vento, pano, sino; biblioteca CC0 opcional (baixar só arquivos CC0, registrar origem).
- [ ] BGM: sequenciador em grade (piano roll simples) com `rustysynth` + SoundFont livre; algumas trilhas/instrumentos; tempo, compasso, loop; render para WAV.
- [ ] Mixer: volume, pan, EQ de 3–5 bandas, compressor, limitador, reverb simples; ducking da BGM pela fala (P1).
- [ ] Loudness: medidor integrado/curto/momentâneo, true peak (oversampling 4×); normalização para -24 LKFS / -1 dBTP (preset ARIB) e outros presets (EBU -23, streaming -14 como referência).
- [ ] Saída de áudio ao vivo (cpal) e playback sincronizado com a imagem (relógio de áudio mestre — compartilhado com a S12).
- [ ] MCP: `aiai_voice_profile`, `aiai_tts_line` (texto, personagem, emoção, ajustes; devolve clipe e moras), `aiai_lipsync` (fala → coluna de boca), `aiai_sfx` (gerar por parâmetros/preset), `aiai_bgm` (notas/padrões), `aiai_mix`, `aiai_loudness`.

## Testes
- Loudness: sinais de teste da EBU (tom 1 kHz etc.) dentro de ±0,1 LU.
- TTS: frase gera clipe, moras com tempos coerentes com a duração; regenerar com mesmos parâmetros = mesmo áudio.
- Lip-sync: frase de teste → boca nos quadros esperados (verificado por tabela).
- Mouse: tocar trilha, arrastar clipe na timeline, mover fader.

## Critério de pronto
- [ ] Prova de uso: o personagem de teste fala duas frases em japonês com voz própria (perfil ajustado), lip-sync aplicado no corte da S12, SFX de brilho e passo sintetizados, BGM de 15 s composta no app, mix normalizado a -24 LKFS; MP4 com áudio e relatório de loudness em evidências.
