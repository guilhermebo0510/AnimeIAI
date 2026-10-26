# 01 — Primeiros passos

## Requisitos

- Windows 10/11 64 bits, GPU com DirectX 12 ou Vulkan.
- Rust estável (para compilar) e ~8 GB livres no disco durante o build.
- Opcional: **FFmpeg** no PATH (exportar MP4/ProRes; `winget install Gyan.FFmpeg`) ou o caminho do executável em `AIAI_FFMPEG`.
- Opcional: VOICEVOX Engine (vozes em japonês) e Kokoro (português/inglês) em `sidecars/` — ver `tools/voicevox/start.ps1` e `tools/kokoro/`.

## Compilar

```
tools\build.ps1 -p aiai-studio --release
tools\build.ps1 -p aiai-mcp --release
```

O script confere o espaço livre antes (recusa com menos de 8 GB) e não aceita `--workspace` (estoura a RAM deste PC). Os executáveis ficam em `app\target\release\`.

## Abrir

```
app\target\release\aiai-studio.exe
```

A tela de abertura mostra os projetos recentes e o botão de projeto novo. Opções úteis:

| Opção | Efeito |
|---|---|
| `--project <pasta.aiai>` | abre esse projeto |
| `--workspace <chave>` | começa num workspace (`roteiro`, `personagens`, `storyboard`, `layout`, `cenario`, `animacao`, `pintura`, `composicao`, `biblioteca`, `modelagem`, `audio`, `edicao`, `entrega`, `producao`) |
| `--lang pt` / `--lang en`, `--theme light` / `--theme dark` | idioma e tema (também em Preferências) |
| `--bridge` | liga a bridge local para a IA operar o app aberto (ver guia 04) |
| `--no-session` | não restaura a sessão anterior |

## Projeto

Um projeto é uma pasta `nome.aiai/` com um diário de comandos (`wal.ndjson`) e os arquivos binários endereçados por hash (`blobs/`). Toda ação — feita com o mouse ou pela IA — entra no mesmo histórico de desfazer/refazer (Ctrl+Z / Ctrl+Shift+Z) e o autosave grava continuamente; Ctrl+S salva na hora.

Saídas (vídeo, animatic, PSD, quadros) vão para a subpasta `saida/` do projeto, salvo quando você escolhe outro destino.

## Fluxo típico

Roteiro → Personagens (model sheet, paleta) → Storyboard (painéis, animatic, *Gerar cortes*) → Layout/Cenário → Animação (genga/douga, mesa de luz, timesheet) → Pintura (balde, paleta indexada, *Verificar*) → Composição (efeitos, câmera, grade) → Áudio (falas, lip-sync, SFX, BGM, mixagem) → Edição → Entrega. O workspace Produção acompanha as etapas de cada corte, correções e retakes.
