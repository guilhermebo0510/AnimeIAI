//! Ferramenta curada da Modelagem (S24 3D, fase 12): `aiai_modeling` faz adereços simples por comando (primitivas,
//! extrude, inset, mover/escalar faces, bevel, modificadores, cor toon) num ativo 3D editável do projeto, manda para o
//! Layout como camada 3D e exporta .glb. Açúcar sobre `aiai_bridge::modeling_tools`; roda no app e no headless.

use crate::context::ToolError;
use crate::tools::Plan;
use crate::tools_preprod::{object_schema, required};
use serde_json::{Map, Value};

const MODELING_DESC: &str = r##"MODELAGEM 3D simples (como o modo Edição do Blender, para adereços e blocagem que viram referência de desenho). O modelo é um ativo 3D editável do projeto: cada op lê, muda e regrava o ativo (um passo de undo) e o GLB que a camada 3D usa. Metros, Y para cima; faces e deslocamentos nos eixos do objeto. `op`:
- new {name, primitive: cube|sphere|cylinder|cone|plane|torus, size? (m, padrão 1), pos?, rot_deg?, scale?, color? "#rrggbb"} → cria o modelo com o primeiro objeto; devolve `asset`.
- add_primitive {asset, primitive, size?, pos?, rot_deg?, scale?, name?, color?} → outro objeto no mesmo modelo.
- info {asset} → objetos (nome, transformação, cor, modificadores, caixa) e a lista de faces com normal e centro (para escolher o que extrudar). list → modelos editáveis.
- transform {asset, object?, pos?, rot_deg?, scale?} → o objeto inteiro (object = índice ou nome; padrão o último).
- extrude {asset, object?, faces?, distance} · inset {asset, object?, faces?, thickness, depth?} · move_faces {asset, object?, faces?, offset: [x,y,z]} · scale_faces {asset, object?, faces?, factor: [x,y,z]} (em volta do centro das faces; 0 num eixo achata). `faces`: índices, "all" ou uma direção ("+y"/"top", "-y", "+x", "-x", "+z"/"front", "-z"); padrão "+y". Depois de um extrude, a tampa nova continua sendo a face da direção (ex.: "+y").
- bevel {asset, object?, edges?: "all" | [[a,b],...], width, segments? (1 = chanfro)} · merge_by_distance {asset, object?, merge_distance? (padrão 0,0001)} (funde vértices que ficaram no mesmo lugar)
- modifier {asset, object?, add?: mirror|subsurf|array, axes?: [x,y,z] (mirror), levels? 1..3 (subsurf), count? e array_offset? (array), remove?: posição na pilha}
- material {asset, object?, color? "#rrggbb", shading?: auto|flat|smooth} · delete_object {asset, object?}
- send {asset, cut} → cria a camada 3D do modelo no corte (Enviar ao Layout; confira com aiai_render include_reference:true). export {asset, path: absoluto .glb}
Exemplo (casa com telhado de duas águas): {"op":"new","name":"Casa","primitive":"cube","pos":[0,0.5,0]} → {"op":"extrude","asset":"<id>","faces":"top","distance":0.6} → {"op":"scale_faces","asset":"<id>","faces":"top","factor":[0,1,1]} → {"op":"merge_by_distance","asset":"<id>"} → {"op":"material","asset":"<id>","color":"#c86448"} → {"op":"send","asset":"<id>","cut":"<corte>"}"##;

pub fn definitions() -> Vec<Value> {
    vec![serde_json::json!({ "name": "aiai_modeling", "description": MODELING_DESC, "inputSchema": object_schema::<aiai_tools::modeling::ModelingRequest>() })]
}

/// Plano da ferramenta de modelagem; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    match name {
        "aiai_modeling" => Some(required(m, &["op"]).map(|_| Plan { action: "MODELING", params: Value::Object(m.clone()), save_png: None })),
        _ => None,
    }
}
