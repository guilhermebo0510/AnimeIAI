//! Ferramentas curadas de satsuei, câmera e cor (S13/S15) com schema tipado: efeitos por camada, cor do corte, câmera
//! (valores, chaves, movimento, tremor) e movimento/profundidade de camada (multiplano). Açúcar sobre
//! `aiai_bridge::satsuei_tools`; rodam no app e no headless.

use crate::context::ToolError;
use crate::tools::Plan;
use crate::tools_preprod::{object_schema, required};
use serde_json::{Map, Value, json};

const EFFECTS_DESC: &str = r##"Pilha de efeitos de satsuei de uma camada (schema tipado de cada efeito em `effect`: gaussian_blur, directional_blur, radial_blur, color_aware_blur, glow, diffusion, halation, transmitted_light, light_wrap, rim_light, para, chromatic_aberration, vignette, grain, deband, lens_flare, god_rays, drop_shadow, outline, color_overlay, invert, two_tone, adjust, grade...). Numa camada comum o efeito vale para o conteúdo DELA (glow de um brilho, contorno, sombra, rim light); numa camada do tipo `effect` (crie com aiai_apply create_node {type:"layer",kind:"effect"}) vale para tudo o que está abaixo dela. `op`:
- list {layer}
- add {layer, effect:{type,...}, index?, posterize_time? (2/3 = efeito em 2s/3s), keys? ({"intensity":[{frame,value,ease}]})}
- update {layer, index, effect?, enabled?, posterize_time?, keys?} / remove {layer, index} / clear {layer}
- set {layer, effects:[{effect, enabled, posterize_time, keys}]} (substitui tudo)
- key {layer, index, param, keys:[{frame,value,ease?}]} → anima UM parâmetro (lista vazia = volta ao valor fixo). `param` é o caminho numérico no efeito: "radius", "intensity", "center.0" (x de [x,y]), "color.2" (azul), "adjustment.hue". Sem chave o efeito é estático; com chave o render avalia o valor em cada quadro (respeitando posterize_time). Caminho que não é número do efeito é recusado (a mensagem lista os válidos).
- ease {layer, index, param?, frames?, interpolation? | ease?} → interpolação das chaves (todas, ou só `param`/`frames`): interpolation = linear | hold | easy_ease (F9 do After Effects: chega e sai devagar) | easy_ease_in | easy_ease_out; ou `ease` cru: linear|hold|ease_in|ease_out|ease_in_out|{"type":"bezier",x1,y1,x2,y2} (vale do quadro da chave até a próxima)
Um passo de undo por chamada. Confirme com aiai_render (em dois quadros, se animou).
Exemplo: {"op":"add","layer":"<camada do brilho>","effect":{"type":"glow","threshold":0.6,"radius":18,"intensity":0.8}}
Exemplo (raio do desfoque de 0 a 12 em 10 quadros, com Easy Ease): {"op":"key","layer":"<camada>","index":0,"param":"radius","keys":[{"frame":0,"value":0},{"frame":10,"value":12}]} e depois {"op":"ease","layer":"<camada>","index":0,"interpolation":"easy_ease"}"##;

const GRADE_DESC: &str = r##"Cor do corte: grava a pilha `effects` numa camada de efeito no TOPO do corte (criada se faltar e sempre movida para o topo; nome padrão "Grade"), então vale para o quadro inteiro. Use `grade` {lift,gamma,gain [r,g,b], saturation, temperature, tint, contrast, pivot}, `adjust` (níveis/curvas/HSL), `vignette`, `diffusion`, `grain`. `opacity` mistura a cor. Lista vazia limpa. Devolve o id da camada.
Exemplo: {"cut":"<corte>","effects":[{"effect":{"type":"grade","gain":[1.05,1.0,0.92],"saturation":1.1,"temperature":0.15,"contrast":1.08}},{"effect":{"type":"vignette","amount":0.25,"softness":0.6}}]}"##;

const CAMERA_DESC: &str = r##"Câmera do corte (o quadro de saída que olha para o canvas). Centro em PIXELS do canvas, zoom 1 = quadro do projeto (2 = metade da área, mais perto), rotação em graus. Camadas com `depth` > 0 (aiai_layer_motion) se movem 1/(1+depth) do pan/zoom: multiplano. `op`:
- read {cut}
- set {cut, center?, clear_center?, zoom?, rotation?, shake_seed?} (valores fixos)
- move {cut, from:{center,zoom,rotation?}, to:{...}, start?, end?, ease? (linear|hold|ease_in|ease_out|ease_in_out|{bezier:{x1,y1,x2,y2}}; padrão ease_in_out)} → pan/tilt/zoom/rotação; parada antes de start e depois de end
- shake {cut, amplitude (px), start, end, decay? (padrão true), seed?} → tremor de impacto
- keys {cut, property: center_x|center_y|zoom|rotation|shake, keys:[{frame,value,ease}]} (lista vazia apaga) / clear_keys {cut}
Para ver o enquadramento, aiai_render com camera true (padrão); com camera false vê o canvas inteiro.
Exemplo: {"op":"move","cut":"<corte>","from":{"center":[700,540],"zoom":1.2},"to":{"center":[1220,540],"zoom":1.2},"start":0,"end":71}"##;

const LAYER_MOTION_DESC: &str = r##"Posição, profundidade (multiplano) e chaves de uma camada. `op`:
- read {layer}
- set {layer, offset?:[x,y] px, depth? (0 = plano da câmera; 1 = move metade do pan; 4 = céu quase parado), opacity?, scale? (1 = tamanho do canvas), rotation? (graus, horário), anchor?:[x,y] (pivô da escala/rotação em px a partir do centro do canvas)} → valores fixos, usados quando a propriedade não tem chave
- slide {layer, from:[x,y], to:[x,y], start, end, ease?} → desliza a camada (cenário maior que o quadro, objeto que passa)
- keys {layer, property: offset_x|offset_y|opacity|rotation|scale|anchor_x|anchor_y, keys:[{frame,value,ease}]} / clear_keys {layer}
- ease {layer, property?, frames?, interpolation? | ease?} → interpolação das chaves (todas, ou só `property`/`frames`): interpolation = linear | hold | easy_ease (F9 do After Effects) | easy_ease_in | easy_ease_out; ou `ease` cru (linear|hold|ease_in|ease_out|ease_in_out|{"type":"bezier",x1,y1,x2,y2}). O `ease` de uma chave vale do quadro dela até a próxima; easy_ease mexe na saída da chave e na chegada do trecho anterior (alças da Bézier).
Estas são as mesmas chaves que a Linha do tempo da composição mostra (losangos) e edita.
Exemplo: {"op":"set","layer":"<BG céu>","depth":4}
Exemplo: {"op":"keys","layer":"<camada>","property":"rotation","keys":[{"frame":0,"value":0},{"frame":12,"value":90}]} e {"op":"ease","layer":"<camada>","property":"rotation","interpolation":"easy_ease"}"##;

pub fn definitions() -> Vec<Value> {
    use aiai_bridge::satsuei_tools as st;
    vec![
        json!({ "name": "aiai_effects", "description": EFFECTS_DESC, "inputSchema": object_schema::<st::EffectsRequest>() }),
        json!({ "name": "aiai_grade", "description": GRADE_DESC, "inputSchema": object_schema::<st::GradeRequest>() }),
        json!({ "name": "aiai_camera", "description": CAMERA_DESC, "inputSchema": object_schema::<st::CameraRequest>() }),
        json!({ "name": "aiai_layer_motion", "description": LAYER_MOTION_DESC, "inputSchema": object_schema::<st::LayerMotionRequest>() }),
    ]
}

/// Plano de uma ferramenta de satsuei/câmera/cor; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    let with_req = |keys: &[&str], action: &'static str| -> Result<Plan, ToolError> {
        required(m, keys)?;
        Ok(Plan { action, params: Value::Object(m.clone()), save_png: None })
    };
    Some(match name {
        "aiai_effects" => with_req(&["op", "layer"], "EFFECTS"),
        "aiai_grade" => with_req(&["cut", "effects"], "GRADE"),
        "aiai_camera" => with_req(&["op", "cut"], "CAMERA"),
        "aiai_layer_motion" => with_req(&["op", "layer"], "LAYER_MOTION"),
        _ => return None,
    })
}
