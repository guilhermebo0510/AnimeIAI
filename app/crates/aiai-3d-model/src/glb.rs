//! GLB normalizado do AnimeIAI: o formato único em que o modelo vai para o blob store (o importador de OBJ/glTF converte
//! para cá; o carregador em tempo de execução é o mesmo importador de GLB). Um nó por parte, sem hierarquia (as
//! transformações já foram aplicadas), POSITION/NORMAL em float, índices u32, um material por cor e as informações de
//! origem em `asset.extras.aiai`. Com pele ([`crate::model::Skin`]): nós de junta em hierarquia (repouso local),
//! `JOINTS_0` (u16) e `WEIGHTS_0` (float) por parte e `skins[0]` com as inversas de bind (a malha já está no repouso).
//! Determinístico: o mesmo modelo dá os mesmos bytes (e o mesmo hash).

use crate::gltf;
use crate::import::{ImportError, import_bytes};
use crate::model::Model;
use glam::{Mat4, Quat, Vec3};
use serde_json::{Value, json};

fn push_f32s(bin: &mut Vec<u8>, data: impl Iterator<Item = f32>) -> (usize, usize) {
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let start = bin.len();
    for v in data {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    (start, bin.len() - start)
}

fn push_u16s(bin: &mut Vec<u8>, data: impl Iterator<Item = u16>) -> (usize, usize) {
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let start = bin.len();
    for v in data {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    (start, bin.len() - start)
}

/// Repouso global de um osso (posição + rotação).
fn bone_global(b: &crate::model::SkinBone) -> Mat4 {
    Mat4::from_rotation_translation(Quat::from_array(b.rest_rot).normalize(), Vec3::from(b.rest_pos))
}

fn push_u32s(bin: &mut Vec<u8>, data: &[u32]) -> (usize, usize) {
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let start = bin.len();
    for v in data {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    (start, bin.len() - start)
}

/// Escreve o modelo como GLB normalizado.
pub fn write_glb(model: &Model) -> Vec<u8> {
    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    let mut materials: Vec<Value> = Vec::new();
    let mut mat_index: Vec<([u32; 4], String, usize)> = Vec::new();
    for (i, p) in model.parts.iter().enumerate() {
        let key = p.color.map(f32::to_bits);
        let mi = match mat_index.iter().find(|(k, n, _)| *k == key && *n == p.material) {
            Some((_, _, mi)) => *mi,
            None => {
                materials.push(json!({ "name": p.material, "pbrMetallicRoughness": { "baseColorFactor": p.color, "metallicFactor": 0.0, "roughnessFactor": 1.0 } }));
                mat_index.push((key, p.material.clone(), materials.len() - 1));
                materials.len() - 1
            }
        };
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for v in &p.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        let (ps, pl) = push_f32s(&mut bin, p.positions.iter().flatten().copied());
        views.push(json!({ "buffer": 0, "byteOffset": ps, "byteLength": pl, "target": 34962 }));
        accessors.push(json!({ "bufferView": views.len() - 1, "componentType": gltf::COMPONENT_F32, "count": p.positions.len(), "type": "VEC3", "min": lo, "max": hi }));
        let pos_acc = accessors.len() - 1;
        let (ns, nl) = push_f32s(&mut bin, p.normals.iter().flatten().copied());
        views.push(json!({ "buffer": 0, "byteOffset": ns, "byteLength": nl, "target": 34962 }));
        accessors.push(json!({ "bufferView": views.len() - 1, "componentType": gltf::COMPONENT_F32, "count": p.normals.len(), "type": "VEC3" }));
        let nor_acc = accessors.len() - 1;
        let (is, il) = push_u32s(&mut bin, &p.indices);
        views.push(json!({ "buffer": 0, "byteOffset": is, "byteLength": il, "target": 34963 }));
        accessors.push(json!({ "bufferView": views.len() - 1, "componentType": gltf::COMPONENT_U32, "count": p.indices.len(), "type": "SCALAR" }));
        let idx_acc = accessors.len() - 1;
        let mut attributes = json!({ "POSITION": pos_acc, "NORMAL": nor_acc });
        let ps = model.skin.as_ref().and_then(|s| s.parts.get(i)).filter(|ps| ps.joints.len() == p.positions.len());
        if let Some(ps) = ps {
            let (js, jl) = push_u16s(&mut bin, ps.joints.iter().flatten().copied());
            views.push(json!({ "buffer": 0, "byteOffset": js, "byteLength": jl, "target": 34962 }));
            accessors.push(json!({ "bufferView": views.len() - 1, "componentType": gltf::COMPONENT_U16, "count": ps.joints.len(), "type": "VEC4" }));
            attributes["JOINTS_0"] = json!(accessors.len() - 1);
            let (ws, wl) = push_f32s(&mut bin, ps.weights.iter().flatten().copied());
            views.push(json!({ "buffer": 0, "byteOffset": ws, "byteLength": wl, "target": 34962 }));
            accessors.push(json!({ "bufferView": views.len() - 1, "componentType": gltf::COMPONENT_F32, "count": ps.weights.len(), "type": "VEC4" }));
            attributes["WEIGHTS_0"] = json!(accessors.len() - 1);
        }
        meshes.push(json!({ "name": p.name, "primitives": [{ "attributes": attributes, "indices": idx_acc, "material": mi, "mode": 4 }] }));
        if ps.is_some() {
            nodes.push(json!({ "name": p.name, "mesh": i, "skin": 0 }));
        } else {
            nodes.push(json!({ "name": p.name, "mesh": i }));
        }
    }
    let mut scene_nodes: Vec<usize> = (0..nodes.len()).collect();
    let mut skins: Vec<Value> = Vec::new();
    if let Some(skin) = model.skin.as_ref().filter(|s| !s.bones.is_empty()) {
        let first = nodes.len();
        let globals: Vec<Mat4> = skin.bones.iter().map(bone_global).collect();
        for (bi, b) in skin.bones.iter().enumerate() {
            let local = match b.parent {
                Some(pa) => globals[pa].inverse() * globals[bi],
                None => globals[bi],
            };
            let (_, r, t) = local.to_scale_rotation_translation();
            let children: Vec<usize> = skin.bones.iter().enumerate().filter(|(_, c)| c.parent == Some(bi)).map(|(ci, _)| first + ci).collect();
            let mut n = json!({ "name": b.name, "translation": t.to_array(), "rotation": r.normalize().to_array() });
            if !children.is_empty() {
                n["children"] = json!(children);
            }
            nodes.push(n);
            if b.parent.is_none() {
                scene_nodes.push(first + bi);
            }
        }
        let (is, il) = push_f32s(&mut bin, globals.iter().flat_map(|g| g.inverse().to_cols_array()));
        views.push(json!({ "buffer": 0, "byteOffset": is, "byteLength": il }));
        accessors.push(json!({ "bufferView": views.len() - 1, "componentType": gltf::COMPONENT_F32, "count": skin.bones.len(), "type": "MAT4" }));
        skins.push(json!({ "joints": (first..first + skin.bones.len()).collect::<Vec<_>>(), "inverseBindMatrices": accessors.len() - 1, "name": "esqueleto" }));
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let info = &model.info;
    let mut doc = json!({
        "asset": {
            "version": "2.0",
            "generator": "AnimeIAI aiai-3d-model",
            "extras": { "aiai": { "version": 1, "format": info.format, "file_name": info.file_name, "license": info.license, "warnings": info.warnings } },
        },
        "scene": 0,
        "scenes": [{ "nodes": scene_nodes }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{ "byteLength": bin.len() }],
    });
    if !skins.is_empty() {
        doc["skins"] = json!(skins);
    }
    gltf::build_glb(&doc, &bin)
}

/// Lê um GLB normalizado (ou qualquer GLB) de volta para o [`Model`].
pub fn read_glb(bytes: &[u8]) -> Result<Model, ImportError> {
    import_bytes(bytes, "glb", "", &|_| None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LicenseInfo, ModelInfo, Part};

    pub(crate) fn two_part_model() -> Model {
        let quad = |y: f32, color: [f32; 4], name: &str| Part {
            name: name.into(),
            positions: vec![[0.0, y, 0.0], [1.0, y, 0.0], [1.0, y, 1.0], [0.0, y, 1.0]],
            normals: vec![[0.0, 1.0, 0.0]; 4],
            indices: vec![0, 2, 1, 0, 3, 2],
            color,
            material: name.into(),
        };
        let mut m = Model {
            parts: vec![quad(0.0, [1.0, 0.0, 0.0, 1.0], "chao"), quad(1.0, [0.0, 0.0, 1.0, 1.0], "teto")],
            info: ModelInfo { format: "obj".into(), file_name: "sala.obj".into(), license: LicenseInfo { license: "CC0".into(), ..Default::default() }, ..Default::default() },
            skin: None,
        };
        m.refresh_info();
        m
    }

    #[test]
    fn glb_round_trip_keeps_geometry_colors_and_origin() {
        let m = two_part_model();
        let bytes = write_glb(&m);
        assert_eq!(write_glb(&m), bytes, "determinístico");
        let back = read_glb(&bytes).unwrap();
        assert_eq!(back.parts.len(), 2);
        assert_eq!(back.parts[0].positions, m.parts[0].positions);
        assert_eq!(back.parts[1].indices, m.parts[1].indices);
        assert_eq!(back.parts[1].color, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(back.info.format, "obj");
        assert_eq!(back.info.file_name, "sala.obj");
        assert_eq!(back.info.license.license, "CC0");
        assert_eq!(back.info.triangles, 4);
        assert_eq!(back.info.bbox, [[0.0; 3], [1.0; 3]]);
    }

    /// GLB "de fora" com dois nós (um filho girado e deslocado do outro) e duas malhas: as transformações dos nós
    /// entram na geometria.
    #[test]
    fn multi_node_gltf_applies_node_transforms() {
        let tri: Vec<f32> = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let mut bin = Vec::new();
        for v in &tri {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        let doc = json!({
            "asset": { "version": "2.0", "copyright": "Fulano 2026", "extras": { "author": "Fulano", "license": "CC-BY 4.0" } },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [
                { "name": "raiz", "mesh": 0, "translation": [10.0, 0.0, 0.0], "children": [1] },
                { "name": "filho", "mesh": 1, "translation": [0.0, 5.0, 0.0], "scale": [2.0, 2.0, 2.0] }
            ],
            "meshes": [
                { "name": "A", "primitives": [{ "attributes": { "POSITION": 0 } }] },
                { "name": "B", "primitives": [{ "attributes": { "POSITION": 0 }, "material": 0 }] }
            ],
            "materials": [{ "name": "verde", "pbrMetallicRoughness": { "baseColorFactor": [0.0, 1.0, 0.0, 1.0] } }],
            "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0] }],
            "bufferViews": [{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }],
            "buffers": [{ "byteLength": 36 }]
        });
        let glb = gltf::build_glb(&doc, &bin);
        let m = read_glb(&glb).unwrap();
        assert_eq!(m.parts.len(), 2);
        assert_eq!(m.parts[0].positions[1], [11.0, 0.0, 0.0]);
        // filho: escala 2, sobe 5, herda o deslocamento de 10 do pai
        assert_eq!(m.parts[1].positions[2], [10.0, 7.0, 0.0]);
        assert_eq!(m.parts[1].color, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(m.info.license.author, "Fulano");
        assert_eq!(m.info.license.license, "CC-BY 4.0");
        assert_eq!(m.info.license.copyright, "Fulano 2026");
        // sem NORMAL no arquivo: normais calculadas (o triângulo está no plano XY)
        assert!((m.parts[0].normals[0][2].abs() - 1.0).abs() < 1e-5);
        // e o GLB normalizado guarda tudo isso
        let again = read_glb(&write_glb(&m)).unwrap();
        assert_eq!(again.parts[1].positions, m.parts[1].positions);
        assert_eq!(again.info.license, m.info.license);
    }

    /// Pele no GLB normalizado: esqueleto, pesos e repouso voltam iguais (a malha não muda ao reler).
    #[test]
    fn skin_round_trips_through_the_normalized_glb() {
        use crate::model::{PartSkin, Skin, SkinBone};
        let mut m = two_part_model();
        let r = Quat::from_rotation_z(0.3).to_array();
        m.skin = Some(Skin {
            bones: vec![
                SkinBone { name: "hips".into(), parent: None, rest_pos: [0.5, 0.5, 0.5], rest_rot: [0.0, 0.0, 0.0, 1.0] },
                SkinBone { name: "spine".into(), parent: Some(0), rest_pos: [0.5, 1.0, 0.5], rest_rot: r },
            ],
            parts: vec![
                PartSkin { joints: vec![[0, 0, 0, 0]; 4], weights: vec![[1.0, 0.0, 0.0, 0.0]; 4] },
                PartSkin { joints: vec![[1, 0, 0, 0]; 4], weights: vec![[0.75, 0.25, 0.0, 0.0]; 4] },
            ],
        });
        m.refresh_info();
        let bytes = write_glb(&m);
        assert_eq!(write_glb(&m), bytes, "determinístico");
        let back = read_glb(&bytes).unwrap();
        let s = back.skin.as_ref().expect("pele");
        assert_eq!(s.bones.len(), 2);
        assert_eq!(s.bones[1].name, "spine");
        assert_eq!(s.bones[1].parent, Some(0));
        for k in 0..3 {
            assert!((s.bones[1].rest_pos[k] - [0.5, 1.0, 0.5][k]).abs() < 1e-5);
        }
        assert!(Quat::from_array(s.bones[1].rest_rot).dot(Quat::from_array(r)).abs() > 0.99999);
        assert_eq!(s.parts[1].joints[0], [1, 0, 0, 0]);
        assert_eq!(s.parts[1].weights[0], [0.75, 0.25, 0.0, 0.0]);
        for (a, b) in back.parts[1].positions.iter().zip(&m.parts[1].positions) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 1e-5, "{a:?} {b:?}");
            }
        }
        assert_eq!(back.info.bones, 2);
    }

    #[test]
    fn vrm_is_refused_with_em_breve() {
        let doc = json!({ "asset": { "version": "2.0" }, "extensionsUsed": ["VRMC_vrm"], "buffers": [] });
        let e = read_glb(&gltf::build_glb(&doc, &[])).unwrap_err();
        assert!(e.0.contains("em breve"), "{e}");
    }
}
