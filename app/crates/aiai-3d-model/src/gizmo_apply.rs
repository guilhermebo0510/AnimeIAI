//! Aplicação de um arrasto do gizmo ([`crate::gizmo::DragDelta`]) à transformação da camada 3D. O pivô é o centro da
//! caixa do modelo (onde o gizmo é desenhado): girar e escalar mantêm esse ponto parado no mundo.

use crate::camera::{model_matrix, rotation_deg, rotation_quat};
use crate::gizmo::{DragDelta, GizmoFrame, GizmoMode};
use aiai_core::model3d::{MIN_SCALE, Transform3d};
use glam::{Quat, Vec3};

/// Passo do encaixe angular (Shift no arrasto do anel), em graus.
pub const ROTATE_SNAP_DEG: f32 = 15.0;

/// Posição do pivô no mundo.
pub fn pivot_world(t: &Transform3d, pivot_model: Vec3) -> Vec3 {
    model_matrix(t).transform_point3(pivot_model)
}

/// Quadro do gizmo para o modo: eixos do mundo para mover e girar, eixos do modelo para escalar (a escala é por eixo
/// do modelo).
pub fn frame_for(mode: GizmoMode, t: &Transform3d, pivot_model: Vec3, view: &crate::camera::CamView) -> GizmoFrame {
    let q = if mode == GizmoMode::Scale { rotation_quat(t.rot_deg) } else { Quat::IDENTITY };
    GizmoFrame::new(pivot_world(t, pivot_model), q, view)
}

/// Nova transformação a partir da do início do arrasto. `snap` encaixa a rotação em passos de [`ROTATE_SNAP_DEG`].
pub fn apply(start: &Transform3d, pivot_model: Vec3, delta: DragDelta, snap: bool) -> Transform3d {
    let pivot = pivot_world(start, pivot_model);
    let mut out = *start;
    match delta {
        DragDelta::Translate(d) => {
            out.pos = (Vec3::from(start.pos) + d).to_array();
            return out;
        }
        DragDelta::Rotate { axis, angle } => {
            let a = if snap {
                let step = ROTATE_SNAP_DEG.to_radians();
                (angle / step).round() * step
            } else {
                angle
            };
            let q = (Quat::from_axis_angle(axis.normalize_or(Vec3::Y), a) * rotation_quat(start.rot_deg)).normalize();
            out.rot_deg = rotation_deg(q);
        }
        DragDelta::Scale { factors, .. } => {
            let s = Vec3::from(start.scale) * factors;
            out.scale = s.to_array().map(|v| if v.abs() < MIN_SCALE { MIN_SCALE.copysign(v) } else { v });
        }
    }
    // o pivô (em espaço do modelo) volta ao mesmo ponto do mundo: pos' = pivô − R'·S'·pivô_modelo
    let r = rotation_quat(out.rot_deg);
    let s = Vec3::from(out.scale);
    out.pos = (pivot - r * (s * pivot_model)).to_array();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{CamView, OrbitCamera};
    use crate::gizmo::{Axis3, GizmoDrag, Handle};
    use glam::Vec2;

    fn view() -> CamView {
        let cam = OrbitCamera { target: Vec3::new(0.0, 1.0, 0.0), yaw: 0.35, pitch: 0.3, distance: 7.0, ..Default::default() };
        CamView::from_orbit(&cam, Vec2::new(1920.0, 1080.0))
    }

    #[test]
    fn translate_moves_position_only() {
        let t = Transform3d { pos: [1.0, 0.0, 0.0], rot_deg: [0.0, 30.0, 0.0], scale: [2.0; 3] };
        let o = apply(&t, Vec3::new(0.0, 0.5, 0.0), DragDelta::Translate(Vec3::new(0.0, 1.0, -2.0)), false);
        assert_eq!(o.pos, [1.0, 1.0, -2.0]);
        assert_eq!((o.rot_deg, o.scale), (t.rot_deg, t.scale));
    }

    #[test]
    fn rotate_and_scale_keep_the_pivot_fixed() {
        let t = Transform3d { pos: [0.5, 0.0, -1.0], rot_deg: [0.0, 10.0, 0.0], scale: [1.0; 3] };
        let pm = Vec3::new(0.0, 0.9, 0.2);
        let p0 = pivot_world(&t, pm);
        let r = apply(&t, pm, DragDelta::Rotate { axis: Vec3::Y, angle: 1.0 }, false);
        assert!((pivot_world(&r, pm) - p0).length() < 1e-4);
        assert!((r.rot_deg[1] - (10.0 + 1f32.to_degrees())).abs() < 1e-2, "{:?}", r.rot_deg);
        let s = apply(&t, pm, DragDelta::Scale { factors: Vec3::new(2.0, 1.0, 1.0), uniform: false }, false);
        assert!((pivot_world(&s, pm) - p0).length() < 1e-4);
        assert_eq!(s.scale, [2.0, 1.0, 1.0]);
    }

    #[test]
    fn rotation_snaps_to_15_degrees() {
        let t = Transform3d::default();
        let r = apply(&t, Vec3::ZERO, DragDelta::Rotate { axis: Vec3::Y, angle: 22f32.to_radians() }, true);
        assert!((r.rot_deg[1] - 15.0).abs() < 1e-3, "{:?}", r.rot_deg);
    }

    /// Ponta a ponta: arrastar a seta X do gizmo (desenhado no pivô) leva o modelo junto com o cursor.
    #[test]
    fn dragging_the_x_arrow_moves_the_model_under_the_cursor() {
        let v = view();
        let t = Transform3d::default();
        let pm = Vec3::new(0.0, 1.0, 0.0);
        let f = frame_for(GizmoMode::Translate, &t, pm, &v);
        let (s, e) = f.axis_segment(Axis3::X);
        let grab = s.lerp(e, 0.5);
        let (p0, _) = v.project(grab).unwrap();
        assert_eq!(crate::gizmo::hit_test(GizmoMode::Translate, &f, &v, p0), Some(Handle::Axis(Axis3::X)));
        let mut drag = GizmoDrag::begin(GizmoMode::Translate, Handle::Axis(Axis3::X), f, &v, p0).unwrap();
        let (p1, _) = v.project(grab + Vec3::X * 0.75).unwrap();
        let d = drag.update(&v, p1).unwrap();
        let o = apply(&t, pm, d, false);
        assert!((o.pos[0] - 0.75).abs() < 0.02 && o.pos[1].abs() < 1e-4 && o.pos[2].abs() < 1e-4, "{:?}", o.pos);
    }
}
