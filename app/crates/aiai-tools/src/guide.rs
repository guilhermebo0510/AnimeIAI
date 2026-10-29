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

/// Juntas e linhas do corpo de cada manequim projetadas no canvas (auditoria 1.5b): para desenhar por cima da guia
/// sabendo onde fica cada articulação. Cada ponto = [x, y, profundidade em m] (ausente = atrás da câmera).
/// `lines`: eixo do corpo (pelve → topo da cabeça), linha dos ombros, linha do quadril e `front` (a frente do tronco,
/// do centro do peito 0,3 m para a frente: diz para onde o corpo está virado no desenho).
pub fn guide3d_projection(p: &Project, req: &Guide3dRequest) -> ToolResult<serde_json::Value> {
    let c = cut_data(p, &req.cut)?;
    let (w, h) = canvas_size(p, c);
    let pr = req.camera.projector(w, h);
    let r1 = |v: f32| (v * 10.0).round() / 10.0;
    let pt = |v: glam::Vec3| pr.project(v).map(|(x, y, d)| json!([r1(x), r1(y), (d * 1000.0).round() / 1000.0]));
    let mut mans = Vec::new();
    for (i, m) in req.scene.mannequins.iter().enumerate() {
        let j = m.joints();
        let mut joints = serde_json::Map::new();
        for (name, (_, p0, p1)) in &j {
            joints.insert((*name).to_owned(), json!({ "start": pt(*p0), "end": pt(*p1) }));
        }
        let start = |n: &str| j.get(n).map(|(_, a, _)| *a);
        let end = |n: &str| j.get(n).map(|(_, _, b)| *b);
        let seg = |a: Option<glam::Vec3>, b: Option<glam::Vec3>| match (a, b) {
            (Some(a), Some(b)) => json!([pt(a), pt(b)]),
            _ => serde_json::Value::Null,
        };
        // frente do tronco: a direção para onde o peito aponta (eixo Z local do osso do peito)
        let front = j.get("chest").map(|(mtx, a, b)| {
            let mid = (*a + *b) * 0.5;
            let fwd = mtx.transform_vector3(glam::Vec3::Z).normalize_or_zero() * 0.3;
            json!([pt(mid), pt(mid + fwd)])
        });
        mans.push(json!({
            "index": i,
            "joints": joints,
            "lines": {
                "midline": seg(start("pelvis"), end("head")),
                "shoulders": seg(start("upper_arm_l"), start("upper_arm_r")),
                "hips": seg(start("thigh_l"), start("thigh_r")),
                "front": front,
            },
        }));
    }
    Ok(json!({ "canvas": [w, h], "mannequins": mans }))
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

/// Traço de guia reto (polilinha de marcador, pressão 1): o das linhas de fuga e o das réguas convertidas em traços.
pub fn guide_stroke(pts: Vec<[f32; 2]>, width: f32, color: Rgba) -> Stroke {
    Stroke { taper: None,
        id: Id(String::new()),
        points: pts.into_iter().map(|q| StrokePoint { x: q[0], y: q[1], pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
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
        group: String::new(),
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
    let mk = |pts: Vec<[f32; 2]>, width: f32| guide_stroke(pts, width, color);
    let mut strokes = Vec::new();
    let reach = (w.max(h) as f32) * 3.0;
    for vp in &info.points {
        // raios distribuídos pelo ângulo sob o qual o canvas é visto a partir do ponto (a mesma conta das réguas)
        let (a0, a1) = crate::ruler::ray_arc(*vp, [0.0, 0.0, w as f32, h as f32]);
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
