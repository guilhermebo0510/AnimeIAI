# Diário de produção — 「最後の一輪」 (A última rosa)

Curta original de 33,2 s (796 quadros a 23,976 fps; 26,5 s de cortes + 7 s de créditos), feito inteiro dentro do
AnimeIAI pelo Claude, por comandos do app (`aiai-mcp call`, o mesmo Command Bus das ferramentas MCP). Nenhuma imagem
gerada por IA, nenhum material do episódio de referência, nenhuma imagem externa (texturas procedurais do app).

## Entregáveis

| Arquivo | Conteúdo |
|---|---|
| `ultima-rosa.mp4` | H.264 1920×1080 23,976 fps + AAC estéreo 48 kHz, legendas PT queimadas, créditos |
| `ultima-rosa.srt` | legendas de entrega (4 falas) |
| `ultima-rosa_mix_arib.wav` | mixagem normalizada ARIB: −24,0 LKFS, true peak −5,0 dBTP, LRA 9,8 LU |
| `ultima-rosa.aiai/` | projeto (log de comandos = origem de todo desenho, pintura e efeito) |
| `design/` | model sheets, expressões e bocas, rostos 3/4 |
| `econte.png`, `animatic.mp4` | e-conte de 7 painéis e animatic com TTS |
| `revisao.json` | proporções × model sheet, ΔE × settei, lint de cada corte |
| `PLANO.md` | história, personagens, cortes |

## Etapas (scripts em `tools/curta/`)

| Etapa | Script | Observação |
|---|---|---|
| Bíblia, beats, roteiro JA+PT, fichas | `c00_preprod.py` | aiai_script/beats, personagens com paleta |
| Vozes | `c01_vozes.py` | VOICEVOX: 春日部つむぎ (Hana), 白上虎太郎 (Ren) |
| Model sheets, expressões, bocas | `c02`–`c04`, `rig.py` | rig de desenho (cabeça com giro, expressões, bocas, cabelo, sombras de cel) |
| Storyboard, animatic, cortes | `c05`, `c06` | 7 painéis → 7 cortes |
| Cenário 1 — jardim (C001) | `c07_bg_jardim.py` | casa em 2 pontos de fuga, textura de madeira e telha em perspectiva, céu, pan com multiplano em 5 planos |
| Cenário 2 — varanda (C006) | `c08_bg_varanda.py` | cena de blocos na guia 3D; massas pintadas com a mesma câmera (conferidas sobre a guia) |
| Cenário 3 — céu (C007) | `c09_bg_ceu.py` | céu, raios, jardim em contraluz, tilt com multiplano |
| C003, C004 | `c10`, `c11`, `corte.py` | 18 cels olho×boca por personagem; lip-sync por moras + piscadas no timesheet das 5 camadas |
| C005 (gag) | `c12_c005.py` | fundo radial, linhas de foco fervendo (3 desenhos em 2s), tremor |
| C002 (mãos) | `c13_c002.py` | rosa de perfil, braços entrando por deslize de camada e se tocando |
| C006 (animação) | `c14_c006.py` | virada de Hana: 2 chaves + 6 intercalações (aiai_inbetween, em 2s), sorriso, fala com 6 bocas felizes |
| C007 (efeito) | `c15_c007.py` | partículas de pétalas, glow, god rays, difusão, vinheta |
| Som | `c16_som.py` | 10 SFX procedurais; trilha original (flauta, piano, cordas, baixo; 11 compassos) contínua por `trim_start`, muda no gag |
| Edição | `c17_edicao.py` | fade do preto, flash no gag, dissolve, 4 legendas, créditos |
| Export | `c18_export.py` | 796/796 quadros conferidos pelo ffprobe |
| Cor | `c19_grade.py` | grade por corte (entardecer quente; gag com mais contraste) |
| Revisão | `c20_revisao.py` | proporções medidas no render; ΔE nas sementes; lint de cada corte (ver correção S23 abaixo) |
| Repintura honesta (S23) | `c22_repintura.py` | sem o remendo das sobras; sementes explícitas no rig; frestas só por `close_leftovers` |

## Dificuldades e o que foi corrigido no programa

- **Balde em camada animada** (C002): o balde lia as linhas na posição deslizada do quadro e pintava o canvas inteiro.
  Corrigido no motor (`RenderOptions.layer_motion`), com teste de regressão (LACUNAS 13).
- **Ease em texto** recusado por câmera/camada; **CLI** recusava `GUIDE3D` (dígito no nome) — corrigidos (LACUNAS 7 e 9).
- **Consistência** comparava com guias genéricas (olhos a 0,55 cabeça) e acusava 9% em todo desenho do rig (olhos a
  0,60); o personagem agora grava as próprias linhas-guia (`measurements.landmarks`).
- **Propagação do balde** casou o rosto com uma mecha em 2 de 6 intercalações da virada; contornado com as sementes do
  rig interpoladas (LACUNAS 14, aberta). Formas raster (olhos, sombras, alças) não intercalam — interpoladas à mão
  (LACUNAS 15, aberta).
- **Rig**: busto cortado pela borda deixava regiões abertas; gola e alças abriam vazamentos — corrigido (LACUNAS 12).
- **Multiplano** com zoom mostra a borda dos planos distantes; a escala de cada plano foi calculada à mão (LACUNAS 8).
- **Lip-sync** escreve uma camada só; com 5 camadas por personagem o timesheet foi montado a partir das exposições
  (LACUNAS 11).

## Limites honestos

- O desenho dos personagens é limpo mas simples (rig paramétrico desenhado pelo agente); a animação de corpo é só
  a virada de cabeça do C006 e o deslize dos braços do C002.
- O C001 não tem personagens (como no storyboard); as mãos do C002 são um desenho único que desliza.
- Não houve teste de escuta humano do áudio; os números de loudness vêm do medidor BS.1770 do app.

## Correção (S23, 2026-10-19, depois da auditoria externa)

A revisão anterior dizia "lint 0 nos 7 cortes" e "proporções ≤ 1,5%". Os dois números não valiam:

- **Lint 0** vinha de um remendo: `curta.paint()` pegava toda sobra < 4000 px e pintava de cabelo ou roupa conforme a
  altura na tela, e `c21_sobras.py` pintava cada fresta com a cor do pixel mais próximo. Isso zerava o lint e errava
  a cor (alça do avental creme no 3/4 da Hana, triângulos pretos acima das sobrancelhas do Ren, dedos azuis).
- **Proporções** eram comparadas com os próprios parâmetros do rig (0,60 contra 0,60), não com o desenho.

O que mudou: o balde do app foi corrigido (regiões estreitas fechadas tinham rótulo 0 e a semente pulava a linha);
o remendo saiu; cada região que sobrou ganhou semente explícita no rig com a cor certa (mechas e pele entre
mechas, alças como forma, aba da gola, avesso das mangas); frestas de até 300 px são fechadas pela ferramenta
`close_leftovers` do balde (cor que mais aparece em volta, opção visível no MCP e na interface); `c20_revisao.py`
mede topo, olhos e queixo no render. O C999 de ensaio saiu do projeto. Números novos em `repintura.json` e
`revisao.json`.
