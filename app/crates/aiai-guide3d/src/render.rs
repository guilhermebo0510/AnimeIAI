//! Render de guia: z-buffer em CPU, arestas de contorno (silhueta), vincos e bordas, com teste de visibilidade por
//! amostragem → polilinhas encadeadas → traços vetoriais. Sombreado Lambert simples para referência de luz.

use crate::camera::{Camera3d, Projector};
use crate::mesh::Mesh;
use crate::scene::Scene3d;
use aiai_core::{ColorRef, Id, Rgba, Stroke, StrokeGeometry, StrokePoint, VectorTip, WidthProfile};
use aiai_paint::Image;
use glam::Vec3;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GuideStyle {
    /// Cor das linhas visíveis.
    #[serde(default = "d_color")]
    pub color: String,
    #[serde(default = "d_width")]
    pub width: f32,
    /// Ângulo (graus) acima do qual uma aresta entre faces vira vinco desenhado.
    #[serde(default = "d_crease")]
    pub crease_angle: f32,
    /// Desenhar também as linhas ocultas (mais transparentes).
    #[serde(default)]
    pub hidden_lines: bool,
    /// Cor da grade de chão.
    #[serde(default = "d_grid_color")]
    pub grid_color: String,
}

fn d_color() -> String {
    "#2f7ff0".into()
}
fn d_width() -> f32 {
    2.0
}
fn d_crease() -> f32 {
    35.0
}
fn d_grid_color() -> String {
    "#8fb8f0".into()
}

impl Default for GuideStyle {
    fn default() -> Self {
        GuideStyle { color: d_color(), width: d_width(), crease_angle: d_crease(), hidden_lines: false, grid_color: d_grid_color() }
    }
}

struct ZBuf {
    w: usize,
    h: usize,
    z: Vec<f32>,
}

impl ZBuf {
    fn raster(meshes: &[Mesh], proj: &Projector, w: u32, h: u32) -> ZBuf {
        let (w, h) = (w as usize, h as usize);
        let mut z = vec![f32::INFINITY; w * h];
        for m in meshes {
            let pv: Vec<Option<(f32, f32, f32)>> = m.verts.iter().map(|v| proj.project(*v)).collect();
            for t in &m.tris {
                let (Some(a), Some(b), Some(c)) = (pv[t[0] as usize], pv[t[1] as usize], pv[t[2] as usize]) else { continue };
                let (x0, x1) = (a.0.min(b.0).min(c.0).floor().max(0.0) as i32, a.0.max(b.0).max(c.0).ceil().min(w as f32 - 1.0) as i32);
                let (y0, y1) = (a.1.min(b.1).min(c.1).floor().max(0.0) as i32, a.1.max(b.1).max(c.1).ceil().min(h as f32 - 1.0) as i32);
                if x0 > x1 || y0 > y1 {
                    continue;
                }
                let area = (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1);
                if area.abs() < 1e-6 {
                    continue;
                }
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                        let w0 = ((b.0 - px) * (c.1 - py) - (c.0 - px) * (b.1 - py)) / area;
                        let w1 = ((c.0 - px) * (a.1 - py) - (a.0 - px) * (c.1 - py)) / area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                            continue;
                        }
                        // profundidade com correção de perspectiva
                        let inv = w0 / a.2 + w1 / b.2 + w2 / c.2;
                        let d = 1.0 / inv;
                        let i = y as usize * w + x as usize;
                        if d < z[i] {
                            z[i] = d;
                        }
                    }
                }
            }
        }
        ZBuf { w, h, z }
    }

    /// Visível se não há superfície claramente à frente (vizinhança 3×3 → tolera bordas).
    fn visible(&self, x: f32, y: f32, depth: f32) -> bool {
        let (xi, yi) = (x as i32, y as i32);
        // o mais DISTANTE da vizinhança 3×3 (uma aresta de silhueta encosta no fundo e continua visível)
        let mut nearest = f32::NEG_INFINITY;
        let mut any = false;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (xx, yy) = (xi + dx, yi + dy);
                if xx < 0 || yy < 0 || xx as usize >= self.w || yy as usize >= self.h {
                    continue;
                }
                any = true;
                nearest = nearest.max(self.z[yy as usize * self.w + xx as usize].min(f32::MAX));
            }
        }
        if !any {
            return false;
        }
        // usa o MAIS distante da vizinhança: uma aresta de silhueta encosta no fundo
        let tol = depth * 0.01 + 0.01;
        depth <= nearest + tol || nearest == f32::MAX
    }
}

type Polyline = Vec<[f32; 2]>;

/// Segmenta a aresta 3D (a, b) projetada em trechos visíveis/ocultos.
fn edge_runs(a: Vec3, b: Vec3, proj: &Projector, zb: &ZBuf, out_vis: &mut Vec<Polyline>, out_hid: &mut Vec<Polyline>) {
    let (Some(pa), Some(pb)) = (proj.project(a), proj.project(b)) else { return };
    let len = ((pb.0 - pa.0).powi(2) + (pb.1 - pa.1).powi(2)).sqrt();
    let n = (len / 1.0).ceil().clamp(1.0, 4000.0) as usize;
    let mut cur: Polyline = Vec::new();
    let mut cur_vis = None;
    for k in 0..=n {
        let t = k as f32 / n as f32;
        // profundidade interpolada em 1/z (perspectiva)
        let inv = (1.0 - t) / pa.2 + t / pb.2;
        let d = 1.0 / inv;
        let (x, y) = (pa.0 + (pb.0 - pa.0) * t, pa.1 + (pb.1 - pa.1) * t);
        let inside = x >= 0.0 && y >= 0.0 && x < zb.w as f32 && y < zb.h as f32;
        let vis = inside && zb.visible(x, y, d);
        if cur_vis != Some(vis) {
            if cur.len() >= 2 {
                if cur_vis == Some(true) { out_vis.push(std::mem::take(&mut cur)) } else { out_hid.push(std::mem::take(&mut cur)) }
            }
            cur.clear();
            if let Some(last) = cur_vis.and(None::<[f32; 2]>) {
                cur.push(last);
            }
            cur_vis = Some(vis);
        }
        if inside {
            cur.push([x, y]);
        }
    }
    if cur.len() >= 2 {
        if cur_vis == Some(true) { out_vis.push(cur) } else { out_hid.push(cur) }
    }
}

/// Encadeia polilinhas cujas pontas coincidem (menos traços, linhas contínuas).
fn chain(mut lines: Vec<Polyline>) -> Vec<Polyline> {
    let key = |p: [f32; 2]| ((p[0] * 2.0).round() as i64, (p[1] * 2.0).round() as i64);
    let mut out: Vec<Polyline> = Vec::new();
    while let Some(mut cur) = lines.pop() {
        loop {
            let end = key(*cur.last().expect("≥2"));
            let start = key(cur[0]);
            let found = lines.iter().position(|l| key(l[0]) == end || key(*l.last().expect("≥2")) == end || key(*l.last().expect("≥2")) == start || key(l[0]) == start);
            let Some(i) = found else { break };
            let mut l = lines.swap_remove(i);
            if key(l[0]) == end {
                cur.extend(l.drain(1..));
            } else if key(*l.last().expect("≥2")) == end {
                l.reverse();
                cur.extend(l.drain(1..));
            } else if key(*l.last().expect("≥2")) == start {
                l.extend(cur.drain(1..));
                cur = l;
            } else {
                l.reverse();
                l.extend(cur.drain(1..));
                cur = l;
            }
        }
        out.push(cur);
    }
    out
}

/// Simplifica (Ramer–Douglas–Peucker) para não gerar milhares de pontos.
fn simplify(p: &[[f32; 2]], eps: f32) -> Polyline {
    if p.len() < 3 {
        return p.to_vec();
    }
    let (a, b) = (p[0], p[p.len() - 1]);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let (mut idx, mut dmax) = (0, 0.0f32);
    for (i, q) in p.iter().enumerate().take(p.len() - 1).skip(1) {
        // distância ao SEGMENTO (não à reta): polilinhas fechadas ou de ida e volta têm início ≈ fim
        let t = if l2 > 1e-9 { (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let (px, py) = (a[0] + dx * t, a[1] + dy * t);
        let d = ((q[0] - px).powi(2) + (q[1] - py).powi(2)).sqrt();
        if d > dmax {
            dmax = d;
            idx = i;
        }
    }
    if dmax > eps {
        let mut left = simplify(&p[..=idx], eps);
        let right = simplify(&p[idx..], eps);
        left.pop();
        left.extend(right);
        left
    } else {
        vec![a, b]
    }
}

fn to_stroke(p: &[[f32; 2]], width: f32, color: Rgba) -> Stroke {
    Stroke {
        id: Id(String::new()),
        points: p.iter().map(|q| StrokePoint { x: q[0], y: q[1], pressure: 1.0, tilt: None, t: None }).collect(),
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
    }
}

/// Linhas de guia da cena vistas pela câmera, como traços vetoriais no canvas `w × h`.
pub fn render_lines(scene: &Scene3d, camera: &Camera3d, w: u32, h: u32, style: &GuideStyle) -> Result<Vec<Stroke>, String> {
    scene.validate()?;
    camera.validate()?;
    let proj = camera.projector(w, h);
    let meshes = scene.meshes();
    let zb = ZBuf::raster(&meshes, &proj, w, h);
    let eye = Vec3::from(camera.position);
    let crease_cos = style.crease_angle.to_radians().cos();
    let (mut vis, mut hid) = (Vec::new(), Vec::new());
    for m in &meshes {
        let normals: Vec<Vec3> = (0..m.tris.len()).map(|t| m.face_normal(t)).collect();
        let mut edges: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (ti, t) in m.tris.iter().enumerate() {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                edges.entry((a.min(b), a.max(b))).or_default().push(ti);
            }
        }
        let mut keys: Vec<&(u32, u32)> = edges.keys().collect();
        keys.sort_unstable(); // determinismo
        for k in keys {
            let faces = &edges[k];
            let (va, vb) = (m.verts[k.0 as usize], m.verts[k.1 as usize]);
            let mid = (va + vb) * 0.5;
            let to_eye = if camera.ortho_height.is_some() { eye - Vec3::from(camera.target) } else { eye - mid };
            let front = |f: usize| normals[f].dot(to_eye) > 0.0;
            let draw = match faces.as_slice() {
                [f] => normals[*f] != Vec3::ZERO && front(*f),
                [f1, f2, ..] => {
                    let (n1, n2) = (normals[*f1], normals[*f2]);
                    if n1 == Vec3::ZERO || n2 == Vec3::ZERO {
                        false
                    } else {
                        let sil = front(*f1) != front(*f2);
                        let crease = n1.dot(n2) < crease_cos && (front(*f1) || front(*f2));
                        sil || crease
                    }
                }
                _ => false,
            };
            if draw {
                edge_runs(va, vb, &proj, &zb, &mut vis, &mut hid);
            }
        }
    }
    let color = Rgba::from_hex(&style.color).ok_or("cor inválida")?;
    let mut strokes: Vec<Stroke> = chain(vis).iter().map(|l| simplify(l, 0.35)).filter(|l| l.len() >= 2).map(|l| to_stroke(&l, style.width, color)).collect();
    if style.hidden_lines {
        let mut c = color;
        c.0[3] *= 0.3;
        strokes.extend(chain(hid).iter().map(|l| simplify(l, 0.35)).filter(|l| l.len() >= 2).map(|l| to_stroke(&l, style.width * 0.6, c)));
    }
    if let Some(g) = &scene.floor_grid {
        let gc = Rgba::from_hex(&style.grid_color).ok_or("cor da grade inválida")?;
        let half = g.size / 2.0;
        let n = (g.size / g.step).round() as i32;
        let mut lines = Vec::new();
        for i in 0..=n {
            let v = -half + i as f32 * g.step;
            for (a, b) in [(Vec3::new(v, 0.0, -half), Vec3::new(v, 0.0, half)), (Vec3::new(-half, 0.0, v), Vec3::new(half, 0.0, v))] {
                // linhas do chão: recorta onde algo está na frente
                let (mut gv, mut gh) = (Vec::new(), Vec::new());
                // subdivide para projetar perto da câmera sem atravessar o plano da imagem
                let steps = 32;
                for k in 0..steps {
                    let (p0, p1) = (a + (b - a) * (k as f32 / steps as f32), a + (b - a) * ((k + 1) as f32 / steps as f32));
                    edge_runs(p0, p1, &proj, &zb, &mut gv, &mut gh);
                }
                lines.extend(gv);
            }
        }
        strokes.extend(chain(lines).iter().map(|l| simplify(l, 0.35)).filter(|l| l.len() >= 2).map(|l| to_stroke(&l, (style.width * 0.6).max(0.6), gc)));
    }
    Ok(strokes)
}

/// Sombreado de referência (Lambert com luz direcional + ambiente), cinza, fundo transparente.
pub fn render_shaded(scene: &Scene3d, camera: &Camera3d, w: u32, h: u32, light_dir: [f32; 3]) -> Result<Image, String> {
    scene.validate()?;
    camera.validate()?;
    let proj = camera.projector(w, h);
    let meshes = scene.meshes();
    let l = Vec3::from(light_dir).normalize_or_zero();
    let (wu, hu) = (w as usize, h as usize);
    let mut z = vec![f32::INFINITY; wu * hu];
    let mut img = Image::new(w, h);
    for m in &meshes {
        let pv: Vec<Option<(f32, f32, f32)>> = m.verts.iter().map(|v| proj.project(*v)).collect();
        for (ti, t) in m.tris.iter().enumerate() {
            let (Some(a), Some(b), Some(c)) = (pv[t[0] as usize], pv[t[1] as usize], pv[t[2] as usize]) else { continue };
            let n = m.face_normal(ti);
            let shade = (0.25 + 0.75 * n.dot(-l).max(0.0)).clamp(0.0, 1.0);
            let area = (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1);
            if area.abs() < 1e-6 {
                continue;
            }
            let (x0, x1) = (a.0.min(b.0).min(c.0).floor().max(0.0) as i32, a.0.max(b.0).max(c.0).ceil().min(w as f32 - 1.0) as i32);
            let (y0, y1) = (a.1.min(b.1).min(c.1).floor().max(0.0) as i32, a.1.max(b.1).max(c.1).ceil().min(h as f32 - 1.0) as i32);
            for y in y0..=y1.max(y0 - 1) {
                for x in x0..=x1.max(x0 - 1) {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b.0 - px) * (c.1 - py) - (c.0 - px) * (b.1 - py)) / area;
                    let w1 = ((c.0 - px) * (a.1 - py) - (a.0 - px) * (c.1 - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let d = 1.0 / (w0 / a.2 + w1 / b.2 + w2 / c.2);
                    let i = y as usize * wu + x as usize;
                    if d < z[i] {
                        z[i] = d;
                        img.data[i] = [shade, shade, shade, 1.0];
                    }
                }
            }
        }
    }
    Ok(img)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VanishingPoints {
    /// Pontos de fuga dos eixos X, Y e Z do mundo (None = paralelo ao plano da imagem).
    pub x: Option<[f32; 2]>,
    pub y: Option<[f32; 2]>,
    pub z: Option<[f32; 2]>,
    /// Altura (px) da linha do horizonte (fuga das direções horizontais), se definida.
    pub horizon_y: Option<f32>,
}

pub fn vanishing_points(camera: &Camera3d, w: u32, h: u32) -> VanishingPoints {
    let proj = camera.projector(w, h);
    let x = proj.vanishing_point(Vec3::X);
    let y = proj.vanishing_point(Vec3::Y);
    let z = proj.vanishing_point(Vec3::Z);
    // horizonte: fuga de duas direções horizontais quaisquer
    let a = proj.vanishing_point(Vec3::new(1.0, 0.0, 1.0));
    let b = proj.vanishing_point(Vec3::new(-1.0, 0.0, 1.0));
    let horizon_y = match (a, b) {
        (Some(p), Some(q)) => Some((p[1] + q[1]) / 2.0),
        (Some(p), None) | (None, Some(p)) => Some(p[1]),
        _ => None,
    };
    VanishingPoints { x, y, z, horizon_y }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mannequin::Mannequin;
    use crate::scene::{Object3d, Shape};

    #[test]
    fn box_outline_and_vanishing_points() {
        let scene = Scene3d { objects: vec![Object3d { name: "mesa".into(), shape: Shape::Box { size: [1.0, 0.8, 0.6] }, position: [0.0, 0.4, 0.0], rotation: [0.0, 30.0, 0.0], scale: [1.0; 3] }], mannequins: vec![], floor_grid: None };
        let cam = Camera3d { position: [2.5, 1.6, 3.0], target: [0.0, 0.5, 0.0], focal_mm: 35.0, roll: 0.0, ortho_height: None };
        let strokes = render_lines(&scene, &cam, 640, 360, &GuideStyle::default()).unwrap();
        assert!(!strokes.is_empty());
        let pts: usize = strokes.iter().map(|s| s.points.len()).sum();
        assert!(pts < 400, "linhas simplificadas: {pts} pontos");
        // 9 arestas visíveis de uma caixa vista de cima e de lado (3 faces) → encadeadas em poucos traços
        assert!(strokes.len() <= 9, "{} traços", strokes.len());
        let vp = vanishing_points(&cam, 640, 360);
        let hy = vp.horizon_y.unwrap();
        // olho a 1,6 m olhando para baixo → horizonte acima do centro
        assert!(hy < 180.0, "horizonte acima do meio: {hy}");
    }

    #[test]
    fn mannequin_lines_and_shading() {
        let scene = Scene3d { objects: vec![], mannequins: vec![Mannequin::default()], floor_grid: Some(crate::scene::FloorGrid { size: 4.0, step: 0.5 }) };
        let cam = Camera3d { position: [0.0, 1.2, 3.5], target: [0.0, 0.9, 0.0], focal_mm: 50.0, roll: 0.0, ortho_height: None };
        let strokes = render_lines(&scene, &cam, 480, 640, &GuideStyle::default()).unwrap();
        assert!(strokes.len() > 10);
        let img = render_shaded(&scene, &cam, 480, 640, [-0.4, -1.0, -0.6]).unwrap();
        let covered = img.data.iter().filter(|p| p[3] > 0.0).count();
        assert!(covered > 5_000, "manequim ocupa área: {covered}");
    }
}


#[cfg(test)]
mod hidden_tests {
    use super::*;
    use crate::scene::{Object3d, Shape};

    #[test]
    fn box_behind_box_is_hidden() {
        // caixa pequena atrás de uma grande, de frente: as arestas da pequena ficam ocultas
        let scene = Scene3d {
            objects: vec![
                Object3d { name: "frente".into(), shape: Shape::Box { size: [2.0, 2.0, 0.2] }, position: [0.0, 1.0, 1.0], rotation: [0.0; 3], scale: [1.0; 3] },
                Object3d { name: "atras".into(), shape: Shape::Box { size: [0.5, 0.5, 0.5] }, position: [0.0, 1.0, -1.0], rotation: [0.0; 3], scale: [1.0; 3] },
            ],
            mannequins: vec![],
            floor_grid: None,
        };
        let cam = Camera3d { position: [0.0, 1.0, 8.0], target: [0.0, 1.0, 0.0], focal_mm: 50.0, roll: 0.0, ortho_height: None };
        let vis = render_lines(&scene, &cam, 400, 400, &GuideStyle::default()).unwrap();
        let with_hidden = render_lines(&scene, &cam, 400, 400, &GuideStyle { hidden_lines: true, ..Default::default() }).unwrap();
        assert!(with_hidden.len() > vis.len(), "há linhas ocultas: {} vs {}", with_hidden.len(), vis.len());
        // nenhuma linha visível no miolo da caixa da frente (onde só a de trás teria arestas)
        let inner = vis.iter().flat_map(|s| s.points.iter()).filter(|p| (p.x - 200.0).abs() < 25.0 && (p.y - 200.0).abs() < 25.0).count();
        assert_eq!(inner, 0, "arestas da caixa de trás não aparecem");
    }
}
