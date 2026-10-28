//! Catálogo das ferramentas MCP genéricas e estáveis (a lista não muda entre sprints, então a sessão do agente não precisa
//! reiniciar) e o despacho delas para as ações da bridge. As descrições são em português: é o que o agente lê.
//!
//! Ferramentas curadas de alto nível (`aiai_draw_curves`, `aiai_fill`, `aiai_lint`, `aiai_paint`, `aiai_gradient`,
//! `aiai_transform`, `aiai_repeat`, `aiai_brush_presets`, `aiai_render`) e as de produção (`tools_production`): açúcar sobre comandos do Command Bus (nada de
//! caminho paralelo), com o JSON Schema de entrada gerado dos tipos de requisição de `aiai-tools`. Rodam no app e no headless.

use crate::context::{Ctx, ToolError, mcp_image, take_png};
use serde_json::{Map, Value, json};
use std::path::PathBuf;

pub struct ToolResult {
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl ToolResult {
    pub fn error(e: &ToolError) -> Self {
        ToolResult { content: vec![json!({"type": "text", "text": format!("Falha (código {}): {}", e.code, e.message)})], is_error: true }
    }
}

const APPLY_DESC: &str = "Aplica UM comando ao documento (entra no undo/redo e aparece ao vivo no app). Devolve o Outcome: revision, description, created (ids criados, na ordem), affected, can_undo, can_redo. \
Use `if_revision` (a revision que você viu por último) para não pisar em mudança feita por outra pessoa/agente: se não bater, falha com código -32010 e nada muda. \
Ids novos podem ser omitidos (a sessão gera) — leia-os em `created`. \
Hierarquia: episode > scene > cut > layer > cel (palette/character/asset/document são raízes). \
Cola dos comandos principais (campo `kind`):\n\
- create_node {name, parent?, index?, id?, data}: data é {\"type\":\"episode\",\"number\":1} | {\"type\":\"scene\"} | {\"type\":\"cut\",\"duration\":48} | {\"type\":\"layer\",\"kind\":\"vector\"} (kinds: group, vector, raster, fill, text, guide, reference, adjustment, effect, particles) | {\"type\":\"cel\",\"number\":1}. Sem parent = raiz (só episode/character/palette/asset/document).\n\
- set_props {id, name?, data?}: renomeia e/ou aplica um JSON Merge Patch em `data` do nó (ex.: {\"opacity\":0.5,\"blend\":\"multiply\"}).\n\
- delete_node {id}: apaga o nó e a subárvore. move_node {id, parent?, index?}.\n\
- insert_strokes {cel, items:[{index, stroke}]}: stroke = {\"id\":\"\",\"points\":[{\"x\":10,\"y\":20,\"pressure\":0.8}],\"geometry\":\"spline\",\"width\":4,\"tip\":\"g_pen\",\"color\":{\"type\":\"direct\",\"rgba\":[0,0,0,1]}}; coordenadas em pixels do quadro do corte.\n\
- set_tiles {cel, tiles:[{x,y,blob}]} (raster; blob = hash já gravado).\n\
- add_track {cut, track:{\"id\":\"\",\"name\":\"A\",\"kind\":\"drawing\",\"layer\":\"<id da camada>\"}}; set_cells {cut, track, cells:[{frame, cell:{\"type\":\"cel\",\"number\":1}|{\"type\":\"empty\"}|null}]} (quadro 0-based).\n\
- batch {commands:[...], label?}: vários comandos num único passo de undo (ids dados explicitamente em create_node permitem referenciar dentro do lote).\n\
Ferramentas de alto nível (açúcar sobre estes comandos, com validação e exemplo; prefira-as quando existirem): desenho aiai_draw_curves, aiai_fill, aiai_lint, aiai_paint, aiai_gradient, aiai_gradient_paint, aiai_figure, aiai_transform, aiai_repeat, aiai_erase; cenário e efeitos aiai_sky, aiai_pattern_fill, aiai_text, aiai_text_layer, aiai_layer_merge, aiai_lines, aiai_gag_bg, aiai_anime_fx (fogo, fumaça, raio), aiai_colorize_hints (colorir por rabiscos), aiai_particles; guias aiai_guide3d, aiai_perspective; personagem aiai_model_sheet, aiai_character_material, aiai_consistency; animação aiai_inbetween, aiai_propagate_fills; áudio aiai_tts_line, aiai_tts_kokoro, aiai_voice_profile, aiai_lipsync, aiai_sfx, aiai_bgm, aiai_import_audio, aiai_mix, aiai_loudness, aiai_audio_edit, aiai_audio_analyze; forma preenchida aiai_fill_shape; calibração aiai_compare, aiai_probe_color, aiai_measure_line, aiai_measure_edge; edição aiai_edit, aiai_edit_ops, aiai_edit_captions, aiai_edit_preview, aiai_edit_mix; saída aiai_render e aiai_export_video. Use aiai_apply para o que não tem ferramenta (criar nós, mover, timesheet, props cruas).\n\
`aiai_schema {\"which\":\"commands\"}` devolve o JSON Schema completo de todos os comandos.";

const RENDER_DESC: &str = r##"Renderiza um quadro de um corte com o motor CPU real (a MESMA imagem que o app mostra) e devolve a imagem como conteúdo de imagem MCP (reduzida a 1280 px no lado maior) e/ou grava o PNG em `out`. É a forma de VER o que você desenhou: renderize depois de cada passo importante e olhe a imagem.
Parâmetros: cut (id do corte, obrigatório), frame (0-based, padrão 0), layers (ids; só estas camadas, ex.: só a linha), background ("#rrggbb": achata sobre cor opaca; sem isso o fundo é transparente), include_guides / include_reference (mostram camadas de guia/referência), camera (padrão true = tamanho do projeto com a câmera do corte; false = canvas inteiro do corte, sem câmera), size [largura, altura] (reduz o PNG), out (caminho .png dentro das pastas permitidas), inline (true = devolve a imagem mesmo com out; sem out a imagem sempre volta).
Devolve {width, height, warnings, path?}: `warnings` lista o que NÃO foi desenhado (nunca ignore). Vários quadros numa imagem: aiai_render_sheet. `range` e backend gpu não existem aqui e dão erro claro."##;

const DRAW_DESC: &str = r##"Desenha curvas vetoriais (traços de linha) no cel exposto de uma camada VETORIAL no quadro dado. Entra no undo e aparece ao vivo no app. Devolve o Outcome (revision, created, can_undo/can_redo).
GUIA RÁPIDO
- Coordenadas são pixels do canvas do corte: origem no canto superior esquerdo, x para a direita, y PARA BAIXO. Rotação positiva é horária.
- Cada curva: {points: [[x,y] ou [x,y,pressão 0..1], ...], width (px, padrão 3), tip: g_pen (linha principal, afina nas pontas) | maru_pen (fina e uniforme) | marker (largura constante) | pencil, geometry: spline (padrão: passa pelos pontos, suave) | polyline (retas) | freehand | bezier, closed (fecha a curva), widths: [início, meio, fim] (multiplicadores da largura; ex. [0.3,1,0.3] afina as pontas), opacity, color}.
- color: {"type":"direct","rgba":[r,g,b,a]} (0..1) ou {"type":"palette","palette":"<id da paleta>","color":"<id da cor>"} (trocar a cor na paleta recolore tudo).
- A camada precisa ter um cel exposto no quadro (crie o cel com aiai_apply create_node {type:cel}). Camada travada recusa.
- mirror_x: x do eixo; cada curva também é desenhada espelhada nesse eixo (olhos, simetria de rosto).
- Poucos pontos bem escolhidos (4 a 8 por curva) com spline dão linhas limpas; feche formas com closed:true para o balde (aiai_fill) poder pintar o interior. Linha com buraco maior que `gap` do aiai_fill vaza a cor.
- Confirme sempre com aiai_render (só a camada de linha: layers:[id], background:"#ffffff")."##;

const FILL_DESC: &str = r##"Balde de tinta por paleta: pinta regiões fechadas pelas linhas. Para cada semente {x, y, color} acha a região fechada que contém (ou fica mais perto de) esse ponto, usando as camadas de linha da camada de cor (`line_refs` dela, ou `line_layers` se passado), fecha buracos de até `gap` px e expande `expand` px por baixo da linha (sem fresta branca). Grava a máscara no cel da camada de cor (camada fill; também aceita raster/vetor). Entra no undo.
Fluxo: 1) desenhe a linha com aiai_draw_curves; 2) aiai_lint lista as regiões sem cor com um ponto de cada; 3) aiai_fill com esses pontos e as cores (color: {"type":"palette","palette":id,"color":id} ou direta {"type":"direct","rgba":[r,g,b,a]}); 4) aiai_lint de novo (deve vir vazio) e aiai_render para ver. Semente fora do canvas dá erro; semente em cima da linha pinta a região mais perto, ou é ignorada com `skip_seeds_on_line: true` (recomendado para sementes calculadas: a região sem tinta aparece no lint em vez de pintar a vizinha errada). Pintar de novo a mesma região troca a cor; com `only_unpainted: true` a semente só pinta região ainda sem tinta (sementes de reserva, ex.: pele entre mechas que só existe separada em alguns ângulos). `close_leftovers: N` (opcional) fecha as frestas depois das sementes: cada região fechada sem tinta de até N px, longe da borda do quadro, recebe a cor que mais aparece em volta dela (cantos de olho, entre dedos); regiões maiores continuam no lint e pedem semente com a cor certa. `seeds` pode vir vazio com close_leftovers."##;

const PAINT_DESC: &str = r##"Pintura raster com pincel (aquarela, guache, aerógrafo, lápis...) no cel de uma camada RASTER, por pontos de passagem. Entra no undo (só os tiles tocados).
- preset: NOME de um preset de aiai_brush_presets (ex.: "Redondo duro"; sem diferenciar maiúsculas/acentos) ou o objeto completo do preset; `preset_overrides` (objeto) muda campos do preset (ex.: {"size": 40, "opacity": 0.6}).
- color: {"type":"direct","rgba":[r,g,b,a]} ou de paleta.
- strokes: [{points: [[x,y] ou [x,y,pressão], ...], smooth (padrão true: spline suave)}]; coordenadas em pixels do canvas (origem no canto superior esquerdo, y para baixo). seed: semente do ruído do pincel (mesma entrada = mesma pintura).
Confirme com aiai_render."##;

const GRADIENT_DESC: &str = r##"Degradê de dois pontos (ferramenta Degradê, como no Clip Studio). Pinta o cel de uma camada RASTER, ou cria uma CAMADA DE DEGRADÊ editável com `editable: true` (camada de preenchimento nova logo acima de `layer`, ou no topo com `layer` = o corte; devolve `layer`; com `layer` = uma camada de degradê, refaz os pontos dela e, se vier degradê, as paradas; `opacity`/`blend` viram os da camada). `kind`: linear (de `from` para `to`), radial (centro `from`, raio até `to`), angular (cônico em volta de `from`, começando na direção de `to`) ou diamond (losango em volta de `from`). O degradê: `stops` [[t 0..1, "#rrggbbaa"]...], ou `gradient` completo (paradas com cor de paleta e opacidade, `extend`, `space` srgb|linear|oklab), ou `preset` pelo nome: main_to_transparent, main_to_sub, stripe, sub_stripe (usam `main_color`/`sub_color`), os prontos de aiai_gradient_paint op presets, "Céu noturno", "Fim de tarde", "Céu azul". `extend` pad|repeat|reflect por cima. `selection` {type: rect|ellipse {rect:[x,y,w,h]}, lasso {points}, wand, color_range}: só dentro dela (na camada editável, a área da camada). Degradê livre: `freeform` [{at:[x,y], color}] (pontos de cor soltos, mistura suave no OKLab; ignora kind/from/to; sempre pixels: na raster, ou numa raster nova com `editable`). Degradê como cor de regiões de balde/texto: aiai_gradient_paint; `blend` (padrão normal) e `opacity`. Coordenadas em pixels do canvas (origem no canto superior esquerdo, y para baixo). Entra no undo. Confirme com aiai_render."##;

const FIGURE_DESC: &str = r##"Figura (como no Clip Studio): forma direta numa camada VETORIAL (linha = traço editável, preenchimento = região de preenchimento do cel embaixo dos traços), RASTER (pixels; `selection` limita) ou de PREENCHIMENTO (tudo vira região). `shape` (campo `type`): line {from,to} | curve {from,to,through} (curva de dois passos: passa por `through`) | polyline {points, closed} | continuous_curve {points, closed} (spline suave) | bezier {anchors:[{at:[x,y], handle?:[dx,dy]}], closed} (sem alça = canto) | rect {rect:[x,y,w,h], radius (cantos arredondados), angle} | ellipse {rect, angle} | polygon {center, radius:[rx,ry], sides 3..64, angle} | lasso {points} (laço de preenchimento: sempre preenche). `mode`: line | fill | line_fill (formas abertas só têm linha); `width` px, `color`, `fill_color` (padrão = color; o Clip Studio pinta o preenchimento de line_fill com a cor secundária), `tip` (padrão marker), `opacity`, `aliased`. Linhas de velocidade/foco: aiai_lines. Coordenadas em pixels do canvas. Um passo de undo. Confirme com aiai_render.
Exemplo: {"cut":"<corte>","layer":"<vetorial>","shape":{"type":"polygon","center":[960,540],"radius":[200,200],"sides":5},"mode":"line_fill","width":4,"color":{"type":"direct","rgba":[0,0,0,1]},"fill_color":{"type":"direct","rgba":[1,0.8,0.2,1]}}"##;


const TRANSFORM_DESC: &str = r##"Transforma o conteúdo de uma camada (vetorial: traços; raster: pixels), opcionalmente só dentro de uma `selection`. `deform` (campo `type`): affine {translate:[dx,dy], scale:[sx,sy], rotate (graus, horário positivo), shear:[x,y], pivot:[x,y]} | perspective {from:[4 cantos], to:[4 cantos]} (cantos: sup-esq, sup-dir, inf-dir, inf-esq) | mesh | liquify | flip {horizontal, axis}.
`selection`: {type: rect|ellipse {rect:[x,y,w,h]}, lasso {points}, wand {seed,tolerance}, color_range}. Vetor: `strokes` restringe a ids de traços; scale_width escala a largura junto. Coordenadas em pixels do canvas (y para baixo). Entra no undo. Confirme com aiai_render."##;

const REPEAT_DESC: &str = r##"Repete traços vetoriais ao longo de um caminho (cerca, folhagem, janelas de prédio, fios de cabelo): copia os traços `strokes` (ids dos traços do cel; o motivo é centrado em `origin` [x,y]) em posições espaçadas de `spacing` px ao longo de `path` (pontos [x,y]), com `offset`, `follow` (gira o motivo seguindo a tangente do caminho) e `scale_range` [escala no início, escala no fim] (perspectiva simples). Coordenadas em pixels do canvas. Entra no undo. Confirme com aiai_render."##;

const POINTS_DESC: &str = r##"Lê os pontos de controle de um traço VETORIAL. Escolha o traço por `stroke` (id) ou por `at` [x, y] em px do canvas (o traço mais perto, até `tolerance` px, padrão 12; com `stroke` e `at` juntos só mede a distância). Devolve {cel, stroke, stroke_rev, point_count, revision, geometry, closed, tip, width, hit_distance?, nearest_point?, nearest_point_id?, simplify?, points:[{id (estável: não muda quando outros pontos entram ou saem), index, x, y (no cel), canvas_x, canvas_y (no canvas, com o movimento da camada no quadro), pressure, corner, width (multiplicador), width_px (largura em px no ponto), handle / canvas_handle (alça efetiva [dx,dy]: a curva sai por +handle e chega por -handle; só spline/mão livre), handle_explicit (true = dada por set_handle; false = automática)}]}.
`simplify_preview` (px do cel): ensaio de simplificar — devolve `simplify` {removed (ids que sairiam), kept, max_deviation (≤ tolerância, medido na linha desenhada)}; pontas, cantos e pontos com alça explícita ficam. Não altera o documento.
Com o app aberto, aiai_ui get_state mostra o retângulo de cada ponto do traço escolhido na ferramenta Editar pontos (widgets point:N).
Exemplo: {"cut":"<corte>","layer":"<camada vetorial>","at":[412,230],"simplify_preview":1.0}"##;

const EDIT_POINTS_DESC: &str = r##"Edita os pontos de controle de UM traço vetorial em lote, num único passo de desfazer. Cada operação endereça o ponto por `id` (estável, de aiai_points; preferido) OU por `index` (posição no traço como ficou depois das operações anteriores do lote). Operações (campo `op`):
- move {id|index, x, y}
- insert {after_id|after, x, y}: põe o ponto logo depois desse; insert {x, y, tolerance?} sem after: põe o ponto EM CIMA da curva, no lugar mais perto de x,y (até `tolerance` px, padrão 12)
- delete {id|index} (o traço fica com pelo menos 2)
- corner {id|index, on} (canto: a curva quebra no ponto)
- width_px {id|index, px} (largura em px naquele ponto)
- set_handle {id|index, dx, dy}: alça explícita; a curva sai por +(dx,dy) e chega por -(dx,dy), mais comprida = mais aberta ali; só spline/mão livre e ponto que não é canto. clear_handle {id|index}: volta à automática
- simplify {tolerance}: tira os pontos que não seguram a forma sem a linha desenhada se afastar mais que `tolerance` px; pontas, cantos e alças explícitas ficam
`space`: cel (padrão; x/y/handle de aiai_points) ou canvas (canvas_x/canvas_y/canvas_handle); vale também para dx/dy, px e tolerâncias. `dry_run: true` devolve o traço como ficaria sem mudar nada (prévia de simplify).
GUARDA: operação por índice exige `stroke_rev` de aiai_points (ou `point_count`, ou `if_revision`); se o traço mudou desde a leitura, nada muda e volta o erro -32010 (leia de novo). Por id não precisa de guarda; id que não existe mais é erro claro. Devolve o Outcome + stroke_rev, point_count, geometry e points do traço como ficou, `inserted` e `removed` (ids) e `simplify` {removed, kept, max_deviation}.
Exemplo: {"cut":"<corte>","layer":"<camada>","stroke":"<traço>","ops":[{"op":"insert","after_id":1,"x":120,"y":80},{"op":"corner","id":2,"on":true},{"op":"set_handle","id":3,"dx":18,"dy":-4},{"op":"simplify","tolerance":0.8}]}"##;

const LINT_DESC: &str = r##"Confere o balde de tinta: lista as regiões fechadas pelas linhas (camadas em line_refs da camada de cor) que AINDA não têm cor. Devolve {regions_total, painted, unpainted:[{label, area, point:[x,y], bounds}]}; use `point` de cada região como semente de aiai_fill. Regiões menores que min_area são ignoradas; as que tocam a borda do quadro também, salvo `include_border: true`. `point` é o pixel mais interno da região (bom para PROBE_COLOR na referência). Se `unpainted` vier vazio, tudo está pintado. Se uma região sumiu ou virou uma só, há buraco na linha: aumente `gap` ou feche o traço."##;

pub(crate) fn schema_of<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap_or_else(|_| json!({ "type": "object" }))
}

/// Schema de aiai_edit_points: o da requisição + `if_revision`/`author` (lidos pela bridge ao aplicar).
fn edit_points_schema() -> Value {
    let mut s = schema_of::<aiai_tools::points::PointsEditRequest>();
    if let Some(props) = s.get_mut("properties").and_then(Value::as_object_mut) {
        props.insert("if_revision".into(), json!({ "type": "integer", "minimum": 0, "description": "Guarda alternativa: revisão do projeto que você viu por último." }));
        props.insert("author".into(), json!({ "type": "string" }));
    }
    s
}

/// Schema de aiai_paint: `preset` aceita também o nome (texto) e há `preset_overrides`.
fn paint_schema() -> Value {
    let mut s = schema_of::<aiai_tools::paint::PaintRequest>();
    let names: Vec<String> = aiai_paint::brush::BrushPreset::defaults().into_iter().map(|p| p.name).collect();
    if let Some(props) = s.get_mut("properties").and_then(Value::as_object_mut) {
        let full = props.get("preset").cloned().unwrap_or_else(|| json!({ "type": "object" }));
        props.insert(
            "preset".into(),
            json!({ "description": "Nome de um preset (aiai_brush_presets) ou o objeto completo do preset.", "anyOf": [ { "type": "string", "examples": names }, full ] }),
        );
        props.insert("preset_overrides".into(), json!({ "type": "object", "description": "Campos do preset a sobrescrever (ex.: {\"size\": 40})." }));
    }
    s
}

fn base_definitions() -> Value {
    json!([
        {
            "name": "aiai_status",
            "description": "Estado do AnimeIAI: com o app aberto (--bridge) devolve projeto, revisão, histórico, FPS/GPU; sem o app devolve o estado do modo headless (projeto em disco, se houver). Chame primeiro para saber se há app ou só headless.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "aiai_apply",
            "description": APPLY_DESC,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": { "type": "object", "description": "Comando do Command Bus: objeto com `kind` (create_node, set_props, delete_node, move_node, insert_strokes, set_tiles, set_cells, add_track, batch...).", "properties": { "kind": { "type": "string" } }, "required": ["kind"] },
                    "if_revision": { "type": "integer", "minimum": 0, "description": "Só aplica se a revisão atual do documento for esta." },
                    "author": { "type": "string", "description": "Autor no log (padrão agent:claude)." }
                },
                "required": ["command"],
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_query",
            "description": "Consulta o documento sem alterá-lo. `query` é um objeto com `kind`: project {include_nodes?}, node {id}, tree {id?, depth?} (árvore resumida: id, nome, tipo, filhos), roots {category}, xsheet {cut}, exposure {cut, frame}, change_frames {cut}, log {from?, limit?, include_commands?}, history, schema {which}, stats, validate. \
Confirme sempre o efeito de um aiai_apply lendo de volta (tree/node), nunca pelo 'ok' do retorno. `aiai_schema {\"which\":\"queries\"}` lista todas.",
            "inputSchema": {
                "type": "object",
                "properties": { "query": { "type": "object", "description": "Consulta: objeto com `kind`.", "properties": { "kind": { "type": "string" } }, "required": ["kind"] } },
                "required": ["query"],
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_undo",
            "description": "Desfaz o último passo do histórico (um batch conta como um passo). Devolve o Outcome com can_undo/can_redo. No modo headless via CLI não há histórico entre processos ('nada para desfazer'); dentro do servidor MCP o histórico vale enquanto o processo viver.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "aiai_redo",
            "description": "Refaz o último passo desfeito. Vale só até o próximo aiai_apply.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "aiai_log",
            "description": "Lê o log da sessão (quem fez o quê: seq, revision, op apply/undo/redo, description, author, time). `include_commands:true` inclui o comando de cada entrada. Pagina com from/limit; devolve `total`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "integer", "minimum": 0, "description": "Primeira entrada (padrão 0)." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Quantidade (padrão 100)." },
                    "include_commands": { "type": "boolean", "description": "Inclui o JSON do comando (padrão false)." }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_project",
            "description": "Arquivo do projeto, sempre com caminho explícito (sem diálogo do SO). op=new {name, path?, settings?, overwrite?}: cria projeto (com `path` já salva a pasta .aiai; recusa sobrescrever um projeto existente sem overwrite:true). op=open {path}: abre a pasta .aiai. op=save {path?}: salva (sem path, na pasta atual). \
Só grava em pastas permitidas (AIAI_MCP_ALLOWED_DIRS; padrão producoes/, calibracao/, sprints/registro/evidencias/, temp). Sem o app aberto roda em modo headless: depois de new/open as demais ferramentas de documento passam a atuar nesse projeto, e cada mudança é salva sozinha.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "op": { "type": "string", "enum": ["new", "open", "save"] },
                    "path": { "type": "string", "description": "Pasta do projeto (.aiai), caminho absoluto." },
                    "name": { "type": "string", "description": "Nome do projeto (op=new)." },
                    "settings": { "type": "object", "description": "op=new: width, height, fps_num, fps_den, dialogue_language, subtitle_language, audio_sample_rate (o que faltar fica no padrão 1920x1080, 24000/1001)." },
                    "overwrite": { "type": "boolean", "description": "op=new: permite recomeçar sobre um projeto existente." }
                },
                "required": ["op"],
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_render",
            "description": RENDER_DESC,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "cut": { "type": "string", "description": "Id do corte." },
                    "frame": { "type": "integer", "minimum": 0, "description": "Quadro 0-based (padrão 0)." },
                    "layers": { "type": "array", "items": { "type": "string" }, "description": "Ids de camadas: renderiza só estas (e as pastas que as contêm)." },
                    "background": { "type": "string", "description": "Cor de fundo opaca #rrggbb." },
                    "include_guides": { "type": "boolean" },
                    "include_reference": { "type": "boolean" },
                    "camera": { "type": "boolean", "description": "Padrão true. false = canvas inteiro do corte (coordenadas de canvas, sem câmera/multiplano)." },
                    "size": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "minItems": 2, "maxItems": 2, "description": "[largura, altura] do PNG de saída." },
                    "out": { "type": "string", "description": "Destino .png (dentro das pastas permitidas)." },
                    "inline": { "type": "boolean", "description": "Devolve a imagem MCP mesmo com `out`." }
                },
                "required": ["cut"],
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_screenshot",
            "description": "Captura a janela do app (framebuffer do egui) ou só um painel/retângulo. Devolve a imagem (reduzida a no máximo 1280 px no lado maior) e, com `out`, grava o PNG em tamanho real numa pasta permitida. `region` = {x,y,w,h} em pontos lógicos; `panel` = nome do painel (veja aiai_ui get_state). Exige o app aberto com --bridge.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "region": { "type": "object", "properties": { "x": { "type": "number" }, "y": { "type": "number" }, "w": { "type": "number" }, "h": { "type": "number" } }, "required": ["x", "y", "w", "h"] },
                    "panel": { "type": "string" },
                    "out": { "type": "string", "description": "Caminho do PNG a gravar (pasta permitida)." }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_input",
            "description": "Injeta eventos de entrada DENTRO do egui (sem tocar no cursor do SO), em pontos lógicos da janela (os retângulos vêm de aiai_ui get_state). `events` é uma lista de objetos com `kind`: mouse_move {x,y}, mouse_down/mouse_up {x,y,button: primary|secondary|middle}, wheel {dx,dy}, key {key (nome do egui: A, Z, Enter, Escape, F5...), pressed? (sem pressed = aperta e solta), modifiers?: [ctrl,shift,alt]}, text {text}, pen {x,y,pressure 0..1,phase: down|move|up} (caneta simulada com pressão), wait {frames}. Um clique é mouse_move + mouse_down + mouse_up no mesmo ponto; o app dá um quadro de intervalo entre o movimento e o clique. `wait:true` só responde depois que a fila de eventos terminou. Depois confirme o efeito com aiai_ui get_state / aiai_screenshot.",
            "inputSchema": {
                "type": "object",
                "properties": { "events": { "type": "array", "minItems": 1, "maxItems": 2000, "items": { "type": "object", "properties": { "kind": { "type": "string" } }, "required": ["kind"], "additionalProperties": true } }, "wait": { "type": "boolean", "description": "Responde só quando todos os eventos foram entregues." } },
                "required": ["events"],
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_ui",
            "description": "Estado e ações da interface. action=get_state devolve workspace, painéis (nome, visível, rect_logical/rect_physical/rect_screen), `widgets` (abas de workspace ws_tab:<chave>, menus menu:file|edit|view|window|help, itens abertos menu_item:..., ferramentas tool:<chave>, barras, diálogos), ferramenta ativa, foco, diálogos abertos, escala de DPI, tamanho/posição da janela e input_pending. Ações (UI_ACTION; campos extras vão junto): set_workspace {workspace}, select_tool {tool}, toggle_panel {panel, visible?}, set_theme {theme: light|dark}, set_language {language: pt|en}, set_window_size {width, height, maximized?}, open_dialog/close_dialog {dialog: about|prefs}. get_state traz também `overflow` (widgets cortados na horizontal pela borda do painel/janela; vazio = sem estouro). Exige o app aberto.",
            "inputSchema": {
                "type": "object",
                "properties": { "action": { "type": "string", "description": "get_state | set_workspace | select_tool | toggle_panel | set_theme | set_language | set_window_size | open_dialog | close_dialog" } },
                "required": ["action"],
                "additionalProperties": true
            }
        },
        {
            "name": "aiai_schema",
            "description": "Autodescrição: devolve o JSON Schema de `commands` (todos os comandos aceitos por aiai_apply), `queries` (todas as consultas de aiai_query), `project` (formato do documento) ou `tools` (as ferramentas MCP com descrição e inputSchema; `name` filtra uma, ex.: aiai_edit_points). Use para descobrir comandos novos sem reiniciar a sessão.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "which": { "type": "string", "enum": ["commands", "queries", "project", "tools"] },
                    "name": { "type": "string", "description": "Com which=tools: só esta ferramenta." }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_draw_curves",
            "description": DRAW_DESC,
            "inputSchema": schema_of::<aiai_tools::draw::DrawRequest>()
        },
        {
            "name": "aiai_fill",
            "description": FILL_DESC,
            "inputSchema": schema_of::<aiai_tools::fill::FillRequest>()
        },
        {
            "name": "aiai_lint",
            "description": LINT_DESC,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "cut": { "type": "string", "description": "Id do corte." },
                    "frame": { "type": "integer", "minimum": 0 },
                    "layer": { "type": "string", "description": "Id da camada de cor (fill) cujos line_refs delimitam as regiões." },
                    "gap": { "type": "number", "minimum": 0, "description": "Buraco na linha (px) ainda fechado. Padrão 3." },
                    "min_area": { "type": "integer", "minimum": 0, "description": "Área mínima (px) para listar uma região. Padrão 30." },
                    "include_border": { "type": "boolean", "description": "Também listar regiões que tocam a borda do quadro. Padrão false." }
                },
                "required": ["cut", "layer"],
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_paint",
            "description": PAINT_DESC,
            "inputSchema": paint_schema()
        },
        {
            "name": "aiai_gradient",
            "description": GRADIENT_DESC,
            "inputSchema": schema_of::<aiai_tools::paint::GradientRequest>()
        },
        {
            "name": "aiai_figure",
            "description": FIGURE_DESC,
            "inputSchema": schema_of::<aiai_tools::figure::FigureRequest>()
        },
        {
            "name": "aiai_transform",
            "description": TRANSFORM_DESC,
            "inputSchema": schema_of::<aiai_tools::transform::TransformRequest>()
        },
        {
            "name": "aiai_erase",
            "description": "Borracha vetorial numa camada VETORIAL: `path` (pontos [x,y] do canvas) e `radius` (px). `mode`: segment (padrão; apaga só o trecho coberto e divide o traço), intersection (apaga o trecho entre as interseções com outros traços mais próximas do toque) ou whole (apaga o traço inteiro). Os pedaços que sobram viram traços à mão livre com a mesma ponta, largura e cor. Um passo de undo.",
            "inputSchema": schema_of::<aiai_tools::erase::EraseVectorRequest>()
        },
        {
            "name": "aiai_points",
            "description": POINTS_DESC,
            "inputSchema": schema_of::<aiai_tools::points::PointsRequest>()
        },
        {
            "name": "aiai_edit_points",
            "description": EDIT_POINTS_DESC,
            "inputSchema": edit_points_schema()
        },
        {
            "name": "aiai_repeat",
            "description": REPEAT_DESC,
            "inputSchema": schema_of::<aiai_tools::transform::RepeatRequest>()
        },
        {
            "name": "aiai_brush_presets",
            "description": "Lista os presets de pincel raster (nome + parâmetros completos: ponta, tamanho, espaçamento, opacidade, fluxo, pressão, textura...). Use o `name` em aiai_paint {preset: \"<nome>\"} e ajuste com preset_overrides. Não altera o documento.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }
    ])
}

/// Catálogo completo: ferramentas genéricas + curadas de desenho + de produção (`tools_production`) + de pré-produção
/// (`tools_preprod`) + de satsuei/câmera/cor (`tools_satsuei`) + de prévia/entrega (`tools_preview`) + de edição e análise de áudio (`tools_audio`).
pub fn definitions() -> Value {
    let mut list = base_definitions().as_array().cloned().unwrap_or_default();
    list.extend(crate::tools_production::definitions());
    list.extend(crate::tools_preprod::definitions());
    list.extend(crate::tools_satsuei::definitions());
    list.extend(crate::tools_preview::definitions());
    list.extend(crate::tools_audio::definitions());
    list.extend(crate::tools_ruler::definitions());
    list.extend(crate::tools_line_fix::definitions());
    list.extend(crate::tools_fill_area::definitions());
    list.extend(crate::tools_model3d::definitions());
    list.extend(crate::tools_modeling::definitions());
    list.extend(crate::tools_library::definitions());
    list.extend(crate::tools_delivery::definitions());
    Value::Array(list)
}

fn args_object(args: &Value) -> Result<Map<String, Value>, ToolError> {
    match args {
        Value::Null => Ok(Map::new()),
        Value::Object(m) => Ok(m.clone()),
        _ => Err(ToolError::invalid("os argumentos devem ser um objeto JSON")),
    }
}

fn require<'a>(m: &'a Map<String, Value>, key: &str, what: &str) -> Result<&'a Value, ToolError> {
    m.get(key).filter(|v| !v.is_null()).ok_or_else(|| ToolError::invalid(format!("Missing '{key}': {what}")))
}

/// `frame` ausente = 0 (o primeiro quadro); os tipos de requisição o exigem.
fn with_frame(mut m: Map<String, Value>) -> Value {
    m.entry("frame").or_insert(json!(0));
    Value::Object(m)
}

fn required(m: &Map<String, Value>, keys: &[&str]) -> Result<(), ToolError> {
    for k in keys {
        require(m, k, "veja a descrição da ferramenta")?;
    }
    Ok(())
}

/// Copia só as chaves presentes.
fn pick(m: &Map<String, Value>, keys: &[&str]) -> Value {
    let mut out = Map::new();
    for k in keys {
        if let Some(v) = m.get(*k).filter(|v| !v.is_null()) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    Value::Object(out)
}

/// Ação da bridge, parâmetros e (para screenshot) destino do PNG, a partir do nome da ferramenta e dos argumentos.
pub struct Plan {
    pub action: &'static str,
    pub params: Value,
    pub save_png: Option<String>,
}

pub fn plan(name: &str, args: &Value) -> Result<Plan, ToolError> {
    let m = args_object(args)?;
    let simple = |action: &'static str, params: Value| Ok(Plan { action, params, save_png: None });
    if let Some(p) = crate::tools_production::plan(name, &m) {
        return p;
    }
    if let Some(p) = crate::tools_preprod::plan(name, &m) {
        return p;
    }
    if let Some(p) = crate::tools_satsuei::plan(name, &m) {
        return p;
    }
    if let Some(p) = crate::tools_preview::plan(name, &m) {
        return p;
    }
    if let Some(p) = crate::tools_audio::plan(name, &m) {
        return p;
    }
    if let Some(p) = crate::tools_ruler::plan(name, &m) {
        return p;
    }
    if let Some(p) = crate::tools_line_fix::plan(name, &m) {
        return p;
    }
    if let Some(p) = crate::tools_fill_area::plan(name, &m) {
        return p;
    }
    if let Some(p) = crate::tools_model3d::plan(name, &m) {
        return p;
    }
    if let Some(p) = crate::tools_modeling::plan(name, &m) {
        return p;
    }
    if let Some(p) = crate::tools_library::plan(name, &m) {
        return p;
    }
    if let Some(p) = crate::tools_delivery::plan(name, &m) {
        return p;
    }
    match name {
        "aiai_status" => simple("GET_STATUS", json!({})),
        "aiai_apply" => {
            let cmd = require(&m, "command", "objeto {\"kind\": ...}")?;
            if !cmd.is_object() {
                return Err(ToolError::invalid("'command' deve ser um objeto {\"kind\": ...}"));
            }
            simple("APPLY_COMMAND", pick(&m, &["command", "if_revision", "author"]))
        }
        "aiai_query" => {
            let q = require(&m, "query", "objeto {\"kind\": ...}")?;
            if !q.is_object() {
                return Err(ToolError::invalid("'query' deve ser um objeto {\"kind\": ...}"));
            }
            simple("QUERY", pick(&m, &["query"]))
        }
        "aiai_undo" => simple("UNDO", json!({})),
        "aiai_redo" => simple("REDO", json!({})),
        "aiai_log" => simple("GET_LOG", pick(&m, &["from", "limit", "include_commands"])),
        "aiai_project" => {
            let op = require(&m, "op", "new | open | save")?.as_str().unwrap_or("");
            let action = match op {
                "new" => "NEW_PROJECT",
                "open" => "OPEN_PROJECT",
                "save" => "SAVE_PROJECT",
                other => return Err(ToolError::invalid(format!("Invalid action: op deve ser new|open|save, veio {other:?}"))),
            };
            if op == "open" {
                require(&m, "path", "pasta .aiai a abrir")?;
            }
            simple(action, pick(&m, &["path", "name", "settings", "overwrite"]))
        }
        "aiai_render" => {
            require(&m, "cut", "id do corte")?;
            simple("RENDER", Value::Object(m))
        }
        "aiai_draw_curves" => {
            required(&m, &["cut", "layer", "curves"])?;
            if !m.get("curves").is_some_and(|c| c.as_array().is_some_and(|a| !a.is_empty())) {
                return Err(ToolError::invalid("'curves' deve ser uma lista não vazia de curvas {points, width, ...}"));
            }
            simple("DRAW_CURVES", with_frame(m))
        }
        "aiai_fill" => {
            required(&m, &["cut", "layer", "seeds"])?;
            simple("FILL", with_frame(m))
        }
        "aiai_lint" => {
            required(&m, &["cut", "layer"])?;
            simple("LINT", Value::Object(m))
        }
        "aiai_paint" => {
            required(&m, &["cut", "layer", "preset", "color", "strokes"])?;
            simple("PAINT", with_frame(m))
        }
        "aiai_gradient" => {
            required(&m, &["cut", "layer"])?;
            simple("GRADIENT", with_frame(m))
        }
        "aiai_figure" => {
            required(&m, &["cut", "layer", "shape", "color"])?;
            simple("FIGURE", with_frame(m))
        }
        "aiai_transform" => {
            required(&m, &["cut", "layer", "deform"])?;
            simple("TRANSFORM", with_frame(m))
        }
        "aiai_erase" => {
            required(&m, &["cut", "layer", "path", "radius"])?;
            simple("ERASE_VECTOR", with_frame(m))
        }
        "aiai_repeat" => {
            required(&m, &["cut", "layer", "strokes", "path"])?;
            simple("REPEAT", with_frame(m))
        }
        "aiai_points" => {
            required(&m, &["cut", "layer"])?;
            simple("READ_POINTS", with_frame(m))
        }
        "aiai_edit_points" => {
            required(&m, &["cut", "layer", "stroke", "ops"])?;
            simple("EDIT_POINTS", with_frame(m))
        }
        "aiai_brush_presets" => simple("BRUSH_PRESETS", json!({})),
        "aiai_screenshot" => {
            let mut params = pick(&m, &["region", "panel"]);
            // o PNG volta em base64 e é o MCP quem grava em `out` (já validado pelo sandbox)
            params["inline"] = json!(true);
            let save_png = m.get("out").and_then(Value::as_str).map(str::to_string);
            Ok(Plan { action: "SCREENSHOT", params, save_png })
        }
        "aiai_input" => {
            let ev = require(&m, "events", "lista de eventos")?;
            if !ev.as_array().is_some_and(|a| !a.is_empty()) {
                return Err(ToolError::invalid("'events' deve ser uma lista não vazia de objetos com `kind`"));
            }
            simple("INPUT", pick(&m, &["events", "wait"]))
        }
        "aiai_ui" => {
            let action = require(&m, "action", "get_state | set_workspace | ...")?.as_str().ok_or_else(|| ToolError::invalid("'action' deve ser texto"))?;
            if action == "get_state" {
                simple("GET_UI_STATE", json!({}))
            } else {
                simple("UI_ACTION", Value::Object(m))
            }
        }
        "aiai_schema" => {
            let which = m.get("which").and_then(Value::as_str).unwrap_or("commands");
            simple("QUERY", json!({ "query": { "kind": "schema", "which": which } }))
        }
        other => Err(ToolError::invalid(format!("Unknown tool: {other}"))),
    }
}

/// Executa uma ferramenta: monta o plano, roteia (app/headless) e formata o conteúdo MCP (texto + imagem opcional).
pub fn call_tool(ctx: &mut Ctx, name: &str, args: &Value) -> ToolResult {
    match run(ctx, name, args) {
        Ok(r) => r,
        Err(e) => ToolResult::error(&e),
    }
}

fn run(ctx: &mut Ctx, name: &str, args: &Value) -> Result<ToolResult, ToolError> {
    if let Some(mut tools) = schema_tools(name, args)? {
        return finish(&mut tools, None);
    }
    let plan = plan(name, args)?;
    let save_to: Option<PathBuf> = match &plan.save_png {
        Some(p) => Some(ctx.sandbox.check_write(p).map_err(|e| ToolError::invalid(format!("'out': {e}")))?),
        None => None,
    };
    let mut data = ctx.invoke(plan.action, plan.params, None)?;
    finish(&mut data, save_to.as_ref())
}

/// `aiai_schema {which: "tools", name?}`: o catálogo de ferramentas (sem passar pela bridge). `None` = outro pedido.
pub fn schema_tools(name: &str, args: &Value) -> Result<Option<Value>, ToolError> {
    if name != "aiai_schema" || args.get("which").and_then(Value::as_str) != Some("tools") {
        return Ok(None);
    }
    let all = definitions();
    let list = all.as_array().cloned().unwrap_or_default();
    match args.get("name").and_then(Value::as_str) {
        None => Ok(Some(json!({ "tools": list }))),
        Some(n) => match list.into_iter().find(|t| t["name"] == n) {
            Some(t) => Ok(Some(t)),
            None => Err(ToolError::invalid(format!("aiai_schema: ferramenta desconhecida {n:?}"))),
        },
    }
}

/// Formata a resposta: imagem (se houver `png_base64`) reduzida + texto com o resto.
pub fn finish(data: &mut Value, save_to: Option<&PathBuf>) -> Result<ToolResult, ToolError> {
    let png = take_png(data, save_to)?;
    let mut content = vec![json!({"type": "text", "text": serde_json::to_string(data).unwrap_or_default()})];
    if let Some(p) = png {
        content.push(mcp_image(&p.bytes));
    }
    Ok(ToolResult { content, is_error: false })
}
