//! Malhas de triângulos simples (primitivas de blocagem).

use glam::{Mat4, Vec3};

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub verts: Vec<Vec3>,
    pub tris: Vec<[u32; 3]>,
}

impl Mesh {
    pub fn transformed(&self, m: &Mat4) -> Mesh {
        Mesh { verts: self.verts.iter().map(|v| m.transform_point3(*v)).collect(), tris: self.tris.clone() }
    }

    pub fn append(&mut self, other: &Mesh) {
        let base = self.verts.len() as u32;
        self.verts.extend_from_slice(&other.verts);
        self.tris.extend(other.tris.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }

    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.tris.push([a, b, c]);
        self.tris.push([a, c, d]);
    }

    /// Caixa centrada na origem.
    pub fn cuboid(size: Vec3) -> Mesh {
        let h = size * 0.5;
        let mut m = Mesh::default();
        for z in [-h.z, h.z] {
            for y in [-h.y, h.y] {
                for x in [-h.x, h.x] {
                    m.verts.push(Vec3::new(x, y, z));
                }
            }
        }
        // índices: bit0 = x, bit1 = y, bit2 = z ; faces com normal para fora (anti-horário visto de fora)
        m.quad(0, 2, 3, 1); // -z
        m.quad(4, 5, 7, 6); // +z
        m.quad(0, 1, 5, 4); // -y
        m.quad(2, 6, 7, 3); // +y
        m.quad(0, 4, 6, 2); // -x
        m.quad(1, 3, 7, 5); // +x
        m
    }

    /// Cilindro ao longo de Y, centrado na origem.
    pub fn cylinder(radius: f32, height: f32, segments: u32) -> Mesh {
        Mesh::frustum(radius, radius, height, segments)
    }

    /// Tronco de cone ao longo de Y (raio de baixo, raio de cima).
    pub fn frustum(r0: f32, r1: f32, height: f32, segments: u32) -> Mesh {
        let n = segments.max(3);
        let mut m = Mesh::default();
        let h = height * 0.5;
        for k in 0..n {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            m.verts.push(Vec3::new(c * r0, -h, s * r0));
            m.verts.push(Vec3::new(c * r1, h, s * r1));
        }
        let bottom = m.verts.len() as u32;
        m.verts.push(Vec3::new(0.0, -h, 0.0));
        let top = m.verts.len() as u32;
        m.verts.push(Vec3::new(0.0, h, 0.0));
        for k in 0..n {
            let (a0, a1) = (2 * k, 2 * k + 1);
            let (b0, b1) = (2 * ((k + 1) % n), 2 * ((k + 1) % n) + 1);
            m.quad(a0, a1, b1, b0);
            m.tris.push([bottom, a0, b0]);
            m.tris.push([top, b1, a1]);
        }
        m
    }

    /// Esfera UV (ou elipsoide via escala).
    pub fn sphere(radius: f32, rings: u32, segments: u32) -> Mesh {
        let (rings, segs) = (rings.max(3), segments.max(4));
        let mut m = Mesh::default();
        for r in 0..=rings {
            let phi = r as f32 / rings as f32 * std::f32::consts::PI;
            for sgm in 0..segs {
                let th = sgm as f32 / segs as f32 * std::f32::consts::TAU;
                m.verts.push(Vec3::new(radius * phi.sin() * th.cos(), radius * phi.cos(), radius * phi.sin() * th.sin()));
            }
        }
        for r in 0..rings {
            for sgm in 0..segs {
                let a = r * segs + sgm;
                let b = r * segs + (sgm + 1) % segs;
                let c = (r + 1) * segs + (sgm + 1) % segs;
                let d = (r + 1) * segs + sgm;
                m.quad(a, b, c, d);
            }
        }
        m
    }

    /// Plano XZ centrado, normal +Y.
    pub fn plane(w: f32, d: f32) -> Mesh {
        let mut m = Mesh::default();
        let (hw, hd) = (w * 0.5, d * 0.5);
        m.verts = vec![Vec3::new(-hw, 0.0, -hd), Vec3::new(hw, 0.0, -hd), Vec3::new(hw, 0.0, hd), Vec3::new(-hw, 0.0, hd)];
        m.quad(0, 3, 2, 1);
        m
    }

    /// Escada de `steps` degraus subindo em +Z, base em y=0 centrada em x.
    pub fn stairs(steps: u32, width: f32, depth: f32, height: f32) -> Mesh {
        let mut m = Mesh::default();
        let n = steps.max(1);
        let (sd, sh) = (depth / n as f32, height / n as f32);
        for k in 0..n {
            let h = sh * (k + 1) as f32;
            let mut b = Mesh::cuboid(Vec3::new(width, h, sd));
            let t = Mat4::from_translation(Vec3::new(0.0, h * 0.5, sd * (k as f32 + 0.5) - depth * 0.5));
            b = b.transformed(&t);
            m.append(&b);
        }
        m
    }

    /// Normal da face (não normalizada se `raw`).
    pub fn face_normal(&self, t: usize) -> Vec3 {
        let [a, b, c] = self.tris[t];
        let (a, b, c) = (self.verts[a as usize], self.verts[b as usize], self.verts[c as usize]);
        (b - a).cross(c - a).normalize_or_zero()
    }
}
