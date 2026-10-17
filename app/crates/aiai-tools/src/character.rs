//! Personagens (S10): model sheet com grade de proporção e turnaround guiado pelo manequim 3D; medidor de
//! consistência de um desenho contra a ficha.

use crate::context::*;
use aiai_core::*;
use aiai_guide3d::{Camera3d, GuideStyle, Mannequin, Proportions, Scene3d, render_lines};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelSheetRequest {
    pub character: Id,
    #[serde(default = "d_name")]
    pub name: String,
    /// Altura em cabeças.
    pub heads_tall: f32,
    /// Altura da figura em px no sheet.
    #[serde(default = "d_fig")]
    pub figure_px: f32,
    /// Vistas (graus de rotação do corpo; 0 = frente). Padrão: frente, 3/4, perfil, 3/4 costas, costas.
    #[serde(default = "d_views")]
    pub views: Vec<f32>,
    /// Pose do manequim de guia.
    #[serde(default)]
    pub pose: Option<String>,
}

fn d_name() -> String {
    "Model sheet".into()
}
fn d_fig() -> f32 {
    800.0
}
fn d_views() -> Vec<f32> {
    vec![0.0, 45.0, 90.0, 135.0, 180.0]
}

/// Linhas-guia nomeadas (fração da altura a partir do topo da cabeça, em cabeças).
fn landmark_heads(heads: f32) -> Vec<(&'static str, f32)> {
    let chibi = heads < 4.0;
    let leg = if chibi { heads * 0.30 } else { heads * 0.47 };
    vec![
        ("topo", 0.0),
        ("olhos", 0.55),
        ("queixo", 1.0),
        ("ombros", 1.0 + if chibi { 0.12 } else { 0.35 }),
        ("cintura", heads - leg - if chibi { 0.1 } else { 0.4 }),
        ("quadril", heads - leg),
        ("joelhos", heads - leg * 0.5),
        ("pés", heads),
    ]
}

pub struct ModelSheetPlan {
    pub commands: Vec<Command>,
    pub cut: Id,
    pub line_layer: Id,
    pub fill_layer: Id,
}

/// Cria o model sheet: um corte-canvas sob o personagem com camadas de grade, manequim (guia), linha e cor; grava as
/// medidas no personagem.
pub fn model_sheet(p: &Project, idgen: &mut IdGen, req: &ModelSheetRequest) -> ToolResult<ModelSheetPlan> {
    match &node(p, &req.character)?.data {
        NodeKind::Character(_) => {}
        other => return Err(ToolError::Invalid(format!("{} é {}, não personagem", req.character, other.type_name()))),
    }
    if !(1.5..=10.0).contains(&req.heads_tall) || req.views.is_empty() || req.views.len() > 8 {
        return Err(ToolError::Invalid("cabeças 1,5..10 e 1..8 vistas".into()));
    }
    let head_px = req.figure_px / req.heads_tall;
    let slot_w = (req.figure_px * 0.6).max(head_px * 2.6).ceil();
    let margin = (head_px * 0.6).ceil();
    let (w, h) = ((slot_w * req.views.len() as f32).ceil() as u32, (req.figure_px + margin * 2.0).ceil() as u32);
    let top = margin;
    let mut cmds = Vec::new();
    let cut = idgen.next();
    let mut cd = CutData::new(1);
    cd.canvas = Some([w, h]);
    cmds.push(Command::CreateNode { id: Some(cut.clone()), parent: Some(req.character.clone()), index: None, name: req.name.clone(), data: NodeKind::Cut(cd) });
    let mk_layer = |cmds: &mut Vec<Command>, idgen: &mut IdGen, name: &str, kind: LayerType, strokes: Vec<Stroke>| -> Id {
        let l = idgen.next();
        cmds.push(Command::CreateNode { id: Some(l.clone()), parent: Some(cut.clone()), index: None, name: name.into(), data: NodeKind::Layer(LayerData::new(kind)) });
        let mut cel = CelData::new(1);
        cel.strokes = strokes;
        cmds.push(Command::CreateNode { id: Some(idgen.next()), parent: Some(l.clone()), index: None, name: format!("{name}1"), data: NodeKind::Cel(cel) });
        l
    };
    // grade de cabeças + linhas de referência atravessando todas as vistas
    let line = |pts: Vec<[f32; 2]>, width: f32, hex: &str| Stroke {
        id: Id(String::new()),
        points: pts.into_iter().map(|q| StrokePoint { x: q[0], y: q[1], pressure: 1.0, tilt: None, t: None }).collect(),
        geometry: StrokeGeometry::Polyline,
        tension: 0.0,
        tip: VectorTip::Marker,
        width,
        profile: WidthProfile::default(),
        color: ColorRef::Direct { rgba: Rgba::from_hex(hex).expect("cor fixa") },
        opacity: 1.0,
        closed: false,
        aliased: false,
        color_trace: false,
    };
    let mut grid = Vec::new();
    let n_heads = req.heads_tall.ceil() as u32;
    for k in 0..=n_heads {
        let y = top + k as f32 * head_px;
        if y <= top + req.figure_px + 0.5 {
            grid.push(line(vec![[0.0, y], [w as f32, y]], 1.0, "#9ab4d880"));
        }
    }
    let marks = landmark_heads(req.heads_tall);
    for (name, hh) in &marks {
        if matches!(*name, "olhos" | "ombros" | "cintura" | "joelhos") {
            let y = top + hh * head_px;
            grid.push(line(vec![[0.0, y], [w as f32, y]], 1.5, "#e08a4a90"));
        }
    }
    for i in 0..=req.views.len() {
        let x = i as f32 * slot_w;
        grid.push(line(vec![[x, 0.0], [x, h as f32]], 1.0, "#9ab4d860"));
    }
    let grid_l = mk_layer(&mut cmds, idgen, "guia-grade", LayerType::Guide, grid);
    // manequim em cada vista (ortográfica, mesma altura)
    let height_m = 1.6f32;
    let mut mq = Vec::new();
    for (i, ang) in req.views.iter().enumerate() {
        let man = Mannequin {
            proportions: Proportions { heads_tall: req.heads_tall, height: height_m, ..Default::default() },
            position: [0.0, 0.0, 0.0],
            facing: *ang,
            pose: req.pose.clone().or(Some("standing".into())),
            bones: Default::default(),
        };
        let scene = Scene3d { objects: vec![], mannequins: vec![man], floor_grid: None };
        // ortográfica enquadrando a figura: altura visível = altura do canvas em metros
        let m_per_px = height_m / req.figure_px;
        let cam_h = h as f32 * m_per_px;
        let cy = height_m - (top - margin * 0.0) * m_per_px - cam_h / 2.0 + top * m_per_px - top * m_per_px;
        let center_y = height_m + top * m_per_px - cam_h / 2.0;
        let _ = cy;
        let cam = Camera3d { position: [0.0, center_y, 10.0], target: [0.0, center_y, 0.0], focal_mm: 50.0, roll: 0.0, ortho_height: Some(cam_h) };
        let style = GuideStyle { color: "#3f86f0".into(), width: 1.6, crease_angle: 50.0, hidden_lines: false, grid_color: "#000000".into() };
        let strokes = render_lines(&scene, &cam, slot_w as u32, h, &style).map_err(ToolError::Invalid)?;
        for mut s in strokes {
            for pt in &mut s.points {
                pt.x += i as f32 * slot_w;
            }
            mq.push(s);
        }
    }
    let mq_l = mk_layer(&mut cmds, idgen, "guia-manequim", LayerType::Guide, mq);
    let fill_l = mk_layer(&mut cmds, idgen, "cor", LayerType::Fill, vec![]);
    let line_l = mk_layer(&mut cmds, idgen, "linha", LayerType::Vector, vec![]);
    cmds.push(Command::SetProps { id: fill_l.clone(), name: None, data: json!({"line_refs": [line_l.0]}) });
    let measurements = json!({
        "heads_tall": req.heads_tall,
        "head_px": head_px,
        "figure_px": req.figure_px,
        "top_px": top,
        "slot_w": slot_w,
        "views": req.views,
        "sheet_cut": cut.0,
        "landmarks": marks.iter().map(|(n, hh)| json!({"name": n, "heads": hh, "y_px": top + hh * head_px})).collect::<Vec<_>>(),
    });
    cmds.push(Command::SetProps { id: req.character.clone(), name: None, data: json!({"heads_tall": req.heads_tall, "measurements": measurements}) });
    let _ = (grid_l, mq_l);
    Ok(ModelSheetPlan { commands: cmds, cut, line_layer: line_l, fill_layer: fill_l })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConsistencyRequest {
    pub character: Id,
    /// Medidas do desenho (px) informadas por quem desenhou: topo da cabeça, olhos, queixo, ombros, pés (y),
    /// e opcionalmente largura dos ombros (px).
    pub head_top: f32,
    pub chin: f32,
    #[serde(default)]
    pub eyes: Option<f32>,
    #[serde(default)]
    pub shoulders: Option<f32>,
    #[serde(default)]
    pub feet: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConsistencyReport {
    /// Erros relativos (%) por medida, em relação ao model sheet.
    pub errors_pct: std::collections::BTreeMap<String, f32>,
    pub max_error_pct: f32,
    pub ok: bool,
}

/// Compara proporções medidas com o model sheet (tudo em unidades de "cabeça" do próprio desenho).
pub fn consistency(p: &Project, req: &ConsistencyRequest) -> ToolResult<ConsistencyReport> {
    let NodeKind::Character(ch) = &node(p, &req.character)?.data else {
        return Err(ToolError::Invalid("não é personagem".into()));
    };
    let m = &ch.measurements;
    let heads = m.get("heads_tall").and_then(|v| v.as_f64()).ok_or_else(|| ToolError::Invalid("personagem sem model sheet (measurements)".into()))? as f32;
    let head = (req.chin - req.head_top).abs();
    if head < 2.0 {
        return Err(ToolError::Invalid("cabeça muito pequena para medir".into()));
    }
    let lm = landmark_heads(heads);
    // linhas-guia do próprio settei (ex.: olhos mais baixos num desenho de olhos grandes) valem mais que as genéricas
    let own = m.get("landmarks").and_then(|v| v.as_object());
    let expected = |name: &str| {
        own.and_then(|o| o.get(name)).and_then(|v| v.as_f64()).map(|v| v as f32).or_else(|| lm.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)).unwrap_or(0.0)
    };
    let mut errors = std::collections::BTreeMap::new();
    let rel = |got: f32, exp: f32| if exp.abs() < 1e-3 { 0.0 } else { (got - exp).abs() / exp * 100.0 };
    if let Some(e) = req.eyes {
        errors.insert("olhos".into(), rel((e - req.head_top) / head, expected("olhos")));
    }
    if let Some(s) = req.shoulders {
        errors.insert("ombros".into(), rel((s - req.head_top) / head, expected("ombros")));
    }
    if let Some(f) = req.feet {
        errors.insert("altura_total".into(), rel((f - req.head_top) / head, heads));
    }
    let max = errors.values().cloned().fold(0.0f32, f32::max);
    Ok(ConsistencyReport { errors_pct: errors, max_error_pct: max, ok: max <= 5.0 })
}
