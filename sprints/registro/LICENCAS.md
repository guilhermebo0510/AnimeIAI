# Revisão de licenças (S23)

Feita em 2026-10-26 pela retomada autônoma. Sem `cargo deny`/`cargo about` (não instalados; evitei `cargo install` para não gastar disco): o grafo saiu de `cargo metadata --locked` e foi classificado por `tools/licencas/listar_licencas.py`. Para refazer:

```
cd app && cargo metadata --format-version 1 --locked > %TEMP%\meta.json
python -I tools/licencas/listar_licencas.py %TEMP%\meta.json sprints/registro/LICENCAS-RUST.md
```

## Resumo

| Item | Licença | Como entra no app | Situação |
|---|---|---|---|
| 399 crates Rust de terceiros | todas com alternativa permissiva (MIT/Apache-2.0/Zlib/BSD/ISC/BSL/Unicode/OFL) | ligadas no binário | ok — lista em `LICENCAS-RUST.md` |
| `self_cell` 1.3.0 | Apache-2.0 OR GPL-2.0-only | ligada | ok: usamos a opção Apache-2.0 |
| `r-efi` 5.3 / 6.0 | MIT OR Apache-2.0 OR LGPL-2.1+ | só no alvo UEFI (não entra no build Windows) | ok |
| `epaint_default_fonts` 0.36.2 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | fontes padrão do egui embutidas | ok; ao distribuir, incluir os avisos OFL/Ubuntu Font |
| Noto Sans / Noto Sans JP | SIL OFL 1.1 | `aiai-studio/assets/fonts` (avisos `OFL-*.txt` ao lado) | ok; não vender a fonte sozinha, manter os avisos |
| Ícones e logomarca | originais do projeto (regra 9) | `aiai-studio/assets/icons`, `brand` | ok |
| Texturas de cenário/papel | geradas por código (`aiai-paint::texture`, procedurais com semente) | — | ok, sem arquivo de terceiros |
| FFmpeg 9.0 (gyan.dev full build) | GPL-3 (configurado com `--enable-gpl --enable-version3`) | **processo externo** (`AIAI_FFMPEG` ou `ffmpeg` no PATH, instalado pelo winget); nada é ligado ao app | ok para uso local. **Ao distribuir o app: não empacotar este build**; pedir que o usuário instale o FFmpeg ou empacotar um build LGPL (sem x264/x265) com o aviso e o link do código-fonte |
| VOICEVOX Engine 0.25.2 | LGPL-3 + termos próprios | processo separado por HTTP local, sem modificação; fora do git | ok; **proibido redistribuir o engine sem autorização**; crédito `VOICEVOX:<personagem>` obrigatório nos vídeos (detalhes em `licencas-voz.md`) |
| Kokoro-82M v1.0 | Apache-2.0 | modelo ONNX em `sidecars/kokoro`, executado por script Python separado | ok |
| espeak-ng (fonemização do Kokoro) | GPL-3 | só dentro do processo Python separado | ok (o app lê só o WAV) |
| FluidR3_GM.sf2 | MIT (Frank Wen) | `sidecars/soundfonts`, lido por `rustysynth` (MIT) | ok; manter `FluidR3_GM.COPYING.txt` se o arquivo for redistribuído |
| Crates do workspace (19) | `LicenseRef-Proprietary` | — | código do projeto |

Nenhuma dependência com copyleft obrigatório entra ligada ao binário. Os três pontos de atenção são de distribuição, não de uso local: FFmpeg GPL (não empacotar o build atual), VOICEVOX (não redistribuir o engine; crédito nos vídeos) e os avisos OFL/Ubuntu das fontes (incluir numa pasta `licenses/` do instalador).

## Pendências para um instalador

- Gerar `licenses/THIRD-PARTY.txt` com o texto de cada licença distinta da tabela de expressões em `LICENCAS-RUST.md` (MIT, Apache-2.0, Zlib, BSD-2/3, ISC, BSL-1.0, Unicode-3.0, OFL-1.1, Ubuntu-font-1.0, Unlicense, 0BSD, CC0, MIT-0) e os copyrights. `cargo about` faz isso quando houver disco para instalá-lo.
- Tela "Sobre" com os créditos de terceiros (fontes, FFmpeg como programa externo, VOICEVOX, Kokoro, FluidR3).
