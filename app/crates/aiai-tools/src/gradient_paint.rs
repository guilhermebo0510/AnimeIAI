//! Degradê como COR (`aiai_gradient_paint`): aplica um `Gradient` onde o programa aceita cor —
//! - `fill_regions`: regiões de preenchimento (balde) do cel exposto, as que contêm os `points` (ou todas com `all`);
//!   o degradê fica na região (mapeado na caixa dela) e o render o pinta ao vivo; `gradient: null` volta à cor chapada;
//! - `text_fill` / `text_stroke`: preenchimento ou contorno de uma camada de texto (mapeado na caixa das letras);
//! - `palette_add`: guarda o degradê com nome numa paleta (vai para a biblioteca junto com ela); `palette_remove` tira;
//! - `presets`: lista os degradês prontos (não altera nada).
//!
//! A ferramenta `aiai_gradient` continua pintando degradês raster de dois pontos (agora com `gradient` completo também).
//! O pincel aceita o degradê em `preset.color_ramp` (ao longo do traço).

use crate::context::*;
use crate::fill::fill_covers;
use aiai_core::*;
use aiai_paint::text_layer::{TextParams, TextStroke};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum GradientPaintOp {
    #[default]
    FillRegions,
    TextFill,
    TextStroke,
    PaletteAdd,
    PaletteRemove,
    Presets,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct GradientPaintRequest {
    #[serde(default)]
    pub op: GradientPaintOp,
    /// O degradê (paradas com cor direta ou de paleta). `null`/ausente em fill_regions/text_* = volta à cor chapada.
    #[serde(default)]
    pub gradient: Option<Gradient>,
    /// Nome de um degradê pronto (ver op `presets`) no lugar de `gradient`.
    #[serde(default)]
    pub preset: Option<String>,
    /// fill_regions: corte, quadro e camada (fill, vetorial ou raster com regiões de balde).
    #[serde(default)]
    pub cut: Option<Id>,
    #[serde(default)]
    pub frame: u32,
    #[serde(default)]
    pub layer: Option<Id>,
    /// fill_regions: pontos [x, y] do canvas dentro das regiões.
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    /// fill_regions: todas as regiões do cel.
    #[serde(default)]
    pub all: bool,
    /// palette_add/palette_remove: id da paleta.
    #[serde(default)]
    pub palette: Option<Id>,
    /// palette_add: nome; palette_remove: nome ou id do degradê.
    #[serde(default)]
    pub name: Option<String>,
    /// text_stroke sem contorno ainda: espessura do contorno criado (px).
    #[serde(default)]
    pub width: Option<f32>,
}

/// Resultado: o comando (se muda o documento) e o que devolver.
pub struct GradientPaintPlan {
    pub command: Option<Command>,
    pub value: Value,
}

pub fn presets_json() -> Value {
    json!({ "presets": aiai_core::paint::builtin_gradients().into_iter().map(|(n, g)| json!({ "name": n, "gradient": g })).collect::<Vec<_>>() })
}

fn gradient_of(req: &GradientPaintRequest) -> ToolResult<Option<Gradient>> {
    if let Some(name) = &req.preset {
        let all = aiai_core::paint::builtin_gradients();
        let g = all.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())).map(|(_, g)| g.clone());
        return match g {
            Some(g) => Ok(Some(g)),
            None => Err(ToolError::Invalid(format!("degradê pronto '{name}' não existe; prontos: {}", all.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")))),
        };
    }
    if let Some(g) = &req.gradient {
        g.validate().map_err(ToolError::Invalid)?;
        // paradas de paleta precisam existir
        return Ok(Some(g.clone().sanitized()));
    }
    Ok(None)
}

fn check_palette_refs(p: &Project, g: &Gradient) -> ToolResult<()> {
    for s in &g.stops {
        crate::paint::resolve_color(p, &s.color)?;
    }
    Ok(())
}

pub fn gradient_paint(p: &Project, blobs: &BlobStore, ids: &mut IdGen, req: &GradientPaintRequest) -> ToolResult<GradientPaintPlan> {
    if req.op == GradientPaintOp::Presets {
        return Ok(GradientPaintPlan { command: None, value: presets_json() });
    }
    let g = gradient_of(req)?;
    if let Some(g) = &g {
        check_palette_refs(p, g)?;
    }
    match req.op {
        GradientPaintOp::Presets => unreachable!(),
        GradientPaintOp::FillRegions => {
            let cut = req.cut.as_ref().ok_or_else(|| ToolError::Invalid("Missing 'cut'".into()))?;
            let layer = req.layer.as_ref().ok_or_else(|| ToolError::Invalid("Missing 'layer'".into()))?;
            let cd = cut_data(p, cut)?;
            let cel_node = exposed_cel(p, cd, layer, req.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {}", req.frame)))?;
            let NodeKind::Cel(cel) = &cel_node.data else { return Err(ToolError::Invalid("não é cel".into())) };
            if cel.fills.is_empty() {
                return Err(ToolError::Invalid("o cel não tem regiões de preenchimento (pinte com aiai_fill antes)".into()));
            }
            if !req.all && req.points.is_empty() {
                return Err(ToolError::Invalid("informe `points` dentro das regiões ou `all: true`".into()));
            }
            let mut fills = cel.fills.clone();
            let mut hit = 0usize;
            for f in &mut fills {
                let take = req.all || req.points.iter().any(|q| q[0] >= 0.0 && q[1] >= 0.0 && fill_covers(blobs, f, q[0] as u32, q[1] as u32));
                if take {
                    f.gradient = g.clone();
                    hit += 1;
                }
            }
            if hit == 0 {
                return Err(ToolError::Invalid("nenhuma região contém os pontos".into()));
            }
            Ok(GradientPaintPlan { command: Some(Command::ReplaceFills { cel: cel_node.id.clone(), fills }), value: json!({ "regions": hit }) })
        }
        GradientPaintOp::TextFill | GradientPaintOp::TextStroke => {
            let layer = req.layer.as_ref().ok_or_else(|| ToolError::Invalid("Missing 'layer' (camada de texto)".into()))?;
            let l = layer_data(p, layer)?;
            if l.kind != LayerType::Text {
                return Err(ToolError::Invalid(format!("{layer} não é camada de texto")));
            }
            let mut tp = TextParams::from_layer_params(&l.params).transpose().map_err(ToolError::Invalid)?.unwrap_or_default();
            let paint = g.map(|gradient| Paint::Gradient { gradient });
            if req.op == GradientPaintOp::TextFill {
                tp.fill = paint;
            } else {
                let st = tp.stroke.get_or_insert_with(|| TextStroke { width: req.width.unwrap_or(4.0), ..Default::default() });
                if let Some(w) = req.width {
                    st.width = w;
                }
                st.paint = paint;
            }
            Ok(GradientPaintPlan { command: Some(Command::SetProps { id: layer.clone(), name: None, data: tp.to_layer_patch() }), value: json!({ "text": tp.to_value() }) })
        }
        GradientPaintOp::PaletteAdd | GradientPaintOp::PaletteRemove => {
            let pal = req.palette.as_ref().ok_or_else(|| ToolError::Invalid("Missing 'palette'".into()))?;
            let Some(NodeKind::Palette(pd)) = p.node(pal).map(|n| &n.data) else { return Err(ToolError::NotFound(format!("paleta {pal}"))) };
            let mut pd = pd.clone();
            let name = req.name.clone().unwrap_or_default();
            if req.op == GradientPaintOp::PaletteAdd {
                let g = g.ok_or_else(|| ToolError::Invalid("palette_add precisa de `gradient` ou `preset`".into()))?;
                let id = ids.next();
                let nm = if name.trim().is_empty() { format!("Degradê {}", pd.gradients.len() + 1) } else { name.trim().to_owned() };
                pd.gradients.push(NamedGradient { id: id.clone(), name: nm, gradient: g });
                Ok(GradientPaintPlan { command: Some(Command::ReplaceData { id: pal.clone(), data: NodeKind::Palette(pd) }), value: json!({ "gradient_id": id }) })
            } else {
                let before = pd.gradients.len();
                pd.gradients.retain(|x| x.name != name && x.id.0 != name);
                if pd.gradients.len() == before {
                    return Err(ToolError::NotFound(format!("degradê '{name}' na paleta")));
                }
                let removed = before - pd.gradients.len();
                Ok(GradientPaintPlan { command: Some(Command::ReplaceData { id: pal.clone(), data: NodeKind::Palette(pd) }), value: json!({ "removed": removed }) })
            }
        }
    }
}
