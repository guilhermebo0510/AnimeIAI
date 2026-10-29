//! Desenho vetorial de alto nível: o agente descreve curvas por pontos de passagem e larguras; aqui viram traços.

use crate::context::*;
use aiai_core::command::IndexedStroke;
use aiai_core::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Uma curva descrita para o agente: pontos [x, y], [x, y, pressão] ou [x, y, pressão, tempo em s]; largura com perfil
/// opcional; entrada e saída (`taper`) como no Clip Studio.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CurveSpec {
    pub points: Vec<Vec<f32>>,
    #[serde(default = "default_width")]
    pub width: f32,
    #[serde(default)]
    pub tip: VectorTip,
    #[serde(default = "default_geometry")]
    pub geometry: StrokeGeometry,
    #[serde(default)]
    pub tension: f32,
    /// Perfil de largura: [[t, mult], ...]. Atalho: `widths` [início, meio, fim] gera o perfil.
    #[serde(default)]
    pub profile: Vec<[f32; 2]>,
    #[serde(default)]
    pub widths: Option<[f32; 3]>,
    #[serde(default)]
    pub color: ColorRef,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub aliased: bool,
    #[serde(default)]
    pub color_trace: bool,
    /// Grupo de intercalação: o `INBETWEEN` só casa traços do mesmo grupo (ex.: "olho_esq", "cabelo").
    #[serde(default)]
    pub group: String,
    /// Entrada e saída (substitui o afinamento automático da ponta): {start, end, unit: px|percent, min, by_speed}.
    /// `by_speed` usa o tempo dos pontos ([x, y, pressão, t]); sem tempo conta como rápido.
    #[serde(default)]
    pub taper: Option<aiai_core::StrokeTaper>,
}

fn default_width() -> f32 {
    3.0
}
fn default_geometry() -> StrokeGeometry {
    StrokeGeometry::Spline
}
fn one() -> f32 {
    1.0
}

impl CurveSpec {
    pub fn to_stroke(&self) -> ToolResult<Stroke> {
        let points = self
            .points
            .iter()
            .map(|p| match p.as_slice() {
                [x, y] => Ok(StrokePoint { x: *x, y: *y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }),
                [x, y, pr] => Ok(StrokePoint { x: *x, y: *y, pressure: pr.clamp(0.0, 1.0), tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }),
                [x, y, pr, t] => Ok(StrokePoint { x: *x, y: *y, pressure: pr.clamp(0.0, 1.0), tilt: None, t: Some(t.max(0.0)), corner: false, width: 1.0, id: 0, handle: None }),
                _ => Err(ToolError::Invalid("cada ponto é [x, y], [x, y, pressão] ou [x, y, pressão, tempo]".into())),
            })
            .collect::<ToolResult<Vec<_>>>()?;
        let profile = match self.widths {
            Some([a, b, c]) => vec![[0.0, a], [0.5, b], [1.0, c]],
            None => self.profile.clone(),
        };
        let s = Stroke { taper: self.taper,
            id: Id(String::new()),
            points,
            geometry: self.geometry,
            tension: self.tension,
            tip: self.tip,
            width: self.width,
            profile: WidthProfile(profile),
            color: self.color.clone(),
            opacity: self.opacity,
            closed: self.closed,
            aliased: self.aliased,
            color_trace: self.color_trace,
            group: self.group.clone(),
        };
        s.validate().map_err(ToolError::Invalid)?;
        Ok(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DrawRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    pub curves: Vec<CurveSpec>,
    /// Espelhar cada curva num eixo vertical em x = valor (simetria).
    #[serde(default)]
    pub mirror_x: Option<f32>,
}

/// Comando que insere as curvas no fim do cel exposto pela camada no quadro.
pub fn draw_command(p: &Project, req: &DrawRequest) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let layer = layer_data(p, &req.layer)?;
    if layer.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let cel = exposed_cel(p, cut, &req.layer, req.frame)
        .ok_or_else(|| ToolError::Invalid(format!("a camada não expõe nenhum cel no quadro {} (crie o cel e a célula do timesheet)", req.frame)))?;
    let NodeKind::Cel(cd) = &cel.data else { unreachable!() };
    let mut items = Vec::new();
    let mut idx = cd.strokes.len();
    for c in &req.curves {
        let s = c.to_stroke()?;
        if let Some(mx) = req.mirror_x {
            let mut m = s.clone();
            for pt in &mut m.points {
                pt.x = 2.0 * mx - pt.x;
            }
            items.push(IndexedStroke { index: idx, stroke: s });
            idx += 1;
            items.push(IndexedStroke { index: idx, stroke: m });
        } else {
            items.push(IndexedStroke { index: idx, stroke: s });
        }
        idx += 1;
    }
    Ok(Command::InsertStrokes { cel: cel.id.clone(), items })
}
