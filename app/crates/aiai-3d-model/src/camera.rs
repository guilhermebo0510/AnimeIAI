//! Câmera orbital e visão em pixels. `OrbitCamera` e `CamView` são vendorizados do ANIGO
//! (`anigo-studio/src/viewport/camera.rs` e `scene/camview.rs`, ver `VENDOR.md`); a ponte com a câmera da camada
//! ([`aiai_core::model3d::OrbitCam3d`], em graus) e o enquadramento do modelo são do AnimeIAI.
//!
//! A "tela" da camada 3D é o canvas do corte: pixels do canvas, origem no canto superior esquerdo.

use aiai_core::model3d::{OrbitCam3d, Transform3d};
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OrbitCamera {
    pub target: Vec3,
    /// Rotação em torno de Y (radianos). 0 = olhando para -Z a partir de +Z (vista frontal).
    pub yaw: f32,
    /// Elevação (radianos), limitada a ±89,9°.
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
    pub orthographic: bool,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self { target: Vec3::new(0.0, 1.0, 0.0), yaw: 0.6, pitch: 0.35, distance: 5.0, fov_y: 40f32.to_radians(), near: 0.05, far: 500.0, orthographic: false }
    }
}

pub const MAX_PITCH: f32 = 89.9 * std::f32::consts::PI / 180.0;

impl OrbitCamera {
    /// Direção do alvo para a câmera (unitária).
    fn back(&self) -> Vec3 {
        Vec3::new(self.pitch.cos() * self.yaw.sin(), self.pitch.sin(), self.pitch.cos() * self.yaw.cos())
    }

    pub fn eye(&self) -> Vec3 {
        self.target + self.back() * self.distance
    }

    pub fn forward(&self) -> Vec3 {
        -self.back()
    }

    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X)
    }

    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward()).normalize_or_zero()
    }

    /// "Up" usado na matriz de visão (em visão de topo/baixo o up de mundo degenera).
    pub fn view_up(&self) -> Vec3 {
        if self.pitch.abs() > MAX_PITCH - 1e-3 { Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos()) * self.pitch.signum() } else { Vec3::Y }
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, self.view_up())
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        let aspect = aspect.max(1e-4);
        if self.orthographic {
            let half_h = self.distance * (self.fov_y * 0.5).tan();
            let half_w = half_h * aspect;
            Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, -self.far, self.far)
        } else {
            Mat4::perspective_rh(self.fov_y, aspect, self.near, self.far)
        }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// Arrasto de orbitar (pixels → radianos).
    pub fn orbit(&mut self, dx_px: f32, dy_px: f32) {
        const SENS: f32 = 0.008;
        self.yaw -= dx_px * SENS;
        self.pitch = (self.pitch + dy_px * SENS).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Arrasto de pan: move o alvo no plano da tela proporcional à distância.
    pub fn pan(&mut self, dx_px: f32, dy_px: f32, viewport_h_px: f32) {
        let world_per_px = 2.0 * self.distance * (self.fov_y * 0.5).tan() / viewport_h_px.max(1.0);
        self.target += (-self.right() * dx_px + self.up() * dy_px) * world_per_px;
    }

    /// Zoom multiplicativo (`factor` > 1 afasta). Distância limitada.
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(0.01, 400.0);
    }

    /// Enquadra uma caixa (min, max): ajusta alvo e distância para caber.
    pub fn frame_bounds(&mut self, min: Vec3, max: Vec3) {
        let center = (min + max) * 0.5;
        let radius = ((max - min).length() * 0.5).max(1e-3);
        self.target = center;
        self.distance = (radius / (self.fov_y * 0.5).sin().max(0.1)).clamp(0.05, 400.0);
    }

    /// Projeta um ponto de mundo para pixels (origem no canto superior esquerdo).
    pub fn project(&self, world: Vec3, size: Vec2) -> Option<(Vec2, f32)> {
        project_with(self.view_proj(size.x / size.y.max(1.0)), world, size)
    }
}

/// Projeta com uma matriz view-proj já calculada (evita recomputar por vértice).
pub fn project_with(view_proj: Mat4, world: Vec3, size: Vec2) -> Option<(Vec2, f32)> {
    let clip = view_proj * world.extend(1.0);
    if clip.w <= 1e-6 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some((Vec2::new((ndc.x * 0.5 + 0.5) * size.x, (1.0 - (ndc.y * 0.5 + 0.5)) * size.y), ndc.z))
}

/// Câmera + viewport em pixels: tudo o que o gizmo, o picking e os overlays precisam.
#[derive(Debug, Clone, Copy)]
pub struct CamView {
    pub view_proj: Mat4,
    pub inv_view_proj: Mat4,
    pub eye: Vec3,
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub viewport: Vec2,
    pub ortho: bool,
    pub fov_y: f32,
    /// Altura (mundo) do volume ortográfico, quando `ortho`.
    pub ortho_height: f32,
}

impl CamView {
    pub fn from_orbit(cam: &OrbitCamera, viewport: Vec2) -> Self {
        let aspect = viewport.x / viewport.y.max(1.0);
        let view_proj = cam.view_proj(aspect);
        Self {
            view_proj,
            inv_view_proj: view_proj.inverse(),
            eye: cam.eye(),
            forward: cam.forward(),
            right: cam.right(),
            up: cam.up(),
            viewport,
            ortho: cam.orthographic,
            fov_y: cam.fov_y,
            ortho_height: 2.0 * cam.distance * (cam.fov_y * 0.5).tan(),
        }
    }

    /// Pixel (origem no canto superior esquerdo) e profundidade NDC; `None` atrás da câmera.
    pub fn project(&self, p: Vec3) -> Option<(Vec2, f32)> {
        project_with(self.view_proj, p, self.viewport)
    }

    /// Raio de mundo (origem, direção unitária) a partir de um pixel.
    pub fn ray(&self, px: Vec2) -> (Vec3, Vec3) {
        let ndc = Vec2::new(px.x / self.viewport.x.max(1.0) * 2.0 - 1.0, 1.0 - px.y / self.viewport.y.max(1.0) * 2.0);
        let near = self.inv_view_proj.project_point3(Vec3::new(ndc.x, ndc.y, 0.0));
        let far = self.inv_view_proj.project_point3(Vec3::new(ndc.x, ndc.y, 1.0));
        if self.ortho { (near, self.forward) } else { (self.eye, (far - self.eye).normalize_or_zero()) }
    }

    /// Distância (mundo) que um pixel cobre na profundidade de `p`.
    pub fn world_per_pixel(&self, p: Vec3) -> f32 {
        let h = if self.ortho {
            self.ortho_height
        } else {
            let depth = (p - self.eye).dot(self.forward).abs().max(0.05);
            2.0 * depth * (self.fov_y * 0.5).tan()
        };
        h / self.viewport.y.max(1.0)
    }
}

// ------------------------------------------------------------------------------------------------ ponte com a camada

/// Câmera da camada (graus) → `OrbitCamera` (radianos), com planos perto/longe ajustados ao modelo (centro e raio em
/// mundo) para a profundidade ter precisão.
pub fn orbit_from_layer(c: &OrbitCam3d, model_center: Vec3, model_radius: f32) -> OrbitCamera {
    let mut cam = OrbitCamera {
        target: Vec3::from(c.target),
        yaw: c.yaw_deg.to_radians(),
        pitch: c.pitch_deg.to_radians().clamp(-MAX_PITCH, MAX_PITCH),
        distance: c.distance,
        fov_y: c.fov_deg.to_radians(),
        near: 0.01,
        far: 1000.0,
        orthographic: false,
    };
    let to_model = (model_center - cam.eye()).length();
    let r = model_radius.max(1e-3);
    cam.far = (to_model + r * 2.0).max(cam.distance * 2.0).max(1.0);
    cam.near = (to_model - r * 2.0).max(cam.far * 1e-4).max(1e-4);
    cam
}

/// `OrbitCamera` → câmera da camada (graus). Os planos não são guardados (são derivados do modelo).
pub fn layer_from_orbit(cam: &OrbitCamera) -> OrbitCam3d {
    OrbitCam3d { target: cam.target.to_array(), yaw_deg: cam.yaw.to_degrees(), pitch_deg: cam.pitch.to_degrees(), distance: cam.distance, fov_deg: cam.fov_y.to_degrees() }
}

/// Rotação da camada: Euler em graus, aplicada X, depois Y, depois Z, nos eixos do mundo.
pub fn rotation_quat(rot_deg: [f32; 3]) -> Quat {
    let [x, y, z] = rot_deg.map(f32::to_radians);
    Quat::from_rotation_z(z) * Quat::from_rotation_y(y) * Quat::from_rotation_x(x)
}

/// Inversa de [`rotation_quat`] (graus).
pub fn rotation_deg(q: Quat) -> [f32; 3] {
    let (z, y, x) = q.normalize().to_euler(EulerRot::ZYX);
    [x.to_degrees(), y.to_degrees(), z.to_degrees()]
}

/// Matriz modelo → mundo da camada.
pub fn model_matrix(t: &Transform3d) -> Mat4 {
    Mat4::from_scale_rotation_translation(Vec3::from(t.scale), rotation_quat(t.rot_deg), Vec3::from(t.pos))
}

/// Câmera que enquadra a caixa do modelo (3/4 de cima, como uma foto de referência), para a camada recém-criada.
pub fn framing_camera(bbox: [[f32; 3]; 2], aspect: f32) -> OrbitCam3d {
    let (lo, hi) = (Vec3::from(bbox[0]), Vec3::from(bbox[1]));
    let mut cam = OrbitCamera { yaw: 25f32.to_radians(), pitch: 12f32.to_radians(), fov_y: 35f32.to_radians(), ..Default::default() };
    cam.frame_bounds(lo, hi);
    // quadro mais estreito que alto: afasta para caber na largura também
    if aspect < 1.0 {
        cam.distance /= aspect.max(0.2);
    }
    layer_from_orbit(&cam)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: Vec2 = Vec2::new(800.0, 600.0);

    #[test]
    fn target_projects_to_viewport_center() {
        let cam = OrbitCamera::default();
        let (p, depth) = cam.project(cam.target, SIZE).unwrap();
        assert!((p - SIZE * 0.5).length() < 0.5, "{p:?}");
        assert!((0.0..1.0).contains(&depth));
    }

    #[test]
    fn pixel_ray_roundtrips_projection() {
        let cam = OrbitCamera::default();
        let v = CamView::from_orbit(&cam, SIZE);
        let world = Vec3::new(0.7, 1.4, -0.3);
        let (px, _) = v.project(world).unwrap();
        let (o, d) = v.ray(px);
        let to = world - o;
        assert!((to - d * to.dot(d)).length() < 1e-3);
    }

    #[test]
    fn orbit_keeps_distance_and_clamps_pitch() {
        let mut cam = OrbitCamera::default();
        let d0 = (cam.eye() - cam.target).length();
        cam.orbit(120.0, 5000.0);
        assert!(((cam.eye() - cam.target).length() - d0).abs() < 1e-4);
        assert!(cam.pitch <= MAX_PITCH + 1e-6);
        assert!(cam.view().is_finite());
    }

    #[test]
    fn pan_moves_target_in_screen_plane() {
        let mut cam = OrbitCamera::default();
        let t0 = cam.target;
        cam.pan(100.0, 0.0, 600.0);
        let delta = cam.target - t0;
        assert!(delta.dot(cam.forward()).abs() < 1e-5);
        assert!(delta.dot(cam.right()) < 0.0);
    }

    #[test]
    fn layer_camera_round_trip_and_framing() {
        let c = OrbitCam3d { target: [1.0, 2.0, 3.0], yaw_deg: 30.0, pitch_deg: -10.0, distance: 6.0, fov_deg: 40.0 };
        let back = layer_from_orbit(&orbit_from_layer(&c, Vec3::ZERO, 1.0));
        assert!((back.yaw_deg - 30.0).abs() < 1e-3 && (back.pitch_deg + 10.0).abs() < 1e-3 && (back.fov_deg - 40.0).abs() < 1e-3);
        let f = framing_camera([[-1.0, 0.0, -1.0], [1.0, 2.0, 1.0]], 16.0 / 9.0);
        let cam = orbit_from_layer(&f, Vec3::new(0.0, 1.0, 0.0), 1.8);
        for corner in [Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 2.0, 1.0), Vec3::new(-1.0, 2.0, 1.0)] {
            let (p, _) = cam.project(corner, Vec2::new(1600.0, 900.0)).unwrap();
            assert!(p.x > 0.0 && p.x < 1600.0 && p.y > 0.0 && p.y < 900.0, "{corner:?} → {p:?}");
        }
    }

    #[test]
    fn euler_round_trip() {
        for r in [[10.0, 20.0, 30.0], [-45.0, 80.0, 5.0], [0.0, 0.0, 0.0]] {
            let back = rotation_deg(rotation_quat(r));
            for k in 0..3 {
                assert!((back[k] - r[k]).abs() < 1e-2, "{r:?} → {back:?}");
            }
        }
    }
}
