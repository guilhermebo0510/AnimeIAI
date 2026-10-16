# Licenças e créditos de áudio (S17)

Regra 2: áudio só local e gratuito; vozes sintéticas originais; nunca clonar voz de pessoa real; créditos exigidos pela licença vão nos créditos do vídeo.
Tudo abaixo foi lido nas páginas oficiais em 2026-10-16 (a licença pode mudar: reler antes de publicar).

## Créditos obrigatórios no vídeo

Colocar na tela de créditos e na descrição do vídeo, uma linha por voz usada:

```
VOICEVOX:春日部つむぎ
VOICEVOX:雨晴はう
```

O crédito de cada fala já fica gravado no asset (`AssetData.source` = `VOICEVOX:<personagem>`, e `params.credit`); a lista de créditos do episódio sai desse campo. Fala Kokoro: sem crédito exigido (Apache-2.0), registrar `Kokoro-82M (Apache-2.0)` mesmo assim. Nenhum SFX/BGM do app exige crédito (síntese própria; SoundFont MIT abaixo exige só preservar o aviso quando o próprio arquivo for redistribuído).

## VOICEVOX Engine (japonês)

| Item | Valor |
|---|---|
| Programa | VOICEVOX Engine 0.25.2, build Windows CPU |
| Origem | https://github.com/VOICEVOX/voicevox_engine/releases/tag/0.25.2 — `voicevox_engine-windows-cpu-0.25.2.7z.001` |
| Tamanho | 1 802 304 190 bytes (extraído: ~2,0 GB em `sidecars/voicevox/windows-cpu/`, fora do git) |
| SHA-256 | `2ab86e4bf29448e3317ee97327efb3211c4ecc1063b03a62ab72b15a92ec531d` (igual ao `digest` publicado pelo GitHub no release) |
| Assinatura | Authenticode do `run.exe` válida, assinante `CN=Kazuyuki Hiroshiba` (criador do VOICEVOX) |
| Licença do engine | LGPL v3 (ou licença separada paga, para quem quiser código fechado). Usado sem modificação, como processo separado por HTTP em 127.0.0.1:50021 — sem ligação ao código do app (regra 4). Dependências: `sidecars/voicevox/windows-cpu/licenses.json` (51 entradas). |
| Termos do software | https://voicevox.hiroshiba.jp/term/ — uso comercial e não comercial permitido; seguir os termos de cada biblioteca de voz; **crédito de uso do VOICEVOX obrigatório**; proibido redistribuir o programa sem autorização, engenharia reversa, atos contra a ordem pública. Quem receber áudio gerado deve ser obrigado a cumprir os itens 2 e 3 da permissão. |
| Uso no app | `tools/voicevox/start.ps1` / `stop.ps1` (127.0.0.1:50021). Cliente em `aiai-audio::voicevox`: `/speakers`, `/speaker_info`, `/audio_query`, `/accent_phrases`, `/mora_data`, `/synthesis`. |

### Vozes usadas na prova de uso (S17)

Os termos abaixo foram lidos duas vezes: no texto `policy` que o próprio engine devolve em `/speaker_info` e na página oficial do personagem.

**春日部つむぎ (Kasukabe Tsumugi), estilo ノーマル, speaker id 8** — "Haru" na prova.
- Texto do engine: com o crédito `VOICEVOX:春日部つむぎ`, uso comercial e não comercial permitido.
- Termos completos: https://tsumugi-official.studio.site/rule (seção "VOICEVOX「春日部つむぎ」利用規約"): comercial e não comercial; crédito em local à escolha (vídeo, descrição), exemplo `VOICEVOX:春日部つむぎ`; seguir os termos do VOICEVOX; proibido: difamação, desconforto a terceiros, violação de direitos, ligação a ideologia/organização específica, redistribuição e dizer-se autor da voz, uso que a administração julgue impróprio. A administração pode mudar os termos sem aviso.
- Conclusão: permitido para o micro anime (conteúdo original, sem política/religião); crédito obrigatório.

**雨晴はう (Amehare Hau), estilo ノーマル, speaker id 10** — "Mio" na prova.
- Texto do engine: com o crédito `VOICEVOX:雨晴はう`, uso comercial e não comercial permitido.
- Termos completos: https://amehau.com/?page_id=225 (redireciona a `/rules/amehare-hau-rule`): seguir os termos do VOICEVOX; comercial e não comercial para pessoas e empresas; **proibido uso R18 da voz**; crédito do personagem opcional (exemplo `VOICEVOX:雨晴はう`), **crédito do VOICEVOX obrigatório**; proibido usar áudio não gerado diretamente pelo VOICEVOX e modelos de conversão de voz feitos com a saída do VOICEVOX; proibido: ilegalidade, prejuízo a terceiros, fraude/ameaça, proselitismo político/social, redistribuir ilustrações oficiais, autoria falsa, desinformação médica; pedem bom senso ético.
- Conclusão: permitido; crédito do VOICEVOX obrigatório (incluímos também o do personagem).

Outras vozes avaliadas, não usadas ainda:
- 四国めたん, ずんだもん, 九州そら, 東北ずん子/きりたん/イタコ, 中国うさぎ, 中部つるぎ, あんこもん: termos comuns em https://zunko.jp/con_ongen_kiyaku.html — comercial e não comercial; crédito obrigatório (`VOICEVOX:ずんだもん`, `VOICEVOX:四国めたん`...; sem crédito em uso comercial custa 400 mil ienes por personagem); proibidos: contrário à ordem pública, política/religião, ataque ou apoio a pessoa/organização, infoprodutos, desinformação, certos ramos do entretenimento adulto, organizações antissociais.
- Antes de usar qualquer outra voz, ler `policy` em `/speaker_info` (o cliente tem `VoicevoxClient::speaker_policy`) e a página indicada nele.

Voz do app: nenhuma é clonada de dublador; os estilos do VOICEVOX são vozes sintéticas de personagens de licença própria, ajustadas por perfil (velocidade, altura, entonação, edição de moras).

## Kokoro-82M (português do Brasil e inglês) — opcional, feito

| Item | Valor |
|---|---|
| Modelo | Kokoro-82M v1.0 (hexgrad), Apache-2.0 |
| Arquivos | `kokoro-v1.0.onnx` (325 505 369 B, SHA-256 `beb0d1848dee9a49da392cc3df26958d46cfa35d321edf434f52949153f0df3a`) e `voices-v1.0.bin` (28 214 398 B, SHA-256 `bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d`) |
| Origem | https://github.com/thewh1teagle/kokoro-onnx/releases/tag/model-files-v1.1 (hashes iguais aos `digest` do release) |
| Executor | `kokoro-onnx` 0.6.1 (MIT) via `uv run --script tools/kokoro/kokoro_tts.py`; fonemização por espeak-ng (GPL-3) **só dentro do processo Python separado** — o app lê o WAV, não liga a biblioteca (regra 4) |
| Vozes PT-BR | `pf_dora`, `pm_alex`, `pm_santa`; EN: `af_*`, `am_*`, `bf_*`, `bm_*` |
| Disco | ~350 MB em `sidecars/kokoro/` + ambiente do uv (onnxruntime ~14 MB, espeakng-loader) |
| Limite | Não é determinístico bit a bit (vocoder com ruído sem semente; duração varia ~10 ms entre execuções) e não expõe fonemas: lip-sync por energia. O WAV guardado no projeto é a fonte da verdade. |

## SoundFont da BGM

| Item | Valor |
|---|---|
| Arquivo | `FluidR3_GM.sf2` (General MIDI, Fluid R3 de Frank Wen), 148 398 306 B, SHA-256 `74594e8f4250680adf590507a306655a299935343583256f3b722c48a1bc1cb0` |
| Origem | Fonte original empacotada pelo Debian: https://deb.debian.org/debian/pool/main/f/fluid-soundfont/fluid-soundfont_3.1.orig.tar.gz (SHA-256 do tar.gz `2621acaa1c78e4abdb24bdd163230cc577e61276936d6aa6e3180582142f0343`); arquivos `COPYING` e `README` copiados para `sidecars/soundfonts/` |
| Licença | MIT ("Copyright (c) 2000-2002, 2008 Frank Wen <getfrank@gmail.com>"; o README diz: "I hereby release Fluid under the MIT license"). Confirmada também em Fedora/MuseScore/LibreScore. Obrigação: manter o aviso em cópias do próprio SoundFont; áudio renderizado não o redistribui. |
| Alternativa lida e descartada | GeneralUser GS 2.0.3 (S. Christian Collins): licença permissiva para música e software, mas o próprio autor diz não poder garantir a origem de todas as amostras; MIT do FluidR3 é mais limpa. |
| Onde | `sidecars/soundfonts/` (fora do git). Engine: crate `rustysynth` 1.3.7 (MIT). Sem o arquivo, a BGM cai para a síntese interna (osciladores + ADSR + filtro) e o asset registra `engine: builtin`. |

## Crates Rust novos (aiai-audio)

| Crate | Versão | Licença |
|---|---|---|
| hound | 3.5.1 | Apache-2.0 |
| rustysynth | 1.3.7 | MIT |

Demais dependências já eram do workspace (serde, serde_json, schemars, thiserror). Reamostragem, loudness BS.1770, mixer, SFX e lip-sync são código próprio. Cliente HTTP próprio (127.0.0.1 apenas, sem TLS).

## SFX e BGM do app

- SFX: síntese procedural (osciladores, ruído filtrado, parciais de sino) em `aiai-audio::sfx`; sem amostras de terceiros; sem crédito.
- BGM: composição original escrita para a prova (progressão C–G–Am–F–Dm–G, 96 bpm) renderizada com o SoundFont acima; sem crédito.
- Biblioteca CC0 opcional: não incluída (nenhum arquivo baixado).
