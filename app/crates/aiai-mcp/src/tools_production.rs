//! Ferramentas curadas de produção (S02 B3): cenário, efeitos 2D, guias, personagem, animação, áudio, medição da
//! referência e exportação de vídeo. Cada uma é açúcar sobre uma ação da bridge (`aiai_bridge::production_tools`), com o
//! JSON Schema de entrada gerado do tipo de requisição e uma descrição em português com um exemplo de uma linha.
//! Rodam no app e no headless.

use crate::context::ToolError;
use crate::tools::{Plan, schema_of};
use serde_json::{Map, Value, json};

const SKY_DESC: &str = r##"Pinta um céu de anime (degradê zênite→horizonte + nuvens cúmulo estilizadas) na camada RASTER, no cel exposto no quadro. Entra no undo. `sky`: {time_of_day: day|morning|sunset|dusk|night (ou zenith/horizon [r,g,b] 0..1), horizon_y (px), clouds 0..1 (cobertura), cloud_scale (px), seed}. Coordenadas em pixels do canvas (y para baixo). Confirme com aiai_render.
Exemplo: {"cut":"<corte>","layer":"<camada raster>","sky":{"time_of_day":"sunset","horizon_y":600,"clouds":0.4}}"##;

const PATTERN_FILL_DESC: &str = r##"Preenche uma região da camada RASTER com uma textura procedural (papel, tijolo, madeira, grama, pedra, tecido... veja o schema de `texture.spec`), plana ou em PERSPECTIVE por homografia (parede, chão). `texture`: {spec, dark, light, accent?, seed, contrast, opacity} (cores sRGB 0..1); `mapping`: {type:"flat", scale, rotation, offset} ou {type:"perspective", quad:[4 cantos: sup-esq, sup-dir, inf-dir, inf-esq], size:[largura, altura da textura no plano]}; `region` (opcional): rect/ellipse/lasso/wand/color_range; sem region usa o quadrilátero (perspectiva) ou o canvas todo. `blend`, `opacity`. Entra no undo. Confirme com aiai_render.
Exemplo: {"cut":"<corte>","layer":"<raster>","texture":{"spec":{"type":"paper","scale":6},"dark":[0.8,0.78,0.7],"light":[0.95,0.93,0.86]},"mapping":{"type":"flat"}}"##;

const TEXT_DESC: &str = r##"Letreiro/onomatopeia/título desenhado: o preenchimento das letras vai numa camada RASTER (`fill_layer`) e, se `outline_layer` (VETORIAL) e `outline_width`>0, o contorno vai como traços fechados — um passo de undo. `font`: "black" (Noto Sans JP Black, padrão: onomatopeias e títulos), "regular" ou caminho de um .otf/.ttf sob C:\AnimeIAI. Japonês ok (ドン, ゴゴゴ). `size` px, `center` [x,y], `vertical` (tategaki), `letter_spacing`, `rotation` (graus, horário), `skew`, `scale` [sx,sy], `color` (direta ou de paleta), `roughness` (px de borda irregular de letreiro à mão) + `roughness_wavelength`, `seed`. Cada camada pode receber efeitos/tremor depois. Confirme com aiai_render.
Exemplo: {"cut":"<corte>","fill_layer":"<raster>","outline_layer":"<vetorial>","text":"ドン","size":260,"center":[960,540],"color":{"type":"direct","rgba":[1,0.9,0,1]},"outline_width":10,"roughness":3}"##;

const GRADIENT_PAINT_DESC: &str = r##"DEGRADÊ COMO COR (gestão de cor): aplica um degradê onde o programa aceita cor, ao vivo no render (continua editável). `op`: `fill_regions` (regiões de balde do cel exposto de `layer` no `frame` do `cut`: as que contêm `points` [[x,y]...] ou todas com `all:true`; o degradê é mapeado na caixa de cada região; `gradient:null` volta à cor chapada), `text_fill` / `text_stroke` (preenchimento ou contorno legado de uma camada de texto `layer`; `width` cria/ajusta o contorno), `palette_add` (guarda com `name` na `palette`; vai para a Biblioteca junto com a paleta) / `palette_remove` (`name` ou id), `presets` (lista os prontos; não altera nada). `gradient`: {shape: linear|radial|angular|diamond, stops: [{pos 0..1, color (direta ou de paleta), opacity}], angle (graus; 0 = esquerda→direita, 90 = de cima para baixo), center [0..1,0..1] relativo à caixa, scale (1 = cobre a caixa), extend: pad|repeat|reflect, space: srgb|linear|oklab (srgb = mistura de sempre; linear/oklab misturam pré-multiplicado, sem faixa escura), absolute: [[x,y],[x,y]] opcional (dois pontos do canvas no lugar da caixa)}; ou `preset` (nome de um pronto). Para efeitos de texto com degradê (contornos múltiplos, brilho, sobreposição) use aiai_text_layer `effects`; degradê raster de dois pontos: aiai_gradient (aceita `gradient`); pincel com degradê ao longo do traço: aiai_paint `preset.color_ramp` {gradient, length px}. Um passo de undo. Confirme com aiai_render.
Exemplo: {"op":"fill_regions","cut":"<corte>","layer":"<camada fill>","points":[[420,300]],"gradient":{"angle":90,"space":"oklab","stops":[{"pos":0,"color":{"type":"direct","rgba":[1,0.85,0.3,1]}},{"pos":1,"color":{"type":"direct","rgba":[0.9,0.3,0.1,1]}}]}}"##;

const TEXT_LAYER_DESC: &str = r##"Camada de TEXTO EDITÁVEL (o texto e o estilo ficam na camada e o render refaz o layout a cada quadro: depois dá para trocar o texto, a fonte e o tamanho, no app ou aqui). SEM `layer` cria uma camada nova em `cut` (exige `text`); COM `layer` (camada de texto) muda só os campos informados (`null` apaga `box_size`, `stroke` e `shadow`). Um passo de undo. Campos: `text` (quebra de linha = newline), `font` (família: instalada no Windows, da pasta Documentos/AnimeIAI/Fontes, ou "Noto Sans JP", embutida e padrão; `list_fonts:true` lista as famílias e não altera nada; fonte desconhecida dá erro com sugestões), `bold`/`italic` (usa a face da família se existir, senão sintetiza), `size` (px do canvas, altura do em), `color` (direta ou de paleta), `tracking` (milésimos de em), `leading` (entrelinha, múltiplo do tamanho, 1.25), `align` (left|center|right|justify), `vertical` (tategaki), `pos` [x,y] (texto de ponto: topo da primeira linha em y; x é a borda esquerda, o centro ou a direita conforme `align`; no vertical, a borda direita da primeira coluna), `box_size` [largura, altura] (texto de caixa: `pos` é o canto superior esquerdo e o texto quebra dentro da largura), `rotation` (graus, horário, em torno de `pos`), `stroke` {width px, color} (contorno por fora), `shadow` {offset [x,y], blur px, color com alfa}, `fill` (tinta no lugar de `color`: {"type":"gradient","gradient":{shape, stops [{pos, color, opacity}], angle, center, scale, extend, space}} mapeado na caixa das letras, {"type":"pattern","image":"<blob png>","scale"} ou {"type":"solid","color"}; `null` tira), `effects` (pilha de estilos como no Photoshop, substitui a lista; cada item plano com `type` + `enabled`/`blend`/`opacity`: stroke {width, position outside|inside|center, paint} (vários = contorno duplo; o primeiro fica por cima), drop_shadow / inner_shadow {offset [x,y], blur, spread, noise 0..1, color}, outer_glow / inner_glow {size, spread 0..1, paint (degradê = cor pela distância), noise, from_center}, bevel_emboss {style inner|outer|emboss|pillow, size, depth, soften, angle, altitude, highlight, highlight_opacity, shadow, shadow_opacity, down}, satin {color, angle, distance, size, invert}, gradient_overlay {gradient}, color_overlay {color}, pattern_overlay {paint}, long_shadow {angle, length, paint (degradê ao longo da profundidade), fade} = extrusão 3D de título), `warp` {style arc|arch|wave|flag|bulge|rise|path, bend -1..1, distort -1..1, path [[x,y]...], path_offset} (`null` tira) ou `warp_path_stroke` (id de um traço vetorial: texto no caminho dele), `style` (estilo pronto pelo nome, aplicado antes dos outros campos e escalado pelo `size`; `list_styles:true` lista) ou `style_file` (.aiaistyle), `opacity` da camada, `name`, `above` (a camada nova entra acima desta). Devolve o Outcome + `layer`, `created` e `text` {bounds [x0,y0,x1,y1] no canvas, lines, font_used, params}. Mova a camada com aiai_text_layer {layer, pos} (ou com a ferramenta Mover no app). Para letras assadas em pixels/traços (onomatopeia com efeitos por camada) use aiai_text. Confirme com aiai_render.
Exemplo: {"cut":"<corte>","text":"Olá, mundo","font":"Arial","size":72,"pos":[960,200],"color":{"type":"direct","rgba":[1,1,1,1]},"stroke":{"width":6,"color":{"type":"direct","rgba":[0,0,0,1]}}}"##;

const LAYER_MERGE_DESC: &str = r##"Mescla camadas de desenho como o Ctrl+E do Photoshop, num passo de undo: o resultado fica na camada de BAIXO (nome, posição, opacidade e modo dela) e as de cima saem. `op`: `down` (padrão com `layer`: a camada vai para a irmã logo abaixo), `selected` (as `layers`, 2 ou mais, no mesmo pai, viram uma só na mais baixa) ou `visible` (todas as camadas raster/vetor visíveis do `cut`, ou da pasta `parent`, na mais baixa). Tipos: vetor+vetor junta os traços (geometria intacta; a opacidade da de cima vira opacidade de traço e o modo de mistura tem de ser o mesmo), raster+raster compõe por tile com a opacidade e o modo da de cima (a opacidade da de baixo segue valendo), vetor+raster rasteriza os traços e a camada vira raster; pasta, preenchimento, texto, guia, referência, ajuste, efeito, partículas e 3D são recusados. Vale para o corte inteiro: cada trecho de quadros com a mesma combinação de cels expostos vira um cel da camada de baixo (o próprio cel dela se só aparece naquela combinação, senão um cel novo com a trilha ajustada). Recusa, sem alterar nada, camadas ocultas, travadas, de pastas diferentes, com máscara de recorte, com posição/escala/rotação/chaves diferentes, com cel transformado, ou com outra camada visível de tipo diferente entre elas. Máscaras e fronteiras de balde que apontavam para uma que saiu passam para a de baixo. Devolve o Outcome + `layer` (a que ficou), `merged` (as que saíram), `kind`, `cels_changed` e `cels_created`. Confirme com aiai_render.
Exemplo: {"layer":"<camada de cima>"}  ou  {"op":"selected","layers":["<a>","<b>","<c>"]}  ou  {"op":"visible","cut":"<corte>"}"##;

const LINES_DESC: &str = r##"Linhas de velocidade (paralelas) ou de foco/concentração (集中線, convergindo num ponto) como traços vetoriais afinados na camada VETORIAL (editáveis; troque `seed` por quadro para "ferver"). `kind`: {type:"speed", angle (graus), area:[x,y,w,h]} ou {type:"focus", center:[x,y], clear:[rx,ry] (elipse livre no meio)}; `count` (até 2000), `length` [mín,máx] px, `width` [mín,máx] px, `color`, `seed`. Entra no undo. Confirme com aiai_render.
Exemplo: {"cut":"<corte>","layer":"<vetorial>","kind":{"type":"focus","center":[960,540],"clear":[220,160]},"count":90,"length":[300,900],"width":[2,9],"color":{"type":"direct","rgba":[0,0,0,1]}}"##;

const ANIME_FX_DESC: &str = r##"Efeito de anime desenhado na camada RASTER (sobre o que já tem no cel do quadro), em formas chapadas de 2 a 3 tons: `kind` = {type:"fire", base:[x,y], height} (chama com línguas) | {type:"smoke", center:[x,y], radius, puffs:3, drift:[dx,dy]} (nuvens recortadas subindo) | {type:"lightning", from:[x,y], to:[x,y], width:10} (raio com galhos, brilho azul e miolo branco). `seed` muda o desenho; `phase` 0..1 é o momento do ciclo (chama tremula, fumaça sobe, raio muda): para animar, um cel por quadro com phase = quadro/ciclo. Entra no undo. Exemplo: {"cut":"C","layer":"FX","frame":0,"kind":{"type":"fire","base":[960,900],"height":400},"phase":0.25}. Confirme com aiai_render."##;

const GAG_BG_DESC: &str = r##"Fundo de gag/choque/alegria na camada RASTER (cobre o canvas): `pattern` = {type:"radial_stripes", center, count, colors:["#rrggbb","#rrggbb"]} (faixas radiais) | {type:"dots", spacing, radius, color, background, angle} (retícula de pontos/screentone) | {type:"stripes", spacing, width, angle, colors:[a,b]} (listras) | {type:"flat", color}. Entra no undo. Confirme com aiai_render.
Exemplo: {"cut":"<corte>","layer":"<raster>","pattern":{"type":"radial_stripes","center":[960,540],"count":32,"colors":["#fff3b0","#ffd23f"]}}"##;

const PARTICLES_DESC: &str = r##"Configura o sistema de partículas (pétalas, neve, chuva, brilhos, poeira, folhas) de uma camada do tipo `particles`: grava `params.particles` na camada (a simulação é determinística: mesma semente = mesmas partículas, e o render do quadro N simula até o tempo N). A camada precisa existir antes: crie com aiai_apply create_node {data:{type:"layer",kind:"particles"}} (dentro do corte). Use `preset` (petalas | brilhos | neve | chuva | poeira | folhas; o tamanho do canvas vem do corte) + `overrides` (mesclados em profundidade: ex. {"rate":40,"size":[14,26],"gravity":[0,60]}) OU um `system` completo (veja o schema). `seed` troca a distribuição. Substitui o sistema anterior da camada. Entra no undo. Confirme renderizando um quadro > 0 (ex.: 12) com aiai_render.
Exemplo: {"layer":"<camada particles>","preset":"petalas","overrides":{"rate":30},"seed":7}"##;

const GUIDE3D_DESC: &str = r##"Guia 3D: cena de blocos (caixa, cilindro, cone, esfera, plano, escada) + manequins proporcionais, vista por uma câmera, vira LINHAS de guia no cel da camada `layer` (guia ou vetorial; o conteúdo anterior do cel é substituído) e, opcional, sombreado de referência na camada raster `shaded_layer`. A cena e a câmera ficam gravadas em `params.guide3d` da camada para edição posterior. `scene`: {objects:[{shape:{type:"box",size:[x,y,z]},position,rotation,scale}], mannequins:[{proportions,position,facing,pose,bones}], floor_grid}; `camera`: {position, target, focal_mm (35 grande-angular, 50 normal), roll, ortho_height?}; unidades em metros, Y para cima. Desenhe o traço final POR CIMA com aiai_draw_curves. Entra no undo.
Exemplo: {"cut":"<corte>","layer":"<camada de guia>","scene":{"objects":[{"shape":{"type":"box","size":[4,3,0.2]},"position":[0,1.5,-2]}],"mannequins":[{"position":[0,0,0],"pose":"standing"}]},"camera":{"position":[2.5,1.4,4],"target":[0,1,0],"focal_mm":35}}"##;

const PERSPECTIVE_DESC: &str = r##"Régua de perspectiva: desenha raios a partir de cada ponto de fuga (+ linha do horizonte) como linhas de guia no cel da camada. Os pontos vêm de uma `camera` (a mesma de aiai_guide3d) ou são dados em `points` [[x,y]...] (+ `horizon_y`). `lines_per_point` (padrão 24), `color` ("#rrggbbaa"). Devolve o Outcome + `perspective` {points, horizon_y} (pontos de fuga em pixels, para você ancorar traços). Entra no undo.
Exemplo: {"cut":"<corte>","layer":"<camada de guia>","points":[[-900,540],[2800,540]],"horizon_y":540}"##;

const MODEL_SHEET_DESC: &str = r##"Cria o model sheet de um personagem (nó `character`): um corte-canvas com camadas de grade de proporção, manequim de guia (várias vistas), linha e cor, e grava as medidas (`heads_tall` etc.) no personagem — um passo de undo. `heads_tall`: altura em cabeças (1,5..10; chibi < 4), `figure_px` (padrão 800), `views`: graus de rotação por vista (padrão frente, 3/4, perfil, 3/4 costas, costas), `pose`. Devolve o Outcome + `cut`, `line_layer`, `fill_layer` (desenhe e pinte o personagem nelas). Depois use aiai_consistency para conferir as proporções dos desenhos.
Exemplo: {"character":"<id do personagem>","heads_tall":6.5,"name":"Hana"}"##;

const CHARACTER_MATERIAL_DESC: &str = r##"Pasta do personagem: lista e cria os MATERIAIS de um personagem (folha de modelo, expressões, tabela de boca, olhos, figurinos e folhas extras). Cada material é um corte-canvas filho do personagem com camadas para desenhar (`linha` vetorial, `cor`, grade de guia e, nos modelos, rótulos de texto por célula); a Animação (Referências do personagem) e o workspace Personagens abrem todos. `action`: list (padrão; não altera nada), create (`kind`: model_sheet | expressions (grade 4×3: neutro, alegre, triste, bravo, surpreso, vergonha, medo, cansado, maroto, chorando, sono, nojo) | mouth_chart (uma célula por forma do lip-sync: repouso, A, I, U, E, O, N, meia, aberta) | eyes (3×3: aberto, meio, fechado, piscada, esquerda, direita, cima, baixo, brilho) | outfit (folha + variante de paleta copiada da do personagem; `new_palette` false não copia) | other (canvas livre, `size` [w,h])), import (`path` de uma imagem; vira um corte com a imagem numa camada de referência travada e uma camada `linha` por cima), rename (`name`), duplicate (copia folha e, no figurino, a paleta), delete (apaga o material e o corte), move (`index` 0-based), update (`notes`, `name`, `palette`, `kind`). `labels` troca os rótulos das células (da esquerda para a direita, de cima para baixo). create devolve o Outcome + `material`, `cut`, `line_layer`, `fill_layer` (desenhe nelas com aiai_draw_curves/aiai_fill), `cells` ([{label, rect:[x,y,w,h]}] para você desenhar cada célula no lugar certo), `canvas` e `materials`. Um passo de undo por chamada. Para a folha de modelo com manequim e medidas use `kind: model_sheet` (igual a aiai_model_sheet) e confira com aiai_consistency; confirme o desenho com aiai_render do `cut`.
Exemplo: {"character":"<id>","action":"create","kind":"expressions"}"##;

const CONSISTENCY_DESC: &str = r##"Mede a consistência de um desenho contra o model sheet do personagem (não altera nada): você informa as medidas que desenhou (y em px: `head_top`, `chin`, opcionais `eyes`, `shoulders`, `feet`) e recebe o erro relativo (%) de cada proporção em unidades de cabeça. Devolve {errors_pct, max_error_pct, ok} (ok = todos ≤ 5%). O personagem precisa ter model sheet (aiai_model_sheet). As linhas esperadas são as genéricas do model sheet (olhos 0,55, ombros 1,35 cabeça...) ou as do settei do personagem, se gravadas em `measurements.landmarks` (ex.: aiai_apply set_props {measurements:{landmarks:{olhos:0.6}}}).
Exemplo: {"character":"<id>","head_top":120,"chin":260,"eyes":200,"feet":1010}"##;

const INBETWEEN_DESC: &str = r##"Intercala vetorialmente (estilo CACANi) entre dois desenhos-chave de uma camada VETORIAL: cria `count` cels novos entre os cels de número `key_a` e `key_b` (traços casados por ordem), com tempos por `times` (0..1), `ease` ou a tabela de "metades" (`halves_toward_b`). `first_number` define a numeração; com `start_frame` escreve no timesheet (A, intercalações e B, cada um por `hold` quadros, padrão 2); `fill_layer` (camada de cor ligada) ganha cels de mesmos números. `options` ajusta o casamento. Um passo de undo. Depois pinte com aiai_propagate_fills.
Exemplo: {"cut":"<corte>","layer":"<linha>","key_a":1,"key_b":2,"count":3,"start_frame":0,"hold":2}"##;

const PROPAGATE_FILLS_DESC: &str = r##"Propaga o balde de tinta das chaves para as intercalações: lê as regiões pintadas da camada de cor nos quadros `key_frame_a` e `key_frame_b`, casa por cor e proximidade, e pinta cada quadro de `frames` com sementes interpoladas (um único passo de undo). A chave A precisa estar pintada. `gap` (padrão 3) fecha buracos de linha. Confirme com aiai_lint (vazio = tudo pintado) e aiai_render.
Exemplo: {"cut":"<corte>","fill_layer":"<camada fill>","key_frame_a":0,"key_frame_b":8,"frames":[2,4,6]}"##;

const TTS_LINE_DESC: &str = r##"Fala em JAPONÊS por TTS local (VOICEVOX Engine em http://127.0.0.1:50021; troque por env AIAI_VOICEVOX_URL ou `voicevox_url` — só loopback). Cria um asset de áudio (com texto, consulta de áudio e tempos de moras em params, para lip-sync e para regenerar) e, com `cut`, posiciona o clipe na trilha no `start_frame`. `character` usa o perfil de voz dele (aiai_voice_profile); senão informe `speaker` (id do estilo). `kana` corrige leitura (`オハヨオ'`), `params` ajusta velocidade/tom/entonação. Devolve o Outcome + asset, placement, duration_s/duration_frames, credit (crédito obrigatório "VOICEVOX:<nome>" nos créditos do vídeo) e moras. Se o motor estiver desligado dá erro com o comando para subi-lo (C:\AnimeIAI\tools\voicevox\start.ps1). Entra no undo.
Exemplo: {"text":"おはよう!","speaker":8,"cut":"<corte>","start_frame":6}"##;

const TTS_KOKORO_DESC: &str = r##"Fala em PT-BR/EN pelo Kokoro-82M (ONNX local, sem GPU; processo `uv run` do tools/kokoro). Mesmo contrato de aiai_tts_line (asset + posicionamento opcional em `cut`/`start_frame`), mas sem moras (o lip-sync usa energia) e não determinístico. `kokoro`: parâmetros do modelo (voz, velocidade, idioma; veja o schema). Se o modelo não estiver instalado o erro diz o que falta. Entra no undo.
Exemplo: {"text":"Bom dia!","kokoro":{"voice":"pf_dora"},"cut":"<corte>","start_frame":0}"##;

const VOICE_PROFILE_DESC: &str = r##"Grava o perfil de voz de um personagem (motor voicevox, `speaker` = id do estilo, `speaker_name` para o crédito, `params` = velocidade/tom/entonação/volume padrão). aiai_tts_line com `character` usa este perfil sem repetir o speaker. Entra no undo.
Exemplo: {"character":"<id>","speaker":8,"speaker_name":"春日部つむぎ","params":{"speed_scale":1.05}}"##;

const LIPSYNC_DESC: &str = r##"Lip-sync: transforma um clipe de fala do corte em exposições de boca no timesheet (cria a coluna de boca se faltar; um passo de undo). `placement` (id do clipe em cut.audio) ou `asset`+`start_frame`; `mouth_layer`: camada cujos cels são as bocas; `mapping`: forma→número do cel (a, i, u, e, o, n, closed/rest, half, open). `mode`: auto (moras do VOICEVOX se houver, senão energia), moras, energy; `shapes`: three = fechada/meia/aberta; `min_hold_frames`, `close_gap_frames`, `lead_frames` (adianta a boca). Devolve o Outcome + track, first_frame, frames, exposures (quadro absoluto → cel), used_moras.
Exemplo: {"cut":"<corte>","placement":"<id do clipe>","mouth_layer":"<camada boca>","mapping":{"a":1,"i":2,"u":3,"e":4,"o":5,"closed":6}}"##;

const IMPORT_AUDIO_DESC: &str = r##"Importa um .wav do disco (caminho absoluto sob C:\AnimeIAI) como asset de áudio e, com `cut`, coloca o clipe na trilha `track` (dialogue | sfx | bgm | ambient; padrão sfx) em `start_frame`, com `gain_db`, `pan`, `fade_in`, `fade_out`. Devolve o Outcome + asset, placement, duration_s/duration_frames. Entra no undo."##;

const IMPORT_IMAGE_DESC: &str = r##"Importa uma imagem do disco (PNG, JPEG, WebP, BMP, TIFF, GIF; caminho absoluto sob C:\AnimeIAI) para o cel exposto de `layer` no `frame` (padrão 0): camada de REFERÊNCIA guarda a imagem no cel (rascunho, layout, foto de referência; não sai no render final a menos de include_reference); camada RASTER recebe a imagem como tiles editáveis por cima do que já está pintado. `fit`: contain (padrão, cabe inteira e centralizada) | cover | actual. Entra no undo."##;

const SFX_DESC: &str = r##"Efeito sonoro procedural (determinístico: mesma semente = mesmo áudio) como asset de áudio e, com `cut`, clipe posicionado na trilha SFX. `preset`: whoosh | impact | kira | footstep | door | wind | cloth | chime | pop | magic_swell | sfxr (usa `sfxr`); `seed`, `duration` (s), `pitch` (1 = padrão), `brightness` 0..1, `intensity`; posicionamento: `cut`, `start_frame`, `gain_db`, `pan`, `fade_in`, `fade_out`. Devolve o Outcome + asset, placement, duration_s/duration_frames. Entra no undo.
Exemplo: {"preset":"impact","seed":3,"cut":"<corte>","start_frame":12,"gain_db":-3}"##;

const BGM_DESC: &str = r##"Trilha musical a partir de uma partitura (`song`: tempo_bpm, beats_per_bar, bars, looped, tracks:[{name, program GM 0..127, drums, volume, pan, reverb, notes:[{pitch MIDI (C4=60), start_beat, length_beats, velocity}]}]). Renderiza com SoundFont (`soundfont` ou o padrão de sidecars/soundfonts) ou, sem ele/com `builtin_synth`, com o sintetizador interno. Cria o asset e, com `cut`, o clipe na trilha BGM. Devolve o Outcome + asset, placement, duration_s, engine (soundfont|builtin). Entra no undo.
Exemplo: {"song":{"tempo_bpm":110,"bars":2,"tracks":[{"name":"lead","program":73,"notes":[{"pitch":72,"start_beat":0,"length_beats":2}]}]},"builtin_synth":true,"cut":"<corte>"}"##;

const MIX_DESC: &str = r##"Mixa o áudio do corte (diálogo, SFX, BGM, ambiente, com ducking), mede o loudness (LUFS integrado, LRA, true peak) e, com `out` (.wav, pasta permitida), grava o WAV. `preset` normaliza ao alvo: arib (-24 LKFS, TV japonesa), ebu (-23 LUFS) ou streaming (-14 LUFS), com teto de true peak -1 dBTP. `frames` limita a duração, `bits` (pcm16|pcm24|float32), `settings` ajusta o mixer, `series` inclui a curva de loudness. Não altera o documento. Devolve {duration_s, loudness{integrated_lufs,...}, normalize?, path?}.
Exemplo: {"cut":"<corte>","preset":"arib","out":"C:\\AnimeIAI\\producoes\\mix.wav"}"##;

const LOUDNESS_DESC: &str = r##"Mede o loudness (ITU-R BS.1770: LUFS integrado, momentâneo/curto prazo máximo, LRA, true peak, pico) de UM entre: `asset` (clipe de áudio do projeto), `cut` (mixagem do corte) ou `path` (WAV sob C:\AnimeIAI). `series:true` inclui a curva. Não altera nada.
Exemplo: {"cut":"<corte>"}"##;

const COMPARE_DESC: &str = r##"Compara o quadro RENDERIZADO do corte com um quadro de referência (PNG) pela régua perceptual do protocolo de validação (decisão 25): FLIP limitado pelo FLIP da própria referência sob uma perturbação imperceptível (brilho/saturação ±5%, 1 px de deslocamento e deformação suave; calculado e guardado em <ref>.p5t.json), SSIM ≥ min(limite da perturbação, 0,93), F1 de bordas ≥ 0,90 com tolerância de 4 px, ΔE em regiões chapadas, espessura de linha e paleta. Devolve {report, noise_floor, flip_limit, flip_hard_limit, ssim_limit, edge_f1_limit, line_width_ok, pass, files}. `noise_floor` = FLIP entre dois quadros idênticos da referência (padrão 0), só registrado. `out_dir` (pasta permitida) grava render, mapa FLIP, sobreposição de bordas e relatório JSON. O render e a referência precisam ter o mesmo tamanho. Não altera o documento. Só vale no N1 (com referência).
Exemplo: {"cut":"<corte>","frame":0,"reference":"C:\\AnimeIAI\\calibracao\\q0000.png","out_dir":"C:\\AnimeIAI\\calibracao\\cmp"}"##;

const PROBE_COLOR_DESC: &str = r##"Instrumento de medição N1: cor mediana (por canal, quadrado de lado 2·radius+1) da referência em cada ponto [x,y] — devolve "#rrggbb" e rgb 0..1 por ponto. Serve para você ESCOLHER as cores da paleta que vai pintar; nada é criado nem copiado. `reference`: PNG sob C:\AnimeIAI. Não precisa de projeto aberto.
Exemplo: {"reference":"C:\\AnimeIAI\\calibracao\\q0000.png","points":[[400,300],[900,520]],"radius":3}"##;

const MEASURE_LINE_DESC: &str = r##"Instrumento de medição N1 (decisão 22): dado um traço que VOCÊ estimou (pontos [x,y] ao longo dele), mede na referência, perto de cada ponto e ao longo da normal (±`search` px), o CENTRO e a LARGURA da linha real (e o contraste). Ela só MEDE: não cria traços, não extrai contornos, não vetoriza e não copia pixels. Você continua decidindo existência, topologia, ordem, ponta, largura e cor de cada traço e DEVE emitir os próprios traços (aiai_draw_curves) com os centros/larguras medidos. `points` (um traço) ou `strokes` (vários); `min_contrast` 0..1. Devolve measures[{given, center, moved, width, contrast}] + resumo. Não precisa de projeto aberto.
Exemplo: {"reference":"C:\\AnimeIAI\\calibracao\\q0000.png","points":[[300,200],[340,260],[390,310]],"search":6}"##;

const FILL_SHAPE_DESC: &str = r##"Massa de cor / forma preenchida: preenche contornos fechados (`shapes`: [{points:[[x,y]...] (3+), geometry: "spline" (padrão, suave) | "polyline"}]) numa camada RASTER, com `color` (direta ou de paleta), `opacity`, `blend`, `aliased` (true = sem antisserrilhado, cor chapada de cel) e `roughness`/`roughness_wavelength`/`seed` (borda irregular de pincel/letreiro). Regra não-zero: um contorno em sentido oposto dentro de outro abre um buraco. Serve para manchas de cor, sombras, cabelo, formas de cenário. Entra no undo. Confirme com aiai_render; ajuste os contornos com aiai_measure_edge (N1).
Exemplo: {"cut":"<corte>","layer":"<raster>","shapes":[{"points":[[100,80],[220,70],[240,150],[120,170]]}],"color":{"type":"direct","rgba":[0.9,0.5,0.4,1]},"aliased":true}"##;

const MEASURE_EDGE_DESC: &str = r##"Instrumento de medição N1 (decisão 22) para MASSAS DE COR sem linha: dado o contorno que VOCÊ estimou (pontos [x,y] em ordem, tratado como fechado), procura na referência, ao longo da normal de cada ponto (±`search` px), a maior variação de cor e devolve a posição subpixel da borda, quanto andou (`moved`) e a intensidade do degrau (`strength`; 0 = não achou, `min_strength`). Ela só MEDE: não cria formas, não extrai contornos e não copia pixels; você continua decidindo quais massas existem e emite as suas formas (aiai_fill_shape) com os pontos medidos. `reference`: PNG sob C:\AnimeIAI. Não precisa de projeto aberto.
Exemplo: {"reference":"C:\\AnimeIAI\\calibracao\\q0000.png","points":[[100,80],[220,70],[240,150],[120,170]],"search":8}"##;

const EDIT_DESC: &str = r##"Cria (ou, com `id`, substitui) o documento de EDIÇÃO v2, estilo Premiere: `tracks` [{id, kind: video|audio|subtitle, name?, locked?, hidden? (vídeo/legenda), muted?/solo?/volume? (áudio), lang?/burn?/style? (legenda)}] (V1 embaixo; padrão V1 V2 A1 A2 A3 S1) e `items` [{id, track, start, dur (quadros da edição), src_in (entrada na mídia), source: {type: cut, cut} | {type: cut_audio, cut, track?: dialogue|sfx|bgm|ambience} | {type: image, asset} | {type: color, rgba} | {type: audio, asset} | {type: caption, text, speaker?, style?}, speed (retime), opacity, volume, pan, filters [efeitos do aiai_effects: grade, gaussian_blur, vignette, grain, lut... com keys], audio_fx {slots:[{fx:{type:eq|compressor|...}}]}, keys {opacity|volume|pan: [{frame,value}]}, link (grupo imagem+som), anchor (legenda presa a um item), transition_in/transition_out: {kind: dissolve|dip_black|dip_white|fade_black|fade_white|flash|wipe_left|wipe_right|wipe_up|wipe_down|slide_left|slide_right|push_left|push_right|iris|crossfade, frames, align: center|start|end}, enabled}]; itens da mesma trilha não se sobrepõem; transição entre vizinhos colados usa as alças da mídia. Também `credits` (linhas, `credits_seconds`) e `fade_out_end`. O formato v1 (`clips`: [{cut, in_frame, out_frame?, transition_in?}], `subtitles`: [{start_frame, end_frame, text}]) continua aceito e é convertido (mesma imagem e som). Para mexer numa edição existente prefira aiai_edit_ops (navalha, ripple, inserir...) e aiai_edit_captions (legendas). Devolve o Outcome + `edit` e `timeline` (total_frames, trilhas, itens). Entra no undo. Vídeo: aiai_export_video {edit, out}.
Exemplo: {"name":"Episódio 1","clips":[{"cut":"<corte1>"},{"cut":"<corte2>","transition_in":{"kind":"dissolve","frames":6}}],"subtitles":[{"start_frame":10,"end_frame":60,"text":"Olá!"}],"credits":["Direção: Claude"],"fade_out_end":12}"##;

const EXPORT_VIDEO_DESC: &str = r##"Exporta vídeo: renderiza os quadros [start, end) de um `cut`, todos os cortes de um `episode` (em ordem) ou um documento de `edit` (aiai_edit: trilhas de vídeo compostas, transições, filtros, legendas das trilhas S com `burn` e créditos já desenhados, áudio = mixagem das trilhas de áudio) com o motor real e codifica com ffmpeg (processo externo). `codec`: h264 (padrão, .mp4), h264_nvenc, prores (.mov) ou png_sequence (`out` com %05d); `crf` (padrão 18). Inclui a mixagem de áudio do corte (`audio` padrão true; `preset` arib|ebu|streaming normaliza) e, opcional, legendas queimadas (`srt` sob C:\AnimeIAI ou `subtitles`:[{start,end,text}] em segundos); `sidecar_srt: true` grava também o .srt de entrega ao lado do vídeo (edição: um por trilha de legenda, `.en.srt` etc.). `out` (absoluto) só em pasta permitida. Pode demorar (renderiza quadro a quadro; no app a janela fica ocupada até terminar). Devolve {path, frames, expected_frames, size, probe (ffprobe: frames, duração, codecs), audio (loudness), warnings}. Confira `probe.frames` = quadros pedidos.
Exemplo: {"cut":"<corte>","start":0,"end":48,"out":"C:\\AnimeIAI\\producoes\\gag.mp4"}"##;

/// Remove `frame` de `required` (a camada MCP o preenche com 0) e acrescenta propriedades extras.
fn schema_frame0<T: schemars::JsonSchema>() -> Value {
    let mut s = schema_of::<T>();
    if let Some(req) = s.get_mut("required").and_then(Value::as_array_mut) {
        req.retain(|v| v != "frame");
    }
    if let Some(f) = s.pointer_mut("/properties/frame").and_then(Value::as_object_mut) {
        f.insert("description".into(), json!("Quadro 0-based (padrão 0)."));
    }
    s
}

fn with_props(mut s: Value, extra: Value) -> Value {
    if let (Some(props), Some(e)) = (s.get_mut("properties").and_then(Value::as_object_mut), extra.as_object()) {
        for (k, v) in e {
            props.insert(k.clone(), v.clone());
        }
    }
    s
}

pub fn definitions() -> Vec<Value> {
    use aiai_bridge::production_tools as pt;
    use aiai_tools::{anim, audio, background, char_material, character, compare, fx2d, guide, text_layer as tl};
    let voicevox = json!({ "voicevox_url": { "type": "string", "description": "URL do VOICEVOX Engine (só loopback). Padrão: env AIAI_VOICEVOX_URL ou http://127.0.0.1:50021." } });
    let compare_schema = schema_frame0::<compare::CompareRequest>();
    vec![
        json!({ "name": "aiai_sky", "description": SKY_DESC, "inputSchema": schema_frame0::<background::SkyRequest>() }),
        json!({ "name": "aiai_pattern_fill", "description": PATTERN_FILL_DESC, "inputSchema": schema_frame0::<background::PatternFillRequest>() }),
        json!({ "name": "aiai_text", "description": TEXT_DESC, "inputSchema": schema_frame0::<fx2d::TextRequest>() }),
        json!({ "name": "aiai_text_layer", "description": TEXT_LAYER_DESC, "inputSchema": schema_of::<tl::TextLayerRequest>() }),
        json!({ "name": "aiai_gradient_paint", "description": GRADIENT_PAINT_DESC, "inputSchema": schema_of::<aiai_tools::gradient_paint::GradientPaintRequest>() }),
        json!({ "name": "aiai_layer_merge", "description": LAYER_MERGE_DESC, "inputSchema": schema_of::<aiai_tools::layer_merge::LayerMergeRequest>() }),
        json!({ "name": "aiai_lines", "description": LINES_DESC, "inputSchema": schema_frame0::<fx2d::LinesRequest>() }),
        json!({ "name": "aiai_gag_bg", "description": GAG_BG_DESC, "inputSchema": schema_frame0::<fx2d::GagBgRequest>() }),
        json!({ "name": "aiai_anime_fx", "description": ANIME_FX_DESC, "inputSchema": schema_frame0::<aiai_tools::anime_fx::AnimeFxRequest>() }),
        json!({ "name": "aiai_particles", "description": PARTICLES_DESC, "inputSchema": schema_of::<pt::ParticlesRequest>() }),
        json!({ "name": "aiai_guide3d", "description": GUIDE3D_DESC, "inputSchema": schema_frame0::<guide::Guide3dRequest>() }),
        json!({ "name": "aiai_perspective", "description": PERSPECTIVE_DESC, "inputSchema": schema_frame0::<guide::PerspectiveRequest>() }),
        json!({ "name": "aiai_model_sheet", "description": MODEL_SHEET_DESC, "inputSchema": schema_of::<character::ModelSheetRequest>() }),
        json!({ "name": "aiai_character_material", "description": CHARACTER_MATERIAL_DESC, "inputSchema": schema_of::<char_material::CharMaterialRequest>() }),
        json!({ "name": "aiai_consistency", "description": CONSISTENCY_DESC, "inputSchema": schema_of::<character::ConsistencyRequest>() }),
        json!({ "name": "aiai_inbetween", "description": INBETWEEN_DESC, "inputSchema": schema_of::<anim::InbetweenRequest>() }),
        json!({ "name": "aiai_propagate_fills", "description": PROPAGATE_FILLS_DESC, "inputSchema": schema_of::<anim::PropagateFillsRequest>() }),
        json!({ "name": "aiai_tts_line", "description": TTS_LINE_DESC, "inputSchema": with_props(schema_of::<audio::TtsLineRequest>(), voicevox) }),
        json!({ "name": "aiai_tts_kokoro", "description": TTS_KOKORO_DESC, "inputSchema": schema_of::<audio::KokoroLineRequest>() }),
        json!({ "name": "aiai_voice_profile", "description": VOICE_PROFILE_DESC, "inputSchema": schema_of::<pt::VoiceProfileRequest>() }),
        json!({ "name": "aiai_lipsync", "description": LIPSYNC_DESC, "inputSchema": schema_of::<audio::LipsyncRequest>() }),
        json!({ "name": "aiai_sfx", "description": SFX_DESC, "inputSchema": schema_of::<audio::SfxRequest>() }),
        json!({ "name": "aiai_bgm", "description": BGM_DESC, "inputSchema": schema_of::<audio::BgmRequest>() }),
        json!({ "name": "aiai_import_audio", "description": IMPORT_AUDIO_DESC, "inputSchema": schema_of::<audio::ImportAudioRequest>() }),
        json!({ "name": "aiai_import_image", "description": IMPORT_IMAGE_DESC, "inputSchema": schema_of::<aiai_tools::import::ImportImageRequest>() }),
        json!({ "name": "aiai_import_layers", "description": "Importa um .psd (RGB 8 bits) ou .ora com as camadas: cada camada de pixels vira uma camada raster no topo do corte, com nome, visibilidade, opacidade e modo de mistura. `path` absoluto. Entra no undo (um passo).
Exemplo: {\"cut\":\"<corte>\",\"path\":\"C:\\AnimeIAI\\producoes\\cenario.psd\"}", "inputSchema": schema_of::<aiai_tools::import_layers::ImportLayersRequest>() }),
        json!({ "name": "aiai_export_psd", "description": "PSD em camadas (RGB 8 bits) do quadro `frame` do corte: cada camada desenhável renderizada sozinha e recortada no conteúdo, com nome (Unicode), visibilidade e modo de mistura, mais a imagem composta. `camera` true = quadro da câmera; false (padrão) = canvas inteiro. `out` = caminho absoluto .psd; terminado em .ora grava OpenRaster (Krita, GIMP) com as mesmas camadas. Não altera o documento.", "inputSchema": schema_of::<aiai_tools::export::ExportPsdRequest>() }),
        json!({ "name": "aiai_export_xdts", "description": "Timesheet do corte em XDTS (Exchange Digital TimeSheet v5, formato público Celsys/Toei; abre no Clip Studio Paint EX, TVPaint, OpenToonz): campo de células por camada (de baixo para cima, SYMBOL_NULL_CELL no vazio) e campo de falas pelos clipes de diálogo. `out` = caminho absoluto .xdts. Não altera o documento.", "inputSchema": schema_of::<aiai_tools::export::ExportXdtsRequest>() }),
        json!({ "name": "aiai_export_svg", "description": "Quadro `frame` do corte em SVG: camadas vetoriais como caminhos (o contorno exato que o motor pinta, largura variável e pontas, cor da paleta resolvida, movimento da camada como transform); as outras camadas (raster, cor, partículas) como imagem PNG recortada no conteúdo. Uma <g> por camada (camada do Inkscape) com nome, visibilidade, opacidade e modo de mistura. Recorte/máscara de camada não vão (aviso em `warnings`). `out` = caminho absoluto .svg. Não altera o documento.", "inputSchema": schema_of::<aiai_tools::export::ExportSvgRequest>() }),
        json!({ "name": "aiai_export_otio", "description": "Edição em OpenTimelineIO (.otio, JSON) para editores de vídeo (DaVinci Resolve, Premiere via adaptador, Kdenlive, OpenRV): trilha de vídeo com um clipe por corte (trecho usado), dissolve como SMPTE_Dissolve com o ponto de corte no meio da sobreposição, outras transições como Custom_Transition (tipo nos metadados), créditos como GeneratorReference, legendas como marcadores, trilhas de áudio por tipo (falas, efeitos, música, ambiente). `media_dir` (absoluto, opcional): grava os WAV e liga os clipes de áudio; com `render_video` também renderiza cada clipe de vídeo como sequência PNG (com as sobras das transições) e liga ao clipe (demora). Sem mídia, os clipes ficam com MissingReference e o id do corte/asset nos metadados. Não altera o documento.", "inputSchema": schema_of::<aiai_tools::export::ExportOtioRequest>() }),
        json!({ "name": "aiai_mix", "description": MIX_DESC, "inputSchema": schema_of::<pt::MixRequest>() }),
        json!({
            "name": "aiai_loudness", "description": LOUDNESS_DESC,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "asset": { "type": "string", "description": "Id de um clipe de áudio do projeto." },
                    "cut": { "type": "string", "description": "Id do corte (mede a mixagem)." },
                    "path": { "type": "string", "description": "WAV sob C:\\AnimeIAI." },
                    "series": { "type": "boolean", "description": "Inclui as curvas de loudness (um ponto por 100 ms)." }
                },
                "additionalProperties": false
            }
        }),
        json!({ "name": "aiai_compare", "description": COMPARE_DESC, "inputSchema": compare_schema }),
        json!({ "name": "aiai_probe_color", "description": PROBE_COLOR_DESC, "inputSchema": schema_of::<pt::ProbeColorRequest>() }),
        json!({ "name": "aiai_measure_line", "description": MEASURE_LINE_DESC, "inputSchema": schema_of::<pt::MeasureLineRequest>() }),
        json!({ "name": "aiai_fill_shape", "description": FILL_SHAPE_DESC, "inputSchema": schema_frame0::<aiai_tools::paint::FillShapeRequest>() }),
        json!({ "name": "aiai_measure_edge", "description": MEASURE_EDGE_DESC, "inputSchema": schema_of::<pt::MeasureEdgeRequest>() }),
        json!({ "name": "aiai_edit", "description": EDIT_DESC, "inputSchema": schema_of::<pt::EditSetRequest>() }),
        json!({ "name": "aiai_export_video", "description": EXPORT_VIDEO_DESC, "inputSchema": schema_of::<pt::ExportVideoRequest>() }),
    ]
}

fn require(m: &Map<String, Value>, key: &str) -> Result<(), ToolError> {
    if m.get(key).is_none_or(Value::is_null) {
        return Err(ToolError::invalid(format!("Missing '{key}': veja a descrição da ferramenta")));
    }
    Ok(())
}

fn required(m: &Map<String, Value>, keys: &[&str]) -> Result<(), ToolError> {
    keys.iter().try_for_each(|k| require(m, k))
}

/// `frame` ausente = 0 (o primeiro quadro).
fn with_frame(mut m: Map<String, Value>) -> Value {
    m.entry("frame").or_insert(json!(0));
    Value::Object(m)
}

/// Plano de uma ferramenta de produção; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    let simple = |action: &'static str, params: Value| -> Result<Plan, ToolError> { Ok(Plan { action, params, save_png: None }) };
    let with_req = |keys: &[&str], action: &'static str, frame: bool| -> Result<Plan, ToolError> {
        required(m, keys)?;
        simple(action, if frame { with_frame(m.clone()) } else { Value::Object(m.clone()) })
    };
    Some(match name {
        "aiai_sky" => with_req(&["cut", "layer", "sky"], "SKY", true),
        "aiai_pattern_fill" => with_req(&["cut", "layer", "texture", "mapping"], "PATTERN_FILL", true),
        "aiai_text" => with_req(&["cut", "fill_layer", "text", "size", "center", "color"], "TEXT", true),
        "aiai_text_layer" => {
            let list = ["list_fonts", "list_styles"].iter().any(|k| m.get(*k).and_then(Value::as_bool).unwrap_or(false));
            if !list && m.get("layer").is_none_or(Value::is_null) {
                required(m, &["cut", "text"]).and_then(|_| simple("TEXT_LAYER", Value::Object(m.clone())))
            } else {
                simple("TEXT_LAYER", Value::Object(m.clone()))
            }
        }
        "aiai_layer_merge" => {
            let has = |k: &str| m.get(k).is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()));
            if !(has("layer") || has("layers") || has("cut") || has("parent")) {
                Err(ToolError::invalid("Missing 'layer': informe `layer` (mescla para baixo), `layers` (op selected) ou `cut` (op visible)"))
            } else {
                simple("LAYER_MERGE", Value::Object(m.clone()))
            }
        }
        "aiai_gradient_paint" => simple("GRADIENT_PAINT", Value::Object(m.clone())),
        "aiai_lines" => with_req(&["cut", "layer", "kind", "count", "length", "width", "color"], "LINES", true),
        "aiai_gag_bg" => with_req(&["cut", "layer", "pattern"], "GAG_BG", true),
        "aiai_anime_fx" => with_req(&["cut", "layer", "kind"], "ANIME_FX", true),
        "aiai_particles" => {
            required(m, &["layer"]).and_then(|_| {
                if m.get("preset").is_none_or(Value::is_null) && m.get("system").is_none_or(Value::is_null) {
                    return Err(ToolError::invalid("Missing 'preset': informe um preset (petalas, brilhos, neve, chuva, poeira, folhas) ou um 'system' completo"));
                }
                simple("PARTICLES", Value::Object(m.clone()))
            })
        }
        "aiai_guide3d" => with_req(&["cut", "layer", "scene", "camera"], "GUIDE3D", true),
        "aiai_perspective" => with_req(&["cut", "layer"], "PERSPECTIVE", true),
        "aiai_model_sheet" => with_req(&["character", "heads_tall"], "MODEL_SHEET", false),
        "aiai_character_material" => with_req(&["character"], "CHARACTER_MATERIAL", false),
        "aiai_consistency" => with_req(&["character", "head_top", "chin"], "CONSISTENCY", false),
        "aiai_inbetween" => with_req(&["cut", "layer", "key_a", "key_b", "count"], "INBETWEEN", false),
        "aiai_propagate_fills" => with_req(&["cut", "fill_layer", "key_frame_a", "key_frame_b", "frames"], "PROPAGATE_FILLS", false),
        "aiai_tts_line" => with_req(&["text"], "TTS_LINE", false),
        "aiai_tts_kokoro" => with_req(&["text"], "TTS_KOKORO", false),
        "aiai_voice_profile" => with_req(&["character", "speaker"], "VOICE_PROFILE", false),
        "aiai_lipsync" => with_req(&["cut", "mouth_layer", "mapping"], "LIPSYNC", false),
        "aiai_sfx" => with_req(&["preset"], "SFX", false),
        "aiai_bgm" => with_req(&["song"], "BGM", false),
        "aiai_import_audio" => with_req(&["path"], "IMPORT_AUDIO", false),
        "aiai_import_image" => with_req(&["cut", "layer", "path"], "IMPORT_IMAGE", false),
        "aiai_import_layers" => with_req(&["cut", "path"], "IMPORT_LAYERS", false),
        "aiai_export_psd" => with_req(&["cut", "out"], "EXPORT_PSD", false),
        "aiai_export_xdts" => with_req(&["cut", "out"], "EXPORT_XDTS", false),
        "aiai_export_svg" => with_req(&["cut", "out"], "EXPORT_SVG", false),
        "aiai_export_otio" => with_req(&["edit", "out"], "EXPORT_OTIO", false),
        "aiai_mix" => with_req(&["cut"], "MIX_CUT", false),
        "aiai_loudness" => simple("LOUDNESS", Value::Object(m.clone())),
        "aiai_compare" => with_req(&["cut", "reference"], "COMPARE", true),
        "aiai_probe_color" => with_req(&["reference", "points"], "PROBE_COLOR", false),
        "aiai_measure_line" => {
            required(m, &["reference"]).and_then(|_| {
                if m.get("points").is_none_or(Value::is_null) && m.get("strokes").is_none_or(Value::is_null) {
                    return Err(ToolError::invalid("Missing 'points': informe os pontos do traço que você estimou (ou 'strokes' com vários)"));
                }
                simple("MEASURE_LINE", Value::Object(m.clone()))
            })
        }
        "aiai_fill_shape" => with_req(&["cut", "layer", "shapes", "color"], "FILL_SHAPE", true),
        "aiai_edit" => with_req(&["clips"], "EDIT_SET", false),
        "aiai_measure_edge" => with_req(&["reference", "points"], "MEASURE_EDGE", false),
        "aiai_export_video" => {
            required(m, &["out"]).and_then(|_| {
                if ["cut", "episode", "edit"].iter().all(|k| m.get(*k).is_none_or(Value::is_null)) {
                    return Err(ToolError::invalid("Missing 'cut': informe 'cut' (um corte), 'episode' (episódio inteiro) ou 'edit' (documento de edição)"));
                }
                simple("EXPORT_VIDEO", Value::Object(m.clone()))
            })
        }
        _ => return None,
    })
}
