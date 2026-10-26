//! Ferramenta curada do balde do S25 (à moda do Clip Studio): `aiai_fill_area` cobre os jeitos do balde que não são o
//! clique de tinta-e-pinta (`aiai_fill`): só a camada em edição, cercar e preencher, pintar área não preenchida. Açúcar
//! sobre `aiai_bridge::fill_area_tools`; roda no app e no headless.

use crate::context::ToolError;
use crate::tools::Plan;
use crate::tools_preprod::{object_schema, required};
use serde_json::{Map, Value};

const FILL_AREA_DESC: &str = r##"Balde como o do Clip Studio, além do clique de tinta-e-pinta do aiai_fill, num passo de undo. Coordenadas em px do cel da camada `layer` (sem o movimento de camada, como o aiai_fill). `color` = cor direta {"rgba":[r,g,b,a]} ou de paleta. `mode`:
- editing {point:[x,y], tolerance? 0..255 (10), refer_all? (false), gap? px (3), scale? px (0; negativo contrai)} → as cores da própria camada (ou de todas as camadas visíveis com refer_all) delimitam a região do clique; numa camada RASTER a cor entra nos pixels, numa vetorial/de preenchimento vira região de preenchimento
- enclose {path:[[x,y]...] (laço), min_inside? 0..1 (0,95), gap?, expand? (2), line_layers?} → pinta as regiões fechadas pelas linhas que o laço cerca (a de fora, que só entra em parte, fica)
- unfilled {path (caminho do pincel), radius? px (20), gap?, expand?, line_layers?} → pinta as regiões AINDA SEM TINTA que o pincel toca (frestas que o balde deixou)
As linhas de enclose/unfilled: `line_layers`, senão os `line_refs` da camada, senão a própria camada. Devolve o Outcome + `regions`.
Exemplo: {"cut":"<corte>","layer":"<camada de cor>","frame":0,"mode":"enclose","color":{"rgba":[1,0.8,0.7,1]},"path":[[100,100],[300,100],[300,300],[100,300]]}"##;

pub fn definitions() -> Vec<Value> {
    vec![serde_json::json!({ "name": "aiai_fill_area", "description": FILL_AREA_DESC, "inputSchema": object_schema::<aiai_tools::fill_area::FillAreaRequest>() })]
}

/// Plano da ferramenta; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    match name {
        "aiai_fill_area" => Some(required(m, &["cut", "layer", "color"]).map(|_| {
            let mut params = m.clone();
            params.entry("frame").or_insert(Value::from(0));
            Plan { action: "FILL_AREA", params: Value::Object(params), save_png: None }
        })),
        _ => None,
    }
}
