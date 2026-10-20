use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Standard Anime NPR vertex definition with channels tailored for Cel-Shading, Inverted Hull & Skinning.
/// Exact 72-byte packed layout:
/// - position: [f32; 3] (12 bytes, offset 0)
/// - normal:   [f32; 3] (12 bytes, offset 12)
/// - uv:       [f32; 2] (8 bytes,  offset 24)
/// - color:    [f32; 4] (16 bytes, offset 32)
/// - joints:   [u16; 4] (8 bytes,  offset 48)
/// - weights:  [f32; 4] (16 bytes, offset 56)
///
/// Total: 72 bytes, 4-byte aligned, zero padding bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, Serialize, Deserialize, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Anime vertex attribute encoding:
    /// R: Stylized Ambient Occlusion
    /// G: Shadow threshold shift
    /// B: Outline extrusion width multiplier
    /// A: Specular / Rim light mask
    pub color: [f32; 4],
    /// Skinned mesh bone joint indices (up to 4 joints per vertex)
    pub joints: [u16; 4],
    /// Skinned mesh bone weights (normalized, summing to 1.0)
    pub weights: [f32; 4],
}

impl Vertex {
    pub fn new(position: [f32; 3], normal: [f32; 3], uv: [f32; 2]) -> Self {
        Self {
            position,
            normal,
            uv,
            color: [1.0, 0.5, 1.0, 1.0],
            joints: [0, 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        }
    }

    pub fn with_color(position: [f32; 3], normal: [f32; 3], uv: [f32; 2], color: [f32; 4]) -> Self {
        Self {
            position,
            normal,
            uv,
            color,
            joints: [0, 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        }
    }

    pub fn with_skinning(
        position: [f32; 3],
        normal: [f32; 3],
        uv: [f32; 2],
        color: [f32; 4],
        joints: [u16; 4],
        weights: [f32; 4],
    ) -> Self {
        Self {
            position,
            normal,
            uv,
            color,
            joints,
            weights,
        }
    }
}

/// Custom / application-specific glTF attribute buffer (e.g. `_ANIGO_AO`, `_ANIGO_MORPH_DELTA`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomAttribute {
    pub name: String,
    pub component_type: u32,
    pub attribute_type: String, // "SCALAR", "VEC2", "VEC3", "VEC4"
    pub count: usize,
    pub data: Vec<u8>,
}

/// Errors occurring during glTF 2.0 / GLB parsing, validation and isomorphism checking.
#[derive(Debug, thiserror::Error)]
pub enum GltfMeshError {
    #[error("Invalid glTF magic number: expected 0x46546C67 (glTF), found {0:#x}")]
    InvalidMagic(u32),
    #[error("Unsupported glTF version: {0} (expected 2)")]
    UnsupportedVersion(u32),
    #[error("Corrupted GLB chunk or unexpected EOF")]
    UnexpectedEof,
    #[error("JSON chunk not found in GLB container")]
    MissingJsonChunk,
    #[error("Binary chunk not found in GLB container")]
    MissingBinaryChunk,
    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Accessor out of bounds or invalid: {0}")]
    InvalidAccessor(String),
    #[error("BufferView out of bounds: {0}")]
    InvalidBufferView(String),
    #[error("Mesh does not contain POSITION attribute")]
    MissingPosition,
    #[error("Topology isomorphism mismatch: {0}")]
    IsomorphismMismatch(String),
}

/// Biological gender archetype for canonical 1:1 base meshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaseGender {
    Male,
    Female,
}

/// 3D Geometry mesh consisting of indexed vertices.
///
/// `PartialEq` is derived so scene-level structures (`SceneNode`, `Scene`) can
/// compare whole render graphs — the parity tests use it to prove that the
/// viewport and the export paths build the *same* scene from the same project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub name: String,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub custom_attributes: HashMap<String, CustomAttribute>,
}

fn get_component_byte_size(comp_type: u32) -> Result<usize, GltfMeshError> {
    match comp_type {
        5120 | 5121 => Ok(1), // BYTE, UNSIGNED_BYTE
        5122 | 5123 => Ok(2), // SHORT, UNSIGNED_SHORT
        5125 => Ok(4),        // UNSIGNED_INT
        5126 => Ok(4),        // FLOAT
        other => Err(GltfMeshError::InvalidAccessor(format!("Unsupported glTF componentType: {}", other))),
    }
}

fn get_type_component_count(type_str: &str) -> Result<usize, GltfMeshError> {
    match type_str {
        "SCALAR" => Ok(1),
        "VEC2" => Ok(2),
        "VEC3" => Ok(3),
        "VEC4" => Ok(4),
        "MAT2" => Ok(4),
        "MAT3" => Ok(9),
        "MAT4" => Ok(16),
        other => Err(GltfMeshError::InvalidAccessor(format!("Unsupported glTF type: {}", other))),
    }
}

impl Mesh {
    pub fn new(name: impl Into<String>, vertices: Vec<Vertex>, indices: Vec<u32>) -> Self {
        Self {
            name: name.into(),
            vertices,
            indices,
            custom_attributes: HashMap::new(),
        }
    }

    pub fn with_custom_attribute(mut self, name: impl Into<String>, attr: CustomAttribute) -> Self {
        self.custom_attributes.insert(name.into(), attr);
        self
    }

    /// AABB local da malha (issue #13), em espaço de objeto.
    ///
    /// É o volume que o culling transforma pela matriz mundial do nó: nenhuma
    /// conta por vértice é feita por quadro.
    pub fn local_bounds(&self) -> Option<crate::math::Aabb> {
        crate::math::Aabb::from_points(
            self.vertices
                .iter()
                .map(|vertex| glam::Vec3::from_array(vertex.position)),
        )
    }

    /// Esfera envolvente local da malha (issue #13).
    pub fn local_bounding_sphere(&self) -> Option<crate::math::BoundingSphere> {
        self.local_bounds().map(|bounds| bounds.bounding_sphere())
    }

    /// Creates a unit cube with distinct normals per face positioned at the specified center.
    pub fn create_cube_at(size: f32, center: [f32; 3]) -> Self {
        let h = size * 0.5;
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let faces = [
            // Front (+Z)
            ([0.0, 0.0, 1.0], [[-h, -h, h], [h, -h, h], [h, h, h], [-h, h, h]]),
            // Back (-Z)
            ([0.0, 0.0, -1.0], [[h, -h, -h], [-h, -h, -h], [-h, h, -h], [h, h, -h]]),
            // Top (+Y)
            ([0.0, 1.0, 0.0], [[-h, h, h], [h, h, h], [h, h, -h], [-h, h, -h]]),
            // Bottom (-Y)
            ([0.0, -1.0, 0.0], [[-h, -h, -h], [h, -h, -h], [h, -h, h], [-h, -h, h]]),
            // Right (+X)
            ([1.0, 0.0, 0.0], [[h, -h, h], [h, -h, -h], [h, h, -h], [h, h, h]]),
            // Left (-X)
            ([-1.0, 0.0, 0.0], [[-h, -h, -h], [-h, -h, h], [-h, h, h], [-h, h, -h]]),
        ];

        let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

        for (normal, corners) in faces {
            let base_idx = vertices.len() as u32;
            for (i, &corner) in corners.iter().enumerate() {
                vertices.push(Vertex::with_color(
                    [corner[0] + center[0], corner[1] + center[1], corner[2] + center[2]],
                    normal,
                    uvs[i],
                    [1.0, 0.5, 1.0, 1.0],
                ));
            }
            indices.extend_from_slice(&[
                base_idx, base_idx + 1, base_idx + 2,
                base_idx, base_idx + 2, base_idx + 3,
            ]);
        }

        Self::new("Cube", vertices, indices)
    }

    /// Creates a unit cube with distinct normals per face centered at origin.
    pub fn create_cube(size: f32) -> Self {
        Self::create_cube_at(size, [0.0, 0.0, 0.0])
    }

    /// Creates a stylized Anime Mannequin proxy with parametric anatomical proportions.
    pub fn create_mannequin_proxy_proportions(head_scale: f32, head_ratio: f32) -> Self {
        let total_h = 1.72;
        let hu = total_h / head_ratio.clamp(2.0, 9.0);
        let head_h = hu * head_scale.clamp(0.5, 2.0);
        let head_w = head_h * 0.85;
        let head_d = head_h * 0.9;
        let head_y = total_h - head_h * 0.5;

        let neck_h = hu * 0.35;
        let neck_y = head_y - head_h * 0.5 - neck_h * 0.5;

        let chest_h = hu * 1.35;
        let chest_y = neck_y - neck_h * 0.5 - chest_h * 0.5;

        let pelvis_h = hu * 0.9;
        let pelvis_y = chest_y - chest_h * 0.5 - pelvis_h * 0.5;

        let leg_span = (pelvis_y - pelvis_h * 0.5).max(0.2);
        let thigh_h = leg_span * 0.52;
        let calf_h = leg_span * 0.48;

        let thigh_y = pelvis_y - pelvis_h * 0.5 - thigh_h * 0.5;
        let calf_y = thigh_y - thigh_h * 0.5 - calf_h * 0.5;

        let arm_span = chest_h + pelvis_h * 0.6;
        let upper_arm_h = arm_span * 0.52;
        let forearm_h = arm_span * 0.48;

        let upper_arm_y = chest_y + chest_h * 0.35 - upper_arm_h * 0.5;
        let forearm_y = upper_arm_y - upper_arm_h * 0.5 - forearm_h * 0.5;

        let segments = [
            // Head
            ([0.0, head_y, 0.0], [head_w, head_h, head_d], [1.0, 0.48, 1.2, 1.0]),
            // Neck
            ([0.0, neck_y, 0.0], [hu * 0.28, neck_h, hu * 0.28], [0.9, 0.50, 1.0, 1.0]),
            // Chest
            ([0.0, chest_y, 0.0], [hu * 1.15, chest_h, hu * 0.75], [1.0, 0.50, 1.0, 1.0]),
            // Pelvis
            ([0.0, pelvis_y, 0.0], [hu * 1.05, pelvis_h, hu * 0.70], [1.0, 0.50, 1.0, 1.0]),
            // Left Upper Arm
            ([-hu * 0.85, upper_arm_y, 0.0], [hu * 0.28, upper_arm_h, hu * 0.28], [0.95, 0.50, 1.0, 0.8]),
            // Right Upper Arm
            ([hu * 0.85, upper_arm_y, 0.0], [hu * 0.28, upper_arm_h, hu * 0.28], [0.95, 0.50, 1.0, 0.8]),
            // Left Forearm
            ([-hu * 0.85, forearm_y, 0.0], [hu * 0.24, forearm_h, hu * 0.24], [0.95, 0.50, 1.0, 0.8]),
            // Right Forearm
            ([hu * 0.85, forearm_y, 0.0], [hu * 0.24, forearm_h, hu * 0.24], [0.95, 0.50, 1.0, 0.8]),
            // Left Thigh
            ([-hu * 0.38, thigh_y, 0.0], [hu * 0.42, thigh_h, hu * 0.42], [1.0, 0.50, 1.0, 0.9]),
            // Right Thigh
            ([hu * 0.38, thigh_y, 0.0], [hu * 0.42, thigh_h, hu * 0.42], [1.0, 0.50, 1.0, 0.9]),
            // Left Calf
            ([-hu * 0.38, calf_y, 0.0], [hu * 0.34, calf_h, hu * 0.34], [1.0, 0.50, 1.0, 0.9]),
            // Right Calf
            ([hu * 0.38, calf_y, 0.0], [hu * 0.34, calf_h, hu * 0.34], [1.0, 0.50, 1.0, 0.9]),
            // Ground Pedestal
            ([0.0, -0.02, 0.0], [1.2, 0.04, 1.2], [0.7, 0.50, 0.0, 0.3]),
        ];

        let mut combined_vertices = Vec::new();
        let mut combined_indices = Vec::new();
        let num_segments = segments.len();
        for (i, (center, dims, color)) in segments.into_iter().enumerate() {
            let sub_cube = Self::create_cube(1.0);
            let base_idx = combined_vertices.len() as u32;
            let is_pedestal = i == num_segments - 1;

            for v in sub_cube.vertices {
                let pos = [
                    center[0] + v.position[0] * dims[0],
                    center[1] + v.position[1] * dims[1],
                    center[2] + v.position[2] * dims[2],
                ];

                // For body limbs, compute smooth radial anime normals to support cel-shading terminators.
                // For the floor pedestal, preserve flat ground plane normals.
                let normal = if is_pedestal {
                    v.normal
                } else {
                    let radial = glam::Vec3::new(
                        v.position[0],
                        v.position[1] * 0.25,
                        v.position[2],
                    ).normalize();
                    [radial.x, radial.y, radial.z]
                };

                combined_vertices.push(Vertex::with_color(pos, normal, v.uv, color));
            }

            for idx in sub_cube.indices {
                combined_indices.push(base_idx + idx);
            }
        }

        Self::new("AnimeMannequinProxy", combined_vertices, combined_indices)
    }

    /// Creates a stylized Anime Mannequin proxy with default 6.5 head canon proportions.
    pub fn create_mannequin_proxy() -> Self {
        Self::create_mannequin_proxy_proportions(1.0, 6.5)
    }

    /// Creates a smooth UV sphere with center offset.
    pub fn create_uv_sphere_at(radius: f32, rings: u32, sectors: u32, center: [f32; 3]) -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for r in 0..=rings {
            let v = r as f32 / rings as f32;
            let theta = v * std::f32::consts::PI;

            for s in 0..=sectors {
                let u = s as f32 / sectors as f32;
                let phi = u * 2.0 * std::f32::consts::PI;

                let x = theta.sin() * phi.cos();
                let y = theta.cos();
                let z = theta.sin() * phi.sin();

                vertices.push(Vertex::with_color(
                    [x * radius + center[0], y * radius + center[1], z * radius + center[2]],
                    [x, y, z],
                    [u, v],
                    [1.0, 0.5, 1.0, 1.0],
                ));
            }
        }

        for r in 0..rings {
            for s in 0..sectors {
                let first = r * (sectors + 1) + s;
                let second = first + sectors + 1;

                indices.push(first);
                indices.push(first + 1);
                indices.push(second);

                indices.push(second);
                indices.push(first + 1);
                indices.push(second + 1);
            }
        }

        Self::new("UvSphere", vertices, indices)
    }

    /// Creates a smooth UV sphere centered at origin.
    pub fn create_uv_sphere(radius: f32, rings: u32, sectors: u32) -> Self {
        Self::create_uv_sphere_at(radius, rings, sectors, [0.0, 0.0, 0.0])
    }

    /// Serializes this mesh to a fully compliant Khronos glTF 2.0 Binary container (.glb).
    pub fn to_glb_bytes(&self) -> Result<Vec<u8>, GltfMeshError> {
        let mut bin_data: Vec<u8> = Vec::new();
        let mut buffer_views = Vec::new();
        let mut accessors = Vec::new();

        let count = self.vertices.len();

        // Helper to push aligned buffer view
        let mut push_buffer_view = |data: &[u8], target: Option<u32>| -> usize {
            while !bin_data.len().is_multiple_of(4) {
                bin_data.push(0);
            }
            let byte_offset = bin_data.len();
            let byte_length = data.len();
            bin_data.extend_from_slice(data);

            let idx = buffer_views.len();
            // P1-01: montado como objeto desde o início — não existe caminho em
            // que `as_object_mut` devolva `None` (e o closure continua infalível).
            let mut bv = serde_json::Map::new();
            bv.insert("buffer".into(), serde_json::json!(0));
            bv.insert("byteOffset".into(), serde_json::json!(byte_offset));
            bv.insert("byteLength".into(), serde_json::json!(byte_length));
            if let Some(t) = target {
                bv.insert("target".into(), serde_json::json!(t));
            }
            buffer_views.push(serde_json::Value::Object(bv));
            idx
        };

        // 1. POSITION (Float32x3)
        let (min_p, max_p) = if count > 0 {
            let mut min_p = [f32::INFINITY; 3];
            let mut max_p = [f32::NEG_INFINITY; 3];
            for v in &self.vertices {
                for i in 0..3 {
                    min_p[i] = min_p[i].min(v.position[i]);
                    max_p[i] = max_p[i].max(v.position[i]);
                }
            }
            (min_p, max_p)
        } else {
            ([0.0, 0.0, 0.0], [0.0, 0.0, 0.0])
        };

        let mut pos_bytes = Vec::with_capacity(count * 12);
        for v in &self.vertices {
            for i in 0..3 {
                pos_bytes.extend_from_slice(&v.position[i].to_le_bytes());
            }
        }
        let pos_bv = push_buffer_view(&pos_bytes, Some(34962)); // ARRAY_BUFFER
        accessors.push(serde_json::json!({
            "bufferView": pos_bv,
            "byteOffset": 0,
            "componentType": 5126, // FLOAT
            "count": count,
            "type": "VEC3",
            "min": min_p,
            "max": max_p,
        }));

        // 2. NORMAL (Float32x3)
        let mut norm_bytes = Vec::with_capacity(count * 12);
        for v in &self.vertices {
            for i in 0..3 {
                norm_bytes.extend_from_slice(&v.normal[i].to_le_bytes());
            }
        }
        let norm_bv = push_buffer_view(&norm_bytes, Some(34962));
        accessors.push(serde_json::json!({
            "bufferView": norm_bv,
            "byteOffset": 0,
            "componentType": 5126,
            "count": count,
            "type": "VEC3",
        }));

        // 3. TEXCOORD_0 (Float32x2)
        let mut uv_bytes = Vec::with_capacity(count * 8);
        for v in &self.vertices {
            uv_bytes.extend_from_slice(&v.uv[0].to_le_bytes());
            uv_bytes.extend_from_slice(&v.uv[1].to_le_bytes());
        }
        let uv_bv = push_buffer_view(&uv_bytes, Some(34962));
        accessors.push(serde_json::json!({
            "bufferView": uv_bv,
            "byteOffset": 0,
            "componentType": 5126,
            "count": count,
            "type": "VEC2",
        }));

        // 4. _ANIGO_COLOR (Float32x4)
        let mut col_bytes = Vec::with_capacity(count * 16);
        for v in &self.vertices {
            for i in 0..4 {
                col_bytes.extend_from_slice(&v.color[i].to_le_bytes());
            }
        }
        let col_bv = push_buffer_view(&col_bytes, Some(34962));
        accessors.push(serde_json::json!({
            "bufferView": col_bv,
            "byteOffset": 0,
            "componentType": 5126,
            "count": count,
            "type": "VEC4",
        }));

        // 5. JOINTS_0 (Uint16x4)
        let mut joints_bytes = Vec::with_capacity(count * 8);
        for v in &self.vertices {
            for i in 0..4 {
                joints_bytes.extend_from_slice(&v.joints[i].to_le_bytes());
            }
        }
        let joints_bv = push_buffer_view(&joints_bytes, Some(34962));
        accessors.push(serde_json::json!({
            "bufferView": joints_bv,
            "byteOffset": 0,
            "componentType": 5123, // UNSIGNED_SHORT
            "count": count,
            "type": "VEC4",
        }));

        // 6. WEIGHTS_0 (Float32x4)
        let mut weights_bytes = Vec::with_capacity(count * 16);
        for v in &self.vertices {
            for i in 0..4 {
                weights_bytes.extend_from_slice(&v.weights[i].to_le_bytes());
            }
        }
        let weights_bv = push_buffer_view(&weights_bytes, Some(34962));
        accessors.push(serde_json::json!({
            "bufferView": weights_bv,
            "byteOffset": 0,
            "componentType": 5126,
            "count": count,
            "type": "VEC4",
        }));

        // Custom attributes
        let mut primitive_attrs = serde_json::json!({
            "POSITION": 0,
            "NORMAL": 1,
            "TEXCOORD_0": 2,
            "_ANIGO_COLOR": 3,
            "JOINTS_0": 4,
            "WEIGHTS_0": 5,
        });

        for (name, attr) in &self.custom_attributes {
            let attr_bv = push_buffer_view(&attr.data, Some(34962));
            let acc_idx = accessors.len();
            accessors.push(serde_json::json!({
                "bufferView": attr_bv,
                "byteOffset": 0,
                "componentType": attr.component_type,
                "count": attr.count,
                "type": attr.attribute_type,
            }));
            // P1-01: `unwrap` removido; um atributo customizado sem objeto
            // `attributes` vira erro explícito (o GLB não é gravado errado).
            let attrs_object = primitive_attrs.as_object_mut().ok_or_else(|| {
                GltfMeshError::InvalidAccessor(
                    "bloco `attributes` da primitive não é um objeto JSON".into(),
                )
            })?;
            attrs_object.insert(name.clone(), serde_json::json!(acc_idx));
        }

        // Indices (Uint32 scalar)
        let ind_acc_idx = if !self.indices.is_empty() {
            let mut ind_bytes = Vec::with_capacity(self.indices.len() * 4);
            for &idx in &self.indices {
                ind_bytes.extend_from_slice(&idx.to_le_bytes());
            }
            let ind_bv = push_buffer_view(&ind_bytes, Some(34963)); // ELEMENT_ARRAY_BUFFER
            let idx = accessors.len();
            accessors.push(serde_json::json!({
                "bufferView": ind_bv,
                "byteOffset": 0,
                "componentType": 5125, // UNSIGNED_INT
                "count": self.indices.len(),
                "type": "SCALAR",
            }));
            Some(idx)
        } else {
            None
        };

        while !bin_data.len().is_multiple_of(4) {
            bin_data.push(0);
        }

        let mut primitive_json = serde_json::json!({
            "attributes": primitive_attrs,
            "mode": 4, // TRIANGLES
        });
        if let Some(idx) = ind_acc_idx {
            primitive_json["indices"] = serde_json::json!(idx);
        }

        let gltf_json = serde_json::json!({
            "asset": {
                "version": "2.0",
                "generator": "ANIGO Anime Engine 0.1.0 (Gold Standard)",
            },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0, "name": self.name }],
            "meshes": [{
                "name": self.name,
                "primitives": [primitive_json],
            }],
            "accessors": accessors,
            "bufferViews": buffer_views,
            "buffers": [{ "byteLength": bin_data.len() }],
        });

        let mut json_bytes = serde_json::to_vec(&gltf_json)?;
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }

        let total_len = 12 + 8 + json_bytes.len() + 8 + bin_data.len();
        let mut glb = Vec::with_capacity(total_len);

        // 12-byte GLB Header
        glb.extend_from_slice(&0x46546C67u32.to_le_bytes());
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total_len as u32).to_le_bytes());

        // Chunk 0: JSON (0x4E4F534A)
        glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        glb.extend_from_slice(&json_bytes);

        // Chunk 1: BIN (0x004E4942)
        glb.extend_from_slice(&(bin_data.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E4942u32.to_le_bytes());
        glb.extend_from_slice(&bin_data);

        Ok(glb)
    }

    /// Serializes and writes this mesh as a glTF 2.0 Binary (.glb) file to disk.
    pub fn to_glb_file(&self, path: impl AsRef<Path>) -> Result<(), GltfMeshError> {
        let bytes = self.to_glb_bytes()?;
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Parses and loads a 3D mesh from glTF 2.0 Binary container (.glb) bytes.
    pub fn from_glb_bytes(bytes: &[u8]) -> Result<Self, GltfMeshError> {
        if bytes.len() < 12 {
            return Err(GltfMeshError::UnexpectedEof);
        }

        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if magic != 0x46546C67 {
            return Err(GltfMeshError::InvalidMagic(magic));
        }

        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != 2 {
            return Err(GltfMeshError::UnsupportedVersion(version));
        }

        let file_length = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        if bytes.len() < file_length {
            return Err(GltfMeshError::UnexpectedEof);
        }

        let mut offset = 12;
        let mut json_data: Option<&[u8]> = None;
        let mut bin_data: Option<&[u8]> = None;

        while offset + 8 <= file_length {
            let chunk_len = u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) as usize;
            let chunk_type = u32::from_le_bytes([bytes[offset + 4], bytes[offset + 5], bytes[offset + 6], bytes[offset + 7]]);
            offset += 8;

            if offset + chunk_len > bytes.len() {
                return Err(GltfMeshError::UnexpectedEof);
            }

            let chunk_slice = &bytes[offset..offset + chunk_len];
            if chunk_type == 0x4E4F534A {
                json_data = Some(chunk_slice);
            } else if chunk_type == 0x004E4942 {
                bin_data = Some(chunk_slice);
            }

            offset += (chunk_len + 3) & !3;
        }

        let json_bytes = json_data.ok_or(GltfMeshError::MissingJsonChunk)?;
        let bin_bytes = bin_data.ok_or(GltfMeshError::MissingBinaryChunk)?;

        let gltf: serde_json::Value = serde_json::from_slice(json_bytes)?;

        let mesh_obj = gltf["meshes"].get(0).ok_or_else(|| GltfMeshError::InvalidAccessor("No meshes in glTF".into()))?;
        let mesh_name = mesh_obj.get("name").and_then(|v| v.as_str()).unwrap_or("UnnamedMesh").to_string();

        let prim = mesh_obj["primitives"].get(0).ok_or_else(|| GltfMeshError::InvalidAccessor("No primitives in mesh".into()))?;
        let attrs = prim["attributes"].as_object().ok_or_else(|| GltfMeshError::InvalidAccessor("Primitive missing attributes".into()))?;

        let get_accessor_data = |acc_idx: usize| -> Result<(&[u8], usize, u32, &str), GltfMeshError> {
            let acc = gltf["accessors"].get(acc_idx).ok_or_else(|| GltfMeshError::InvalidAccessor(format!("Accessor {} missing", acc_idx)))?;
            let bv_idx = acc["bufferView"].as_u64().ok_or_else(|| GltfMeshError::InvalidAccessor(format!("Accessor {} missing bufferView", acc_idx)))? as usize;
            let bv = gltf["bufferViews"].get(bv_idx).ok_or_else(|| GltfMeshError::InvalidBufferView(format!("BufferView {} missing", bv_idx)))?;

            let bv_offset = bv.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let bv_len = bv["byteLength"].as_u64().ok_or_else(|| GltfMeshError::InvalidBufferView(format!("BufferView {} missing byteLength", bv_idx)))? as usize;

            let acc_offset = acc.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let count = acc["count"].as_u64().ok_or_else(|| GltfMeshError::InvalidAccessor(format!("Accessor {} missing count", acc_idx)))? as usize;
            let comp_type = acc["componentType"].as_u64().ok_or_else(|| GltfMeshError::InvalidAccessor(format!("Accessor {} missing componentType", acc_idx)))? as u32;
            let type_str = acc["type"].as_str().unwrap_or("SCALAR");

            let comp_size = get_component_byte_size(comp_type)?;
            let num_comps = get_type_component_count(type_str)?;
            let elem_size = comp_size * num_comps;
            let total_bytes = count.checked_mul(elem_size).ok_or_else(|| GltfMeshError::InvalidAccessor(format!("Accessor {} size overflow", acc_idx)))?;

            let start = bv_offset.checked_add(acc_offset).ok_or_else(|| GltfMeshError::InvalidBufferView("BufferView offset overflow".into()))?;
            let end = start.checked_add(total_bytes).ok_or_else(|| GltfMeshError::InvalidBufferView("Accessor range overflow".into()))?;

            if end > bin_bytes.len() || acc_offset + total_bytes > bv_len {
                return Err(GltfMeshError::InvalidAccessor(format!(
                    "Accessor {} data slice out of bounds: range {}..{} exceeds bufferView len {} or bin len {}",
                    acc_idx, start, end, bv_len, bin_bytes.len()
                )));
            }

            Ok((&bin_bytes[start..end], count, comp_type, type_str))
        };

        let pos_acc_idx = attrs.get("POSITION").and_then(|v| v.as_u64()).ok_or(GltfMeshError::MissingPosition)? as usize;
        let (pos_slice, vert_count, comp_type, type_str) = get_accessor_data(pos_acc_idx)?;
        if comp_type != 5126 || type_str != "VEC3" {
            return Err(GltfMeshError::InvalidAccessor(format!("POSITION accessor must be FLOAT VEC3, got {} {}", comp_type, type_str)));
        }
        let mut positions = Vec::with_capacity(vert_count);
        for i in 0..vert_count {
            let b = &pos_slice[i * 12..(i + 1) * 12];
            positions.push([
                f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                f32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                f32::from_le_bytes([b[8], b[9], b[10], b[11]]),
            ]);
        }

        let mut normals = vec![[0.0, 1.0, 0.0]; vert_count];
        if let Some(norm_val) = attrs.get("NORMAL").and_then(|v| v.as_u64()) {
            let (norm_slice, count, comp_type, type_str) = get_accessor_data(norm_val as usize)?;
            if comp_type == 5126 && type_str == "VEC3" {
                for i in 0..count.min(vert_count) {
                    let b = &norm_slice[i * 12..(i + 1) * 12];
                    normals[i] = [
                        f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                        f32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                        f32::from_le_bytes([b[8], b[9], b[10], b[11]]),
                    ];
                }
            }
        }

        let mut uvs = vec![[0.0, 0.0]; vert_count];
        if let Some(uv_val) = attrs.get("TEXCOORD_0").and_then(|v| v.as_u64()) {
            let (uv_slice, count, comp_type, _) = get_accessor_data(uv_val as usize)?;
            let n = count.min(vert_count);
            if comp_type == 5126 {
                for i in 0..n {
                    let b = &uv_slice[i * 8..(i + 1) * 8];
                    uvs[i] = [
                        f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                        f32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                    ];
                }
            } else if comp_type == 5123 {
                for i in 0..n {
                    let b = &uv_slice[i * 4..(i + 1) * 4];
                    uvs[i] = [
                        u16::from_le_bytes([b[0], b[1]]) as f32 / 65535.0,
                        u16::from_le_bytes([b[2], b[3]]) as f32 / 65535.0,
                    ];
                }
            } else if comp_type == 5121 {
                for i in 0..n {
                    let b = &uv_slice[i * 2..(i + 1) * 2];
                    uvs[i] = [
                        b[0] as f32 / 255.0,
                        b[1] as f32 / 255.0,
                    ];
                }
            }
        }

        let mut colors = vec![[1.0, 0.5, 1.0, 1.0]; vert_count];
        let color_attr = attrs.get("_ANIGO_COLOR").or_else(|| attrs.get("COLOR_0"));
        if let Some(col_val) = color_attr.and_then(|v| v.as_u64()) {
            let (col_slice, count, comp_type, type_str) = get_accessor_data(col_val as usize)?;
            let n = count.min(vert_count);
            let is_vec4 = type_str == "VEC4";
            if comp_type == 5126 {
                let stride = if is_vec4 { 16 } else { 12 };
                for i in 0..n {
                    let b = &col_slice[i * stride..(i + 1) * stride];
                    let r = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                    let g = f32::from_le_bytes([b[4], b[5], b[6], b[7]]);
                    let bl = f32::from_le_bytes([b[8], b[9], b[10], b[11]]);
                    let a = if is_vec4 { f32::from_le_bytes([b[12], b[13], b[14], b[15]]) } else { 1.0 };
                    colors[i] = [r, g, bl, a];
                }
            } else if comp_type == 5121 {
                let stride = if is_vec4 { 4 } else { 3 };
                for i in 0..n {
                    let b = &col_slice[i * stride..(i + 1) * stride];
                    let r = b[0] as f32 / 255.0;
                    let g = b[1] as f32 / 255.0;
                    let bl = b[2] as f32 / 255.0;
                    let a = if is_vec4 { b[3] as f32 / 255.0 } else { 1.0 };
                    colors[i] = [r, g, bl, a];
                }
            } else if comp_type == 5123 {
                let stride = if is_vec4 { 8 } else { 6 };
                for i in 0..n {
                    let b = &col_slice[i * stride..(i + 1) * stride];
                    let r = u16::from_le_bytes([b[0], b[1]]) as f32 / 65535.0;
                    let g = u16::from_le_bytes([b[2], b[3]]) as f32 / 65535.0;
                    let bl = u16::from_le_bytes([b[4], b[5]]) as f32 / 65535.0;
                    let a = if is_vec4 { u16::from_le_bytes([b[6], b[7]]) as f32 / 65535.0 } else { 1.0 };
                    colors[i] = [r, g, bl, a];
                }
            }
        }

        let mut joints = vec![[0u16, 0, 0, 0]; vert_count];
        if let Some(joints_val) = attrs.get("JOINTS_0").and_then(|v| v.as_u64()) {
            let (joints_slice, count, comp_type, _) = get_accessor_data(joints_val as usize)?;
            let n = count.min(vert_count);
            if comp_type == 5123 {
                for i in 0..n {
                    let b = &joints_slice[i * 8..(i + 1) * 8];
                    joints[i] = [
                        u16::from_le_bytes([b[0], b[1]]),
                        u16::from_le_bytes([b[2], b[3]]),
                        u16::from_le_bytes([b[4], b[5]]),
                        u16::from_le_bytes([b[6], b[7]]),
                    ];
                }
            } else if comp_type == 5121 {
                for i in 0..n {
                    let b = &joints_slice[i * 4..(i + 1) * 4];
                    joints[i] = [b[0] as u16, b[1] as u16, b[2] as u16, b[3] as u16];
                }
            }
        }

        let mut weights = vec![[1.0f32, 0.0, 0.0, 0.0]; vert_count];
        if let Some(weights_val) = attrs.get("WEIGHTS_0").and_then(|v| v.as_u64()) {
            let (w_slice, count, comp_type, _) = get_accessor_data(weights_val as usize)?;
            let n = count.min(vert_count);
            if comp_type == 5126 {
                for i in 0..n {
                    let b = &w_slice[i * 16..(i + 1) * 16];
                    weights[i] = [
                        f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                        f32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                        f32::from_le_bytes([b[8], b[9], b[10], b[11]]),
                        f32::from_le_bytes([b[12], b[13], b[14], b[15]]),
                    ];
                }
            } else if comp_type == 5121 {
                for i in 0..n {
                    let b = &w_slice[i * 4..(i + 1) * 4];
                    weights[i] = [
                        b[0] as f32 / 255.0,
                        b[1] as f32 / 255.0,
                        b[2] as f32 / 255.0,
                        b[3] as f32 / 255.0,
                    ];
                }
            } else if comp_type == 5123 {
                for i in 0..n {
                    let b = &w_slice[i * 8..(i + 1) * 8];
                    weights[i] = [
                        u16::from_le_bytes([b[0], b[1]]) as f32 / 65535.0,
                        u16::from_le_bytes([b[2], b[3]]) as f32 / 65535.0,
                        u16::from_le_bytes([b[4], b[5]]) as f32 / 65535.0,
                        u16::from_le_bytes([b[6], b[7]]) as f32 / 65535.0,
                    ];
                }
            }
        }

        let mut custom_attributes = HashMap::new();
        for (name, val) in attrs {
            if name != "POSITION" && name != "NORMAL" && name != "TEXCOORD_0" && name != "_ANIGO_COLOR" && name != "COLOR_0" && name != "JOINTS_0" && name != "WEIGHTS_0" {
                if let Some(acc_idx) = val.as_u64() {
                    if let Ok((slice, count, comp_type, type_str)) = get_accessor_data(acc_idx as usize) {
                        custom_attributes.insert(name.clone(), CustomAttribute {
                            name: name.clone(),
                            component_type: comp_type,
                            attribute_type: type_str.to_string(),
                            count,
                            data: slice.to_vec(),
                        });
                    }
                }
            }
        }

        let mut indices = Vec::new();
        if let Some(ind_val) = prim.get("indices").and_then(|v| v.as_u64()) {
            let (ind_slice, ind_count, comp_type, _) = get_accessor_data(ind_val as usize)?;
            if comp_type == 5125 {
                for i in 0..ind_count {
                    let b = &ind_slice[i * 4..(i + 1) * 4];
                    indices.push(u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                }
            } else if comp_type == 5123 {
                for i in 0..ind_count {
                    let b = &ind_slice[i * 2..(i + 1) * 2];
                    indices.push(u16::from_le_bytes([b[0], b[1]]) as u32);
                }
            } else if comp_type == 5121 {
                for &b in &ind_slice[..ind_count] {
                    indices.push(b as u32);
                }
            }
        } else {
            indices = (0..vert_count as u32).collect();
        }

        let mut vertices = Vec::with_capacity(vert_count);
        for i in 0..vert_count {
            vertices.push(Vertex::with_skinning(
                positions[i],
                normals[i],
                uvs[i],
                colors[i],
                joints[i],
                weights[i],
            ));
        }

        let mut mesh = Self::new(mesh_name, vertices, indices);
        mesh.custom_attributes = custom_attributes;
        Ok(mesh)
    }

    /// Loads a glTF 2.0 Binary container (.glb) from a filesystem path.
    pub fn from_glb_file(path: impl AsRef<Path>) -> Result<Self, GltfMeshError> {
        let bytes = std::fs::read(path)?;
        Self::from_glb_bytes(&bytes)
    }

    /// Validates strict 1:1 topological isomorphism against another mesh.
    ///
    /// Verifies:
    /// 1. Identical vertex count ($N$)
    /// 2. Identical index count and exact face triangle topology
    /// 3. Identical UV coordinates per vertex (within 1e-4 tolerance)
    /// 4. Identical joint skinning indices
    /// 5. Identical joint skinning weights (within 1e-4 tolerance)
    pub fn validate_isomorphism(&self, other: &Mesh) -> Result<(), GltfMeshError> {
        if self.vertices.len() != other.vertices.len() {
            return Err(GltfMeshError::IsomorphismMismatch(format!(
                "Vertex count mismatch: self has {} vertices, other has {}",
                self.vertices.len(),
                other.vertices.len()
            )));
        }

        if self.indices.len() != other.indices.len() {
            return Err(GltfMeshError::IsomorphismMismatch(format!(
                "Index count mismatch: self has {} indices, other has {}",
                self.indices.len(),
                other.indices.len()
            )));
        }

        for (i, (&i_self, &i_other)) in self.indices.iter().zip(other.indices.iter()).enumerate() {
            if i_self != i_other {
                return Err(GltfMeshError::IsomorphismMismatch(format!(
                    "Triangle index mismatch at entry {}: {} vs {}",
                    i, i_self, i_other
                )));
            }
        }

        for (v_idx, (v_self, v_other)) in self.vertices.iter().zip(other.vertices.iter()).enumerate() {
            let du = (v_self.uv[0] - v_other.uv[0]).abs();
            let dv = (v_self.uv[1] - v_other.uv[1]).abs();
            if du > 1e-4 || dv > 1e-4 {
                return Err(GltfMeshError::IsomorphismMismatch(format!(
                    "UV coordinate mismatch at vertex {}: {:?} vs {:?}",
                    v_idx, v_self.uv, v_other.uv
                )));
            }

            if v_self.joints != v_other.joints {
                return Err(GltfMeshError::IsomorphismMismatch(format!(
                    "Joint indices mismatch at vertex {}: {:?} vs {:?}",
                    v_idx, v_self.joints, v_other.joints
                )));
            }

            for j in 0..4 {
                if (v_self.weights[j] - v_other.weights[j]).abs() > 1e-4 {
                    return Err(GltfMeshError::IsomorphismMismatch(format!(
                        "Joint weight {} mismatch at vertex {}: {} vs {}",
                        j, v_idx, v_self.weights[j], v_other.weights[j]
                    )));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vertex_pod_and_zeroable() {
        assert_eq!(std::mem::size_of::<Vertex>(), 72);
        assert_eq!(std::mem::align_of::<Vertex>(), 4);

        let v = Vertex::with_skinning(
            [1.0, 2.0, 3.0],
            [0.0, 1.0, 0.0],
            [0.5, 0.5],
            [1.0, 0.5, 1.0, 1.0],
            [4, 3, 0, 0],
            [0.8, 0.2, 0.0, 0.0],
        );

        let bytes: &[u8] = bytemuck::bytes_of(&v);
        assert_eq!(bytes.len(), 72);

        let v_back: &Vertex = bytemuck::from_bytes(bytes);
        assert_eq!(v, *v_back);
    }

    #[test]
    fn test_create_cube() {
        let cube = Mesh::create_cube(2.0);
        assert_eq!(cube.name, "Cube");
        assert_eq!(cube.vertices.len(), 24); // 6 faces * 4 vertices
        assert_eq!(cube.indices.len(), 36);  // 6 faces * 2 triangles * 3 indices
        assert_eq!(cube.vertices[0].joints, [0, 0, 0, 0]);
        assert_eq!(cube.vertices[0].weights, [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_create_uv_sphere() {
        let rings = 8;
        let sectors = 16;
        let sphere = Mesh::create_uv_sphere(1.0, rings, sectors);
        assert_eq!(sphere.name, "UvSphere");
        assert_eq!(sphere.vertices.len(), ((rings + 1) * (sectors + 1)) as usize);
        assert_eq!(sphere.indices.len(), (rings * sectors * 6) as usize);
    }

    #[test]
    fn test_uv_sphere_outward_normals() {
        let sphere = Mesh::create_uv_sphere(1.0, 16, 32);
        for chunk in sphere.indices.chunks_exact(3) {
            let v0 = sphere.vertices[chunk[0] as usize].position;
            let v1 = sphere.vertices[chunk[1] as usize].position;
            let v2 = sphere.vertices[chunk[2] as usize].position;

            let edge1 = glam::Vec3::from_array(v1) - glam::Vec3::from_array(v0);
            let edge2 = glam::Vec3::from_array(v2) - glam::Vec3::from_array(v0);
            let face_normal = edge1.cross(edge2);

            let center_to_face = glam::Vec3::from_array(v0);
            if face_normal.length_squared() > 1e-6 && center_to_face.length_squared() > 1e-6 {
                let dot = face_normal.dot(center_to_face);
                assert!(dot > 0.0, "Triangle winding must be CCW pointing outward, got dot = {}", dot);
            }
        }
    }

    #[test]
    fn test_create_mannequin_proxy() {
        let mannequin = Mesh::create_mannequin_proxy();
        assert_eq!(mannequin.name, "AnimeMannequinProxy");
        // 13 segments of cubes
        assert_eq!(mannequin.vertices.len(), 13 * 24);
        assert_eq!(mannequin.indices.len(), 13 * 36);
    }

    #[test]
    fn test_glb_serialization_roundtrip() {
        // AnimeIAI: o manequim proxy no lugar da base canônica (não vendorizada)
        let original = Mesh::create_mannequin_proxy();
        let glb_bytes = original.to_glb_bytes().expect("Failed to serialize GLB");
        assert!(glb_bytes.len() > 1000);

        let restored = Mesh::from_glb_bytes(&glb_bytes).expect("Failed to parse GLB");
        assert_eq!(restored.name, original.name);
        assert_eq!(restored.vertices.len(), original.vertices.len());
        assert_eq!(restored.indices.len(), original.indices.len());

        original.validate_isomorphism(&restored).expect("Restored GLB mesh must be isomorphic to original");
    }

    #[test]
    fn test_custom_attributes_support() {
        let custom_data = vec![42u8; 16];
        let mut mesh = Mesh::create_cube(1.0);
        mesh.custom_attributes.insert("_ANIGO_TEST_ATTR".into(), CustomAttribute {
            name: "_ANIGO_TEST_ATTR".into(),
            component_type: 5120,
            attribute_type: "VEC4".into(),
            count: 4,
            data: custom_data.clone(),
        });

        let glb = mesh.to_glb_bytes().unwrap();
        let parsed = Mesh::from_glb_bytes(&glb).unwrap();

        assert!(parsed.custom_attributes.contains_key("_ANIGO_TEST_ATTR"));
        let attr = &parsed.custom_attributes["_ANIGO_TEST_ATTR"];
        assert_eq!(attr.data, custom_data);
    }

    #[test]
    fn test_from_glb_bytes_truncated_accessor_error() {
        let original = Mesh::create_cube(1.0);
        let mut glb_bytes = original.to_glb_bytes().unwrap();

        // Corrupt chunk length or truncate BIN data to simulate malformed GLB
        glb_bytes.truncate(glb_bytes.len() - 50);

        let result = Mesh::from_glb_bytes(&glb_bytes);
        assert!(result.is_err(), "Truncated GLB must return error, not panic");
    }

    #[test]
    fn test_from_glb_bytes_non_indexed() {
        let v0 = Vertex::new([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0]);
        let v1 = Vertex::new([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0]);
        let v2 = Vertex::new([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0]);
        let mesh = Mesh::new("Triangle", vec![v0, v1, v2], vec![]);

        let glb = mesh.to_glb_bytes().unwrap();
        let loaded = Mesh::from_glb_bytes(&glb).unwrap();

        assert_eq!(loaded.vertices.len(), 3);
        assert_eq!(loaded.indices, vec![0, 1, 2]);
    }

}
