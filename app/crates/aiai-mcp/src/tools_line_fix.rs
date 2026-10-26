//! Ferramenta curada do Corrigir linha (S25, à moda do Clip Studio): `aiai_line_fix` puxa, simplifica, conecta, ajusta a
//! largura e redesenha linhas vetoriais já desenhadas, e remove poeira (raster ou vetorial). Açúcar sobre
//! `aiai_bridge::line_fix_tools`; roda no app e no headless.

use crate::context::ToolError;
use crate::tools::Plan;
use crate::tools_preprod::{object_schema, required};
use serde_json::{Map, Value};

const LINE_FIX_DESC: &str = r##"Corrigir linha (como o grupo Corrigir linha do Clip Studio) numa camada (`layer`) do corte (`cut`, `frame`), num passo de undo. Coordenadas em px do CANVAS (o movimento da camada é desfeito). `op`:
- pinch {from:[x,y], to:[x,y], stroke?, range? (px de curva para cada lado, padrão 80), level? (0 = bico, 1 = arco largo; 0,5), fix_ends?} → o lugar da linha mais perto de `from` vai até `to` e a linha em volta acompanha com queda suave (sem `stroke`, o traço a até `tolerance` (12) de `from`)
- simplify {path:[[x,y]...], radius? (15), strength? 0..100 (25) | tolerance? px} → tira os pontos debaixo do pincel que não seguram a forma (a linha não sai mais que a tolerância)
- connect {path, radius?, distance? (24), smooth? (true)} → liga as pontas (de traços abertos) debaixo do pincel que estão a até `distance` uma da outra; duas pontas do mesmo traço o fecham. O mais comprido dá cor/ponta/id; as larguras em px ficam
- width {path, radius?, mode: widen|narrow|scale|fixed, amount (px; fator no scale), whole_line?} → engrossa/afina/escala/fixa a largura debaixo do pincel com queda na borda (ou a linha inteira de cada traço tocado)
- redraw {path, pressure?:[0..1 por ponto], tolerance? (12)} → o caminho deve começar e terminar em cima do MESMO traço: o trecho entre os dois lugares é trocado pelo caminho (emendado na curva)
- redraw_width {path, pressure, radius?} → a largura debaixo do caminho segue a pressão (0,12 + 0,88·p^0,8 da largura do traço)
- dust {size? px (6), dust_mode?: opaque|holes|both, rect?:[x0,y0,x1,y1]} → camada raster: tira pontinhos soltos (e/ou tapa furos fechados) com caixa até `size`; camada vetorial: apaga traços desse tamanho. `dry_run` só conta e devolve `dust_boxes`
Vale em camada VETORIAL (dust também em raster). `dry_run` devolve o relatório sem mudar o documento. Devolve o Outcome + `changed`, `removed`, `removed_points`, `joins`, `dust`, `dust_boxes`.
Exemplo: {"cut":"<corte>","layer":"<camada>","frame":0,"op":"pinch","from":[320,240],"to":[320,200],"range":120}"##;

pub fn definitions() -> Vec<Value> {
    vec![serde_json::json!({ "name": "aiai_line_fix", "description": LINE_FIX_DESC, "inputSchema": object_schema::<aiai_tools::line_fix::LineFixRequest>() })]
}

/// Plano da ferramenta; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    match name {
        "aiai_line_fix" => Some(required(m, &["cut", "layer", "op"]).map(|_| {
            let mut params = m.clone();
            params.entry("frame").or_insert(Value::from(0));
            Plan { action: "LINE_FIX", params: Value::Object(params), save_png: None }
        })),
        _ => None,
    }
}
