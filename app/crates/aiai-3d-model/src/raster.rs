//! Rasterizador CPU de reserva da camada 3D: o mesmo quadro que o serviço GPU desenha (câmera, transformação, modos
//! toon/flat/wire/silhouette, contorno), sem GPU — para o modo headless/MCP sem adaptador, para os testes e para o modo
//! `wire` (o renderer cel não desenha arestas). Determinístico.
//!
//! Saída: RGBA sRGB **pré-multiplicado** 0..1 (o formato do compositor), na resolução pedida, com superamostragem 2×2
//! até ~2 MP (anti-serrilhado do contorno e das bordas).

use crate::camera::{model_matrix, orbit_from_layer};
use crate::model::Model;
use aiai_core::model3d::{Model3dState, RenderMode3d};
use glam::{Mat3, Mat4, Vec3, Vec4};

/// Cor da silhueta quando a camada não define `color` (sRGB).
pub const SILHOUETTE_DEFAULT: [f32; 3] = [0.29, 0.31, 0.38];
/// Multiplicador do tom de sombra do toon (sRGB), levemente azulado como a sombra de anime.
pub const SHADE_MUL: [f32; 3] = [0.62, 0.62, 0.74];
/// Limiar de luz do toon (N·L).
pub const TOON_THRESHOLD: f32 = 0.12;

pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

pub fn srgb_to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Centro e raio do modelo já no mundo (caixa transformada).
pub fn world_bounds(model: &Model, s: &Model3dState) -> (Vec3, f32) {
    let m = model_matrix(&s.transform);
    let c = m.transform_point3(Vec3::from(model.center()));
    let k = s.transform.scale.iter().fold(0.0f32, |a, b| a.max(b.abs()));
    (c, model.radius() * k.max(1e-4))
}

/// Matrizes do quadro: view-proj da câmera da camada (proporção do canvas), matriz do modelo, olho, direção da luz
/// (presa à câmera, de cima e da esquerda, para a referência ficar legível de qualquer ângulo) e planos perto/longe.
pub struct FrameSetup {
    pub view_proj: Mat4,
    pub model: Mat4,
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
    pub light_dir: Vec3,
    pub near: f32,
    pub far: f32,
}

pub fn frame_setup(model: &Model, s: &Model3dState, w: u32, h: u32) -> FrameSetup {
    let (c, r) = world_bounds(model, s);
    let cam = orbit_from_layer(&s.camera, c, r);
    let aspect = w as f32 / h.max(1) as f32;
    let back = -cam.forward();
    let light_dir = (-0.45 * cam.right() + 0.75 * cam.up() + 0.5 * back).normalize();
    FrameSetup {
        view_proj: cam.view_proj(aspect),
        model: model_matrix(&s.transform),
        eye: cam.eye(),
        target: cam.target,
        up: cam.view_up(),
        fov_y: cam.fov_y,
        light_dir,
        near: cam.near,
        far: cam.far,
    }
}

#[derive(Clone, Copy)]
struct CVert {
    clip: Vec4,
    n: Vec3,
}

fn lerp_v(a: &CVert, b: &CVert, t: f32) -> CVert {
    CVert { clip: a.clip + (b.clip - a.clip) * t, n: a.n + (b.n - a.n) * t }
}

/// Recorta o triângulo contra o plano perto (z_clip ≥ 0 na projeção [0, 1] do glam).
fn clip_near(tri: [CVert; 3]) -> Vec<CVert> {
    let inside = |v: &CVert| v.clip.z >= 0.0;
    if tri.iter().all(inside) {
        return tri.to_vec();
    }
    let mut out = Vec::with_capacity(4);
    for i in 0..3 {
        let (a, b) = (&tri[i], &tri[(i + 1) % 3]);
        let (ia, ib) = (inside(a), inside(b));
        if ia {
            out.push(*a);
        }
        if ia != ib {
            let t = a.clip.z / (a.clip.z - b.clip.z);
            out.push(lerp_v(a, b, t));
        }
    }
    out
}

struct Target {
    w: usize,
    h: usize,
    depth: Vec<f32>,
    id: Vec<u32>,
    color: Vec<[f32; 3]>,
}

impl Target {
    fn new(w: usize, h: usize) -> Self {
        Target { w, h, depth: vec![f32::INFINITY; w * h], id: vec![0; w * h], color: vec![[0.0; 3]; w * h] }
    }
}

/// Desenha o modelo. `w`×`h` = tamanho do canvas do corte.
pub fn render_cpu(model: &Model, s: &Model3dState, w: u32, h: u32) -> Vec<[f32; 4]> {
    let (w, h) = (w.max(1), h.max(1));
    let ss: usize = if (w as u64) * (h as u64) <= 2_100_000 { 2 } else { 1 };
    let (sw, sh) = (w as usize * ss, h as usize * ss);
    let fs = frame_setup(model, s, w, h);
    let mvp = fs.view_proj * fs.model;
    let nm = Mat3::from_mat4(fs.model).inverse().transpose();
    let mut t = Target::new(sw, sh);
    let to_screen = |c: Vec4| -> Vec3 {
        let inv = 1.0 / c.w;
        Vec3::new((c.x * inv * 0.5 + 0.5) * sw as f32, (1.0 - (c.y * inv * 0.5 + 0.5)) * sh as f32, c.z * inv)
    };
    let fill = s.mode != RenderMode3d::Wire;
    // vértices transformados por parte (o modo wire reusa para as arestas)
    let mut screen_parts: Vec<Vec<CVert>> = Vec::with_capacity(model.parts.len());
    for (pi, p) in model.parts.iter().enumerate() {
        let verts: Vec<CVert> = p
            .positions
            .iter()
            .zip(&p.normals)
            .map(|(q, n)| CVert { clip: mvp * Vec3::from(*q).extend(1.0), n: (nm * Vec3::from(*n)).normalize_or(Vec3::Y) })
            .collect();
        let base = match (s.mode, s.color) {
            (RenderMode3d::Silhouette, c) => c.unwrap_or(SILHOUETTE_DEFAULT),
            (_, Some(c)) => c,
            _ => [linear_to_srgb(p.color[0]), linear_to_srgb(p.color[1]), linear_to_srgb(p.color[2])],
        };
        let id = pi as u32 + 1;
        for tri in p.indices.chunks_exact(3) {
            let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            if a >= verts.len() || b >= verts.len() || c >= verts.len() {
                continue;
            }
            let poly = clip_near([verts[a], verts[b], verts[c]]);
            for k in 1..poly.len().saturating_sub(1) {
                raster_tri(&mut t, [poly[0], poly[k], poly[k + 1]], &to_screen, id, base, s.mode, fs.light_dir, fs.eye, fill);
            }
        }
        screen_parts.push(verts);
    }
    let mut out_ss: Vec<[f32; 4]> = (0..sw * sh).map(|i| if fill && t.id[i] != 0 { let c = t.color[i]; [c[0], c[1], c[2], 1.0] } else { [0.0; 4] }).collect();
    let line_px = (s.outline_px * ss as f32).max(if s.mode == RenderMode3d::Wire { ss as f32 } else { 0.0 });
    let oc = s.outline_color;
    if s.mode == RenderMode3d::Wire {
        // arestas visíveis: profundidade comparada com a do passe acima (só profundidade, sem cor)
        for (pi, p) in model.parts.iter().enumerate() {
            let verts = &screen_parts[pi];
            for tri in p.indices.chunks_exact(3) {
                for (i0, i1) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                    let (Some(a), Some(b)) = (verts.get(i0 as usize), verts.get(i1 as usize)) else { continue };
                    draw_edge(&t, &mut out_ss, a, b, &to_screen, line_px, oc, fs.near, fs.far);
                }
            }
        }
    } else if line_px > 0.0 {
        outline(&t, &mut out_ss, line_px, oc, fs.near, fs.far);
    }
    // reduz a superamostragem (média pré-multiplicada)
    if ss == 1 {
        return out_ss;
    }
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![[0.0f32; 4]; w * h];
    let k = 1.0 / (ss * ss) as f32;
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for dy in 0..ss {
                let row = (y * ss + dy) * sw + x * ss;
                for dx in 0..ss {
                    let p = out_ss[row + dx];
                    for c in 0..4 {
                        acc[c] += p[c];
                    }
                }
            }
            out[y * w + x] = acc.map(|v| v * k);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn raster_tri(t: &mut Target, v: [CVert; 3], to_screen: &dyn Fn(Vec4) -> Vec3, id: u32, base: [f32; 3], mode: RenderMode3d, light: Vec3, eye: Vec3, fill: bool) {
    let p = [to_screen(v[0].clip), to_screen(v[1].clip), to_screen(v[2].clip)];
    let area = (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[1].y - p[0].y) * (p[2].x - p[0].x);
    if area.abs() < 1e-9 || !area.is_finite() {
        return;
    }
    let minx = p.iter().map(|q| q.x).fold(f32::INFINITY, f32::min).floor().max(0.0) as i64;
    let maxx = p.iter().map(|q| q.x).fold(f32::NEG_INFINITY, f32::max).ceil().min(t.w as f32 - 1.0) as i64;
    let miny = p.iter().map(|q| q.y).fold(f32::INFINITY, f32::min).floor().max(0.0) as i64;
    let maxy = p.iter().map(|q| q.y).fold(f32::NEG_INFINITY, f32::max).ceil().min(t.h as f32 - 1.0) as i64;
    if minx > maxx || miny > maxy {
        return;
    }
    let invw = [1.0 / v[0].clip.w, 1.0 / v[1].clip.w, 1.0 / v[2].clip.w];
    // normal da face no mundo (para o lado que olha para a câmera)
    let shade = |n: Vec3| -> [f32; 3] {
        match mode {
            RenderMode3d::Toon => {
                if n.dot(light) > TOON_THRESHOLD { base } else { [base[0] * SHADE_MUL[0], base[1] * SHADE_MUL[1], base[2] * SHADE_MUL[2]] }
            }
            _ => base,
        }
    };
    let _ = eye;
    let inv_area = 1.0 / area;
    for y in miny..=maxy {
        let py = y as f32 + 0.5;
        for x in minx..=maxx {
            let px = x as f32 + 0.5;
            let w0 = ((p[2].x - p[1].x) * (py - p[1].y) - (p[2].y - p[1].y) * (px - p[1].x)) * inv_area;
            let w1 = ((p[0].x - p[2].x) * (py - p[2].y) - (p[0].y - p[2].y) * (px - p[2].x)) * inv_area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let z = w0 * p[0].z + w1 * p[1].z + w2 * p[2].z;
            if !(0.0..=1.0).contains(&z) {
                continue;
            }
            let i = y as usize * t.w + x as usize;
            if z >= t.depth[i] {
                continue;
            }
            t.depth[i] = z;
            t.id[i] = id;
            if fill {
                // normal com correção de perspectiva, virada para a câmera (faces de trás também são desenhadas)
                let (a, b, c) = (w0 * invw[0], w1 * invw[1], w2 * invw[2]);
                let mut n = (v[0].n * a + v[1].n * b + v[2].n * c) / (a + b + c);
                n = n.normalize_or(Vec3::Y);
                if area > 0.0 {
                    // triângulo visto de costas na tela: inverte a normal interpolada
                    n = -n;
                }
                t.color[i] = shade(n);
            }
        }
    }
}

/// Profundidade linear (distância ao longo do olhar) a partir do z [0, 1] da projeção perspectiva do glam.
fn linear_depth(z: f32, near: f32, far: f32) -> f32 {
    near * far / (far - z * (far - near)).max(1e-9)
}

fn stamp(out: &mut [[f32; 4]], w: usize, h: usize, cx: f32, cy: f32, r: f32, c: [f32; 3]) {
    let x0 = (cx - r).floor().max(0.0) as i64;
    let x1 = (cx + r).ceil().min(w as f32 - 1.0) as i64;
    let y0 = (cy - r).floor().max(0.0) as i64;
    let y1 = (cy + r).ceil().min(h as f32 - 1.0) as i64;
    let r2 = r * r;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            if dx * dx + dy * dy <= r2 {
                out[y as usize * w + x as usize] = [c[0], c[1], c[2], 1.0];
            }
        }
    }
}

/// Contorno: bordas entre partes, entre modelo e fundo e saltos de profundidade, engrossadas até `line_px`.
fn outline(t: &Target, out: &mut [[f32; 4]], line_px: f32, c: [f32; 3], near: f32, far: f32) {
    let (w, h) = (t.w, t.h);
    let mut edge = vec![false; w * h];
    let jump = |a: usize, b: usize| -> bool {
        let (ia, ib) = (t.id[a], t.id[b]);
        if ia != ib {
            return true;
        }
        if ia == 0 {
            return false;
        }
        let (za, zb) = (linear_depth(t.depth[a], near, far), linear_depth(t.depth[b], near, far));
        (za - zb).abs() > 0.04 * za.min(zb).max(1e-6)
    };
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if x + 1 < w && jump(i, i + 1) {
                // a linha fica do lado mais perto (ou do lado do modelo, contra o fundo)
                let near_side = if t.id[i] == 0 { i + 1 } else if t.id[i + 1] == 0 || t.depth[i] <= t.depth[i + 1] { i } else { i + 1 };
                edge[near_side] = true;
            }
            if y + 1 < h && jump(i, i + w) {
                let near_side = if t.id[i] == 0 { i + w } else if t.id[i + w] == 0 || t.depth[i] <= t.depth[i + w] { i } else { i + w };
                edge[near_side] = true;
            }
        }
    }
    let r = (line_px * 0.5).max(0.5);
    for y in 0..h {
        for x in 0..w {
            if edge[y * w + x] {
                stamp(out, w, h, x as f32 + 0.5, y as f32 + 0.5, r, c);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_edge(t: &Target, out: &mut [[f32; 4]], a: &CVert, b: &CVert, to_screen: &dyn Fn(Vec4) -> Vec3, line_px: f32, c: [f32; 3], near: f32, far: f32) {
    // recorta a aresta contra o plano perto
    let (mut a, mut b) = (*a, *b);
    if a.clip.z < 0.0 && b.clip.z < 0.0 {
        return;
    }
    if a.clip.z < 0.0 {
        a = lerp_v(&a, &b, a.clip.z / (a.clip.z - b.clip.z));
    } else if b.clip.z < 0.0 {
        b = lerp_v(&b, &a, b.clip.z / (b.clip.z - a.clip.z));
    }
    let (pa, pb) = (to_screen(a.clip), to_screen(b.clip));
    if !(pa.is_finite() && pb.is_finite()) {
        return;
    }
    let len = (pb.truncate() - pa.truncate()).length();
    let steps = (len.ceil() as usize).clamp(1, 1 << 16);
    let r = (line_px * 0.5).max(0.5);
    for k in 0..=steps {
        let s = k as f32 / steps as f32;
        let p = pa + (pb - pa) * s;
        let (x, y) = (p.x.floor() as i64, p.y.floor() as i64);
        if x < 0 || y < 0 || x >= t.w as i64 || y >= t.h as i64 {
            continue;
        }
        let i = y as usize * t.w + x as usize;
        let d = t.depth[i];
        // visível se não há superfície claramente à frente (tolerância relativa na profundidade linear)
        if d.is_finite() {
            let (zl, dl) = (linear_depth(p.z, near, far), linear_depth(d, near, far));
            if zl > dl * 1.01 + 1e-4 {
                continue;
            }
        }
        stamp(out, t.w, t.h, p.x, p.y, r, c);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{ModelInfo, Part};
    use aiai_core::BlobHash;
    use aiai_core::model3d::{OrbitCam3d, Transform3d};

    pub(crate) fn cube_model() -> Model {
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
            ([0.0, 0.0, 1.0], [[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]]),
            ([0.0, 0.0, -1.0], [[0.5, -0.5, -0.5], [-0.5, -0.5, -0.5], [-0.5, 0.5, -0.5], [0.5, 0.5, -0.5]]),
            ([0.0, 1.0, 0.0], [[-0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [0.5, 0.5, -0.5], [-0.5, 0.5, -0.5]]),
            ([0.0, -1.0, 0.0], [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]]),
            ([1.0, 0.0, 0.0], [[0.5, -0.5, 0.5], [0.5, -0.5, -0.5], [0.5, 0.5, -0.5], [0.5, 0.5, 0.5]]),
            ([-1.0, 0.0, 0.0], [[-0.5, -0.5, -0.5], [-0.5, -0.5, 0.5], [-0.5, 0.5, 0.5], [-0.5, 0.5, -0.5]]),
        ];
        for (n, c) in faces {
            let b = positions.len() as u32;
            positions.extend(c);
            normals.extend([n; 4]);
            indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let mut m = Model { parts: vec![Part { name: "cubo".into(), positions, normals, indices, color: [0.8, 0.2, 0.1, 1.0], material: "m".into() }], info: ModelInfo::default(), skin: None };
        m.refresh_info();
        m
    }

    pub(crate) fn state(mode: RenderMode3d) -> Model3dState {
        Model3dState {
            model_hash: BlobHash::of(b"cubo"),
            transform: Transform3d::default(),
            camera: OrbitCam3d { target: [0.0; 3], yaw_deg: 30.0, pitch_deg: 20.0, distance: 3.0, fov_deg: 40.0 },
            mode,
            outline_px: 2.0,
            outline_color: [0.0, 0.0, 0.0],
            color: None,
            pose: None,
        }
    }

    #[test]
    fn cube_renders_centered_premultiplied_with_transparent_background() {
        let m = cube_model();
        let img = render_cpu(&m, &state(RenderMode3d::Toon), 160, 90);
        assert_eq!(img.len(), 160 * 90);
        assert_eq!(img[0], [0.0; 4], "canto vazio transparente");
        let center = img[45 * 160 + 80];
        assert!(center[3] > 0.99, "o cubo cobre o centro: {center:?}");
        assert!(img.iter().all(|p| p[0] <= p[3] + 1e-5 && p[1] <= p[3] + 1e-5 && p[2] <= p[3] + 1e-5), "pré-multiplicado");
        // há duas cores de face (luz e sombra) e o preto do contorno
        let mut tones: Vec<[u8; 3]> = img.iter().filter(|p| p[3] > 0.999).map(|p| [(p[0] * 255.0) as u8, (p[1] * 255.0) as u8, (p[2] * 255.0) as u8]).collect();
        tones.sort();
        tones.dedup();
        assert!(tones.contains(&[0, 0, 0]), "contorno");
        assert!(tones.len() >= 3, "{tones:?}");
        // borda com alfa parcial (superamostragem)
        assert!(img.iter().any(|p| p[3] > 0.05 && p[3] < 0.95));
    }

    #[test]
    fn modes_differ_and_wire_has_no_fill() {
        let m = cube_model();
        let wire = render_cpu(&m, &state(RenderMode3d::Wire), 120, 120);
        let center = wire[60 * 120 + 60];
        // o centro da face da frente fica vazio no wire (só arestas)
        let covered = wire.iter().filter(|p| p[3] > 0.5).count();
        assert!(covered > 50 && covered < 120 * 120 / 4, "arestas finas: {covered}");
        let _ = center;
        let mut sil = state(RenderMode3d::Silhouette);
        sil.outline_px = 0.0;
        let s = render_cpu(&m, &sil, 120, 120);
        let c = s[60 * 120 + 60];
        assert!((c[0] - SILHOUETTE_DEFAULT[0]).abs() < 1e-4 && (c[2] - SILHOUETTE_DEFAULT[2]).abs() < 1e-4, "{c:?}");
        let flat = render_cpu(&m, &state(RenderMode3d::Flat), 120, 120);
        assert_ne!(flat, s);
    }

    #[test]
    fn transform_moves_the_model_on_screen() {
        let m = cube_model();
        let mut st = state(RenderMode3d::Flat);
        st.outline_px = 0.0;
        let a = render_cpu(&m, &st, 100, 100);
        st.transform.pos = [10.0, 0.0, 0.0];
        let b = render_cpu(&m, &st, 100, 100);
        assert!(a[50 * 100 + 50][3] > 0.9);
        assert!(b[50 * 100 + 50][3] < 0.01, "modelo saiu do quadro");
    }

    #[test]
    fn camera_inside_the_model_does_not_panic() {
        let m = cube_model();
        let mut st = state(RenderMode3d::Toon);
        st.camera.distance = 0.1;
        let img = render_cpu(&m, &st, 64, 64);
        assert_eq!(img.len(), 64 * 64);
    }
}
