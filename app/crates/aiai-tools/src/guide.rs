//! Guias de layout: cena 3D (blocos + manequim) renderizada como linhas numa camada de guia, sombreado de referência,
//! e réguas de perspectiva (linhas de fuga + horizonte) geradas a partir da câmera.

use crate::context::*;
use crate::paint::tiles_for_region;
use aiai_core::*;
use aiai_guide3d::{Camera3d, GuideStyle, Scene3d, render_lines, render_shaded, vanishing_points};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Guide3dRequest {
    pub cut: Id,
    pub frame: u32,
    /// Camada (guia ou vetorial) que recebe as linhas no cel exposto; o conteúdo anterior do cel é substituído.
    pub layer: Id,
    pub scene: Scene3d,
    pub camera: Camera3d,
    #[serde(default)]
    pub style: GuideStyle,
    /// Camada raster opcional para o sombreado de referência.
    #[serde(default)]
    pub shaded_layer: Option<Id>,
    #[serde(default = "d_light")]
    pub light_dir: [f32; 3],
}

fn d_light() -> [f32; 3] {
    [-0.4, -1.0, -0.5]
}

fn exposed<'a>(p: &'a Project, cut: &Id, layer: &Id, frame: u32) -> ToolResult<(&'a Node, &'a CelData, u32, u32)> {
    let c = cut_data(p, cut)?;
    let node = exposed_cel(p, c, layer, frame).ok_or_else(|| ToolError::Invalid(format!("a camada {layer} não expõe cel no quadro {frame}")))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let (w, h) = canvas_size(p, c);
    Ok((node, cel, w, h))
}

/// Lote: linhas da cena no cel da camada de guia (+ sombreado), e a cena/câmera guardadas em `params.guide3d` da
/// camada para edição posterior.
pub fn guide3d_command(p: &Project, blobs: &mut BlobStore, req: &Guide3dRequest) -> ToolResult<Command> {
    let (node, cel, w, h) = exposed(p, &req.cut, &req.layer, req.frame)?;
    let strokes = render_lines(&req.scene, &req.camera, w, h, &req.style).map_err(ToolError::Invalid)?;
    let mut new_cel = cel.clone();
    new_cel.strokes = strokes;
    let mut cmds = vec![
        Command::ReplaceData { id: node.id.clone(), data: NodeKind::Cel(new_cel) },
        Command::SetProps {
            id: req.layer.clone(),
            name: None,
            data: json!({"params": {"guide3d": {"scene": req.scene, "camera": req.camera, "style": req.style}}}),
        },
    ];
    if let Some(sl) = &req.shaded_layer {
        let (snode, _, _, _) = exposed(p, &req.cut, sl, req.frame)?;
        let img = render_shaded(&req.scene, &req.camera, w, h, req.light_dir).map_err(ToolError::Invalid)?;
        cmds.push(Command::SetTiles { cel: snode.id.clone(), tiles: tiles_for_region(blobs, &img, [0, 0, w as i32, h as i32]) });
    }
    Ok(Command::Batch { commands: cmds, label: Some("Guia 3D".into()) })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PerspectiveRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    /// Câmera de onde saem os pontos de fuga (mesma da guia 3D), ou pontos explícitos.
    #[serde(default)]
    pub camera: Option<Camera3d>,
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    #[serde(default)]
    pub horizon_y: Option<f32>,
    /// Linhas por ponto de fuga.
    #[serde(default = "d_lines")]
    pub lines_per_point: u32,
    #[serde(default = "d_pcolor")]
    pub color: String,
}

fn d_lines() -> u32 {
    24
}
fn d_pcolor() -> String {
    "#f07a2f80".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PerspectiveInfo {
    pub points: Vec<[f32; 2]>,
    pub horizon_y: Option<f32>,
}

pub fn perspective_info(p: &Project, req: &PerspectiveRequest) -> ToolResult<PerspectiveInfo> {
    let c = cut_data(p, &req.cut)?;
    let (w, h) = canvas_size(p, c);
    if let Some(cam) = &req.camera {
        let vp = vanishing_points(cam, w, h);
        let pts: Vec<[f32; 2]> = [vp.x, vp.z, vp.y].into_iter().flatten().filter(|q| q[0].abs() < w as f32 * 20.0 && q[1].abs() < h as f32 * 20.0).collect();
        Ok(PerspectiveInfo { points: pts, horizon_y: vp.horizon_y })
    } else {
        Ok(PerspectiveInfo { points: req.points.clone(), horizon_y: req.horizon_y })
    }
}

/// Régua de perspectiva desenhada como linhas de guia (raios a partir de cada ponto de fuga + horizonte).
pub fn perspective_command(p: &Project, req: &PerspectiveRequest) -> ToolResult<Command> {
    let (node, cel, w, h) = exposed(p, &req.cut, &req.layer, req.frame)?;
    let info = perspective_info(p, req)?;
    if info.points.is_empty() {
        return Err(ToolError::Invalid("nenhum ponto de fuga (câmera ortográfica ou paralela?)".into()));
    }
    let color = Rgba::from_hex(&req.color).ok_or_else(|| ToolError::Invalid("cor inválida".into()))?;
    let mk = |pts: Vec<[f32; 2]>, width: f32| Stroke {
        id: Id(String::new()),
        points: pts.into_iter().map(|q| StrokePoint { x: q[0], y: q[1], pressure: 1.0, tilt: None, t: None }).collect(),
        geometry: StrokeGeometry::Polyline,
        tension: 0.0,
        tip: VectorTip::Marker,
        width,
        profile: WidthProfile::default(),
        color: ColorRef::Direct { rgba: color },
        opacity: 1.0,
        closed: false,
        aliased: false,
        color_trace: false,
    };
    let mut strokes = Vec::new();
    let reach = (w.max(h) as f32) * 3.0;
    for vp in &info.points {
        // raios distribuídos pelo ângulo sob o qual o canvas é visto a partir do ponto
        let corners = [[0.0, 0.0], [w as f32, 0.0], [w as f32, h as f32], [0.0, h as f32]];
        let angs: Vec<f32> = corners.iter().map(|c| (c[1] - vp[1]).atan2(c[0] - vp[0])).collect();
        let inside = vp[0] >= 0.0 && vp[1] >= 0.0 && vp[0] <= w as f32 && vp[1] <= h as f32;
        let (a0, a1) = if inside {
            (0.0, std::f32::consts::TAU)
        } else {
            // menor arco que contém os 4 cantos
            let mut best = (0.0f32, std::f32::consts::TAU);
            for &s in &angs {
                let span = angs.iter().map(|&a| (a - s).rem_euclid(std::f32::consts::TAU)).fold(0.0f32, f32::max);
                if span < best.1 - best.0 {
                    best = (s, s + span);
                }
            }
            best
        };
        let n = req.lines_per_point.clamp(2, 200);
        for k in 0..n {
            let a = a0 + (a1 - a0) * (k as f32 + 0.5) / n as f32;
            strokes.push(mk(vec![*vp, [vp[0] + a.cos() * reach, vp[1] + a.sin() * reach]], 1.2));
        }
    }
    if let Some(hy) = info.horizon_y {
        strokes.push(mk(vec![[-(w as f32), hy], [2.0 * w as f32, hy]], 2.0));
    }
    let mut new_cel = cel.clone();
    new_cel.strokes = strokes;
    Ok(Command::Batch {
        commands: vec![
            Command::ReplaceData { id: node.id.clone(), data: NodeKind::Cel(new_cel) },
            Command::SetProps { id: req.layer.clone(), name: None, data: json!({"params": {"ruler": {"points": info.points, "horizon_y": info.horizon_y}}}) },
        ],
        label: Some("Régua de perspectiva".into()),
    })
}
