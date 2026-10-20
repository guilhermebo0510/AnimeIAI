//! Importação de modelos 3D para o [`Model`] normalizado: glTF 2.0 (`.gltf` com buffers externos ou data-URI, `.glb`),
//! vários nós e várias malhas, com as transformações dos nós aplicadas; e OBJ (com `.mtl` para a cor `Kd`).
//!
//! A pele (skin) do glTF é lida (S24 3D, fase 9): o esqueleto da primeira pele e os pesos `JOINTS_0`/`WEIGHTS_0` vão para
//! [`Skin`], com a malha já levada à pose de repouso, e o modelo fica posável por nome de osso (skinning na CPU,
//! [`crate::pose`]). Animações e morphs são ignorados com aviso; texturas não são lidas (cor base do material); VRM é
//! recusado com "em breve" (a permissão de uso do VRM precisa ser mostrada e guardada antes, ver `S24-pesquisa/3d.md`
//! b.4). Erros nunca são silenciosos: arquivo ruim vira [`ImportError`].

use crate::gltf::{self, GltfError, ParsedGltf};
use crate::model::{LicenseInfo, Model, ModelInfo, Part, PartSkin, Skin, SkinBone, smooth_normals};
use base64::Engine;
use glam::{Mat3, Mat4, Quat, Vec3};
use serde_json::Value;
use std::path::Path;

/// Teto de triângulos por modelo (o renderer de referência e o gizmo continuam fluidos abaixo disso).
pub const MAX_TRIANGLES: usize = 500_000;
/// Teto do arquivo de origem.
pub const MAX_FILE_BYTES: u64 = 512 << 20;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct ImportError(pub String);

impl From<GltfError> for ImportError {
    fn from(e: GltfError) -> Self {
        ImportError(e.to_string())
    }
}

fn err<T>(m: impl Into<String>) -> Result<T, ImportError> {
    Err(ImportError(m.into()))
}

/// Extensões aceitas pelo importador (as outras dão erro claro).
pub const EXTENSIONS: [&str; 3] = ["glb", "gltf", "obj"];

/// Lê e importa um arquivo do disco (buffers externos do `.gltf` e o `.mtl` do OBJ são procurados na mesma pasta).
pub fn import_path(path: &Path) -> Result<Model, ImportError> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "vrm" {
        return err("VRM: suporte em breve (a v1 importa .glb, .gltf e .obj; exporte o modelo como .glb sem a extensão VRM)");
    }
    if !EXTENSIONS.contains(&ext.as_str()) {
        return err(format!("formato .{ext} não suportado (use .glb, .gltf ou .obj)"));
    }
    let meta = std::fs::metadata(path).map_err(|e| ImportError(format!("{}: {e}", path.display())))?;
    if meta.len() > MAX_FILE_BYTES {
        return err(format!("arquivo de {} MB acima do limite de {} MB", meta.len() >> 20, MAX_FILE_BYTES >> 20));
    }
    let bytes = std::fs::read(path).map_err(|e| ImportError(format!("{}: {e}", path.display())))?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let resolver = move |uri: &str| -> Option<Vec<u8>> {
        let rel = Path::new(uri);
        // confinado à pasta do arquivo: nada de caminho absoluto nem '..'
        if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return None;
        }
        let decoded = percent_decode(uri);
        std::fs::read(dir.join(decoded)).ok()
    };
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    import_bytes(&bytes, &ext, &name, &resolver)
}

/// Importa a partir dos bytes. `resolver` devolve arquivos vizinhos por caminho relativo (buffers do `.gltf`, `.mtl`).
pub fn import_bytes(bytes: &[u8], ext: &str, file_name: &str, resolver: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model, ImportError> {
    let mut model = match ext {
        "glb" => {
            let (json, bin) = gltf::parse_glb(bytes)?;
            from_gltf_json(json, Some(bin), resolver)?
        }
        "gltf" => {
            let json: Value = serde_json::from_slice(bytes).map_err(|e| ImportError(format!("JSON do .gltf inválido: {e}")))?;
            from_gltf_json(json, None, resolver)?
        }
        "obj" => from_obj(bytes, resolver)?,
        "vrm" => return err("VRM: suporte em breve (a v1 importa .glb, .gltf e .obj)"),
        other => return err(format!("formato .{other} não suportado (use .glb, .gltf ou .obj)")),
    };
    if model.info.format.is_empty() {
        model.info.format = ext.to_string();
    }
    if model.info.file_name.is_empty() {
        model.info.file_name = file_name.to_string();
    }
    finish(model)
}

/// Limites e contagens finais.
fn finish(mut model: Model) -> Result<Model, ImportError> {
    if let Some(skin) = model.skin.as_mut() {
        let keep: Vec<bool> = model.parts.iter().map(|p| p.triangles() > 0).collect();
        let mut i = 0;
        skin.parts.retain(|_| {
            i += 1;
            keep[i - 1]
        });
    }
    model.parts.retain(|p| p.triangles() > 0);
    if let Some(skin) = &model.skin {
        skin.validate(&model.parts).map_err(|e| ImportError(format!("pele inválida: {e}")))?;
    }
    model.refresh_info();
    if model.parts.is_empty() {
        return err("o arquivo não tem nenhuma malha de triângulos");
    }
    if model.info.triangles > MAX_TRIANGLES {
        return err(format!(
            "o modelo tem {} triângulos; o limite da camada 3D é {} (reduza a malha no programa de origem)",
            model.info.triangles, MAX_TRIANGLES
        ));
    }
    let [lo, hi] = model.info.bbox;
    if !lo.iter().chain(hi.iter()).all(|v| v.is_finite()) {
        return err("o modelo tem coordenadas não finitas");
    }
    Ok(model)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn data_uri(uri: &str) -> Option<Result<Vec<u8>, ImportError>> {
    let rest = uri.strip_prefix("data:")?;
    let (head, data) = rest.split_once(',')?;
    if !head.ends_with(";base64") {
        return Some(err("data-URI sem base64 não é suportada"));
    }
    Some(base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|e| ImportError(format!("data-URI inválida: {e}"))))
}

// ------------------------------------------------------------------------------------------------------------ glTF

fn vrm_extension(json: &Value) -> Option<String> {
    let used = json.get("extensionsUsed").and_then(Value::as_array)?;
    used.iter().filter_map(Value::as_str).find(|e| *e == "VRM" || e.starts_with("VRMC_vrm")).map(String::from)
}

fn license_of(json: &Value) -> LicenseInfo {
    let s = |p: &str| json.pointer(p).and_then(Value::as_str).unwrap_or("").trim().to_string();
    LicenseInfo {
        title: s("/asset/extras/title"),
        author: s("/asset/extras/author"),
        license: s("/asset/extras/license"),
        copyright: s("/asset/copyright"),
        source: s("/asset/extras/source"),
    }
}

fn node_local(node: &Value) -> Mat4 {
    if let Some(m) = node.get("matrix").and_then(Value::as_array).filter(|m| m.len() == 16) {
        let mut a = [0.0f32; 16];
        for (i, v) in m.iter().enumerate() {
            a[i] = v.as_f64().unwrap_or(0.0) as f32;
        }
        return Mat4::from_cols_array(&a);
    }
    let v3 = |k: &str, d: [f32; 3]| -> Vec3 {
        node.get(k).and_then(Value::as_array).filter(|a| a.len() == 3).map(|a| Vec3::new(f(&a[0], d[0]), f(&a[1], d[1]), f(&a[2], d[2]))).unwrap_or(Vec3::from(d))
    };
    let t = v3("translation", [0.0; 3]);
    let s = v3("scale", [1.0; 3]);
    let r = node
        .get("rotation")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 4)
        .map(|a| Quat::from_xyzw(f(&a[0], 0.0), f(&a[1], 0.0), f(&a[2], 0.0), f(&a[3], 1.0)).normalize())
        .unwrap_or(Quat::IDENTITY);
    Mat4::from_scale_rotation_translation(s, r, t)
}

fn f(v: &Value, d: f32) -> f32 {
    v.as_f64().map(|x| x as f32).unwrap_or(d)
}

fn material_of(json: &Value, idx: Option<u64>) -> ([f32; 4], String) {
    let Some(m) = idx.and_then(|i| json.get("materials").and_then(Value::as_array).and_then(|a| a.get(i as usize))) else {
        return ([0.8, 0.8, 0.8, 1.0], "padrão".into());
    };
    let name = m.get("name").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("material {}", idx.unwrap_or(0)));
    let factor = m
        .pointer("/pbrMetallicRoughness/baseColorFactor")
        .or_else(|| m.pointer("/extensions/KHR_materials_pbrSpecularGlossiness/diffuseFactor"))
        .and_then(Value::as_array)
        .filter(|a| a.len() == 4)
        .map(|a| [f(&a[0], 1.0), f(&a[1], 1.0), f(&a[2], 1.0), f(&a[3], 1.0)])
        .unwrap_or([1.0, 1.0, 1.0, 1.0]);
    (factor.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 1.0 }), name)
}

fn from_gltf_json(json: Value, bin: Option<Vec<u8>>, resolver: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model, ImportError> {
    if let Some(ext) = vrm_extension(&json) {
        return err(format!("VRM ({ext}): suporte em breve; a v1 importa .glb, .gltf e .obj sem a extensão VRM"));
    }
    let parsed: ParsedGltf = gltf::parse_gltf(json, |i, uri| match uri {
        None => bin.clone().ok_or_else(|| GltfError::new(gltf::GltfErrorCode::MissingBin, format!("buffers[{i}] sem uri e sem chunk BIN"))),
        Some(u) => match data_uri(u) {
            Some(Ok(b)) => Ok(b),
            Some(Err(e)) => Err(GltfError::new(gltf::GltfErrorCode::MissingBuffer, e.0)),
            None => resolver(u).ok_or_else(|| GltfError::new(gltf::GltfErrorCode::MissingBuffer, format!("buffers[{i}]: arquivo '{u}' não encontrado ao lado do .gltf"))),
        },
    })?;
    let json = &parsed.json;
    let mut warnings = parsed.warnings.clone();
    let nodes = json.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
    // raízes: a cena padrão (ou a primeira); sem cenas, todo nó que não é filho de ninguém
    let scene_idx = json.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize;
    let roots: Vec<usize> = match json.get("scenes").and_then(Value::as_array).and_then(|s| s.get(scene_idx)) {
        Some(sc) => sc.get("nodes").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|x| x as usize).collect()).unwrap_or_default(),
        None => {
            let mut child = vec![false; nodes.len()];
            for n in &nodes {
                for c in n.get("children").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64) {
                    if let Some(x) = child.get_mut(c as usize) {
                        *x = true;
                    }
                }
            }
            (0..nodes.len()).filter(|i| !child[*i]).collect()
        }
    };
    // transformação global de cada nó (a pele precisa das juntas, que podem estar fora do caminho das malhas)
    let mut globals = vec![Mat4::IDENTITY; nodes.len()];
    let mut parent_of: Vec<Option<usize>> = vec![None; nodes.len()];
    {
        let mut st: Vec<(usize, Mat4)> = roots.iter().rev().map(|r| (*r, Mat4::IDENTITY)).collect();
        let mut seen = vec![false; nodes.len()];
        while let Some((ni, parent)) = st.pop() {
            let Some(node) = nodes.get(ni) else { continue };
            if std::mem::replace(&mut seen[ni], true) {
                continue;
            }
            globals[ni] = parent * node_local(node);
            for c in node.get("children").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64) {
                if (c as usize) < nodes.len() {
                    parent_of[c as usize] = Some(ni);
                    st.push((c as usize, globals[ni]));
                }
            }
        }
    }
    let skin_setup = match json.get("skins").and_then(Value::as_array).filter(|a| !a.is_empty()) {
        Some(skins) => {
            if skins.len() > 1 {
                warnings.push(format!("{} peles no arquivo: só a primeira é usada para posar", skins.len()));
            }
            Some(skin_setup(&parsed, &skins[0], &globals, &parent_of)?)
        }
        None => None,
    };
    let mut part_skins: Vec<PartSkin> = Vec::new();
    if json.get("animations").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) {
        warnings.push("animações do arquivo ignoradas (anime pela camada: transformação e câmera por chaves)".into());
    }
    let mut parts = Vec::new();
    let mut stack: Vec<(usize, Mat4)> = roots.into_iter().rev().map(|r| (r, Mat4::IDENTITY)).collect();
    let mut visits = 0usize;
    while let Some((ni, parent)) = stack.pop() {
        visits += 1;
        if visits > 1_000_000 {
            return err("hierarquia de nós grande demais");
        }
        let Some(node) = nodes.get(ni) else { continue };
        let world = parent * node_local(node);
        if let Some(mi) = node.get("mesh").and_then(Value::as_u64) {
            let mesh = &json["meshes"][mi as usize];
            let mesh_name = mesh.get("name").and_then(Value::as_str).or_else(|| node.get("name").and_then(Value::as_str)).map(String::from).unwrap_or_else(|| format!("malha {mi}"));
            let prims = mesh.get("primitives").and_then(Value::as_array).cloned().unwrap_or_default();
            // malha com pele (a da primeira pele): vértices no espaço de bind, levados ao repouso pelas juntas
            let skinned = node.get("skin").and_then(Value::as_u64) == Some(0);
            let sk = skin_setup.as_ref().filter(|_| skinned);
            for (pi, prim) in prims.iter().enumerate() {
                match primitive_part(&parsed, prim, &world, &mesh_name, pi, prims.len(), &mut warnings, sk)? {
                    Some((p, ps)) => {
                        let ps = ps.unwrap_or_else(|| rigid_part_skin(skin_setup.as_ref(), ni, &parent_of, p.positions.len()));
                        parts.push(p);
                        part_skins.push(ps);
                    }
                    None => continue,
                }
                let tris: usize = parts.iter().map(Part::triangles).sum();
                if tris > MAX_TRIANGLES {
                    return err(format!("o modelo passa de {MAX_TRIANGLES} triângulos (limite da camada 3D; reduza a malha no programa de origem)"));
                }
            }
        }
        for c in node.get("children").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect::<Vec<_>>().into_iter().rev() {
            stack.push((c as usize, world));
        }
    }
    // GLB normalizado do próprio AnimeIAI: as informações de origem voltam do `asset.extras.aiai`
    let mut info = ModelInfo { license: license_of(json), warnings, ..Default::default() };
    if let Some(a) = json.pointer("/asset/extras/aiai") {
        info.format = a.get("format").and_then(Value::as_str).unwrap_or("").to_string();
        info.file_name = a.get("file_name").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(l) = a.get("license").and_then(|l| serde_json::from_value::<LicenseInfo>(l.clone()).ok()) {
            info.license = l;
        }
        info.warnings = a.get("warnings").and_then(|w| serde_json::from_value::<Vec<String>>(w.clone()).ok()).unwrap_or_default();
    }
    let skin = skin_setup.map(|s| Skin { bones: s.bones, parts: part_skins });
    Ok(Model { parts, info, skin })
}

/// Esqueleto de uma pele do glTF: ossos (juntas em ordem de pai antes de filho, com o repouso global), o mapa junta da
/// pele → osso e a matriz de cada junta no repouso (global × inversa de bind), que leva o vértice do bind ao repouso.
struct SkinSetup {
    bones: Vec<SkinBone>,
    /// Junta k da pele → índice do osso.
    bone_of_joint: Vec<usize>,
    /// Nó → índice do osso (só os nós que são juntas).
    bone_of_node: std::collections::HashMap<usize, usize>,
    joint_mats: Vec<Mat4>,
}

fn skin_setup(parsed: &ParsedGltf, skin: &Value, globals: &[Mat4], parent_of: &[Option<usize>]) -> Result<SkinSetup, ImportError> {
    let joints: Vec<usize> = skin.get("joints").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|x| x as usize).collect()).unwrap_or_default();
    if joints.is_empty() || joints.iter().any(|&j| j >= globals.len()) {
        return err("pele com juntas ausentes ou fora da lista de nós");
    }
    if joints.len() > u16::MAX as usize {
        return err("pele com juntas demais");
    }
    let ibm: Vec<Mat4> = match skin.get("inverseBindMatrices").and_then(Value::as_u64) {
        Some(a) => {
            let v = gltf::read_accessor_f32(parsed, a as usize)?;
            if v.len() < joints.len() * 16 {
                return err("pele: inverseBindMatrices menor que o número de juntas");
            }
            v.chunks_exact(16).take(joints.len()).map(|c| Mat4::from_cols_slice(c)).collect()
        }
        None => vec![Mat4::IDENTITY; joints.len()],
    };
    let joint_set: std::collections::HashSet<usize> = joints.iter().copied().collect();
    // pai = o ancestral mais próximo que também é junta
    let joint_parent = |j: usize| -> Option<usize> {
        let mut cur = parent_of[j];
        let mut guard = 0;
        while let Some(c) = cur {
            if joint_set.contains(&c) {
                return Some(c);
            }
            cur = parent_of[c];
            guard += 1;
            if guard > 100_000 {
                break;
            }
        }
        None
    };
    let depth = |j: usize| -> usize {
        let mut d = 0;
        let mut cur = joint_parent(j);
        while let Some(c) = cur {
            d += 1;
            cur = joint_parent(c);
            if d > 10_000 {
                break;
            }
        }
        d
    };
    let mut order: Vec<usize> = (0..joints.len()).collect();
    order.sort_by_key(|&k| depth(joints[k]));
    let mut bone_of_node = std::collections::HashMap::new();
    let mut bone_of_joint = vec![0usize; joints.len()];
    let mut bones = Vec::with_capacity(joints.len());
    for &k in &order {
        let node = joints[k];
        let (_, rot, pos) = globals[node].to_scale_rotation_translation();
        let name = parsed.json["nodes"][node].get("name").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("osso {node}"));
        let parent = joint_parent(node).and_then(|p| bone_of_node.get(&p).copied());
        bone_of_joint[k] = bones.len();
        bone_of_node.insert(node, bones.len());
        bones.push(SkinBone { name, parent, rest_pos: pos.to_array(), rest_rot: rot.normalize().to_array() });
    }
    let joint_mats = joints.iter().zip(&ibm).map(|(&j, m)| globals[j] * *m).collect();
    Ok(SkinSetup { bones, bone_of_joint, bone_of_node, joint_mats })
}

/// Parte sem pele num modelo com pele: segue a junta ancestral mais próxima do nó (ou fica parada, peso 0).
fn rigid_part_skin(setup: Option<&SkinSetup>, node: usize, parent_of: &[Option<usize>], n: usize) -> PartSkin {
    let Some(s) = setup else { return PartSkin::default() };
    let mut cur = Some(node);
    while let Some(c) = cur {
        if let Some(&b) = s.bone_of_node.get(&c) {
            return PartSkin { joints: vec![[b as u16, 0, 0, 0]; n], weights: vec![[1.0, 0.0, 0.0, 0.0]; n] };
        }
        cur = parent_of[c];
    }
    PartSkin { joints: vec![[0; 4]; n], weights: vec![[0.0; 4]; n] }
}

#[allow(clippy::too_many_arguments)]
fn primitive_part(
    parsed: &ParsedGltf,
    prim: &Value,
    world: &Mat4,
    mesh_name: &str,
    pi: usize,
    n: usize,
    warnings: &mut Vec<String>,
    skin: Option<&SkinSetup>,
) -> Result<Option<(Part, Option<PartSkin>)>, ImportError> {
    let mode = prim.get("mode").and_then(Value::as_u64).unwrap_or(4);
    if !matches!(mode, 4..=6) {
        warnings.push(format!("{mesh_name}: primitiva {pi} com modo {mode} (pontos/linhas) pulada"));
        return Ok(None);
    }
    if prim.get("targets").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) && !warnings.iter().any(|w| w.starts_with("morphs")) {
        warnings.push("morphs (blend shapes) ignorados na v1".into());
    }
    let pos_idx = prim.pointer("/attributes/POSITION").and_then(Value::as_u64).ok_or_else(|| ImportError(format!("{mesh_name}: primitiva sem POSITION")))? as usize;
    let pos = gltf::read_accessor_f32(parsed, pos_idx)?;
    if pos.len() % 3 != 0 {
        return err(format!("{mesh_name}: POSITION com {} floats (não é VEC3)", pos.len()));
    }
    let count = pos.len() / 3;
    let nor = match prim.pointer("/attributes/NORMAL").and_then(Value::as_u64) {
        Some(i) => {
            let v = gltf::read_accessor_f32(parsed, i as usize)?;
            (v.len() == count * 3).then_some(v)
        }
        None => None,
    };
    let raw: Vec<u32> = match prim.get("indices").and_then(Value::as_u64) {
        Some(i) => gltf::read_accessor_indices(parsed, i as usize)?,
        None => (0..count as u32).collect(),
    };
    if let Some(bad) = raw.iter().find(|&&i| i as usize >= count) {
        return err(format!("{mesh_name}: índice {bad} fora dos {count} vértices"));
    }
    let mut indices = Vec::with_capacity(raw.len());
    match mode {
        4 => indices.extend(raw.chunks_exact(3).flatten().copied()),
        5 => {
            for i in 2..raw.len() {
                let (a, b, c) = if i % 2 == 0 { (raw[i - 2], raw[i - 1], raw[i]) } else { (raw[i - 1], raw[i - 2], raw[i]) };
                indices.extend([a, b, c]);
            }
        }
        _ => {
            for i in 2..raw.len() {
                indices.extend([raw[0], raw[i - 1], raw[i]]);
            }
        }
    }
    // pele: pesos por vértice e a matriz de repouso de cada vértice (soma ponderada das juntas)
    let mut part_skin = None;
    let mut vert_mats: Option<Vec<Mat4>> = None;
    if let Some(sk) = skin {
        let ji = prim.pointer("/attributes/JOINTS_0").and_then(Value::as_u64);
        let wi = prim.pointer("/attributes/WEIGHTS_0").and_then(Value::as_u64);
        if let (Some(ji), Some(wi)) = (ji, wi) {
            let jv = gltf::read_accessor_f32(parsed, ji as usize)?;
            let wv = gltf::read_accessor_f32(parsed, wi as usize)?;
            if jv.len() != count * 4 || wv.len() != count * 4 {
                return err(format!("{mesh_name}: JOINTS_0/WEIGHTS_0 sem 4 valores por vértice"));
            }
            let mut joints = Vec::with_capacity(count);
            let mut weights = Vec::with_capacity(count);
            let mut mats = Vec::with_capacity(count);
            for v in 0..count {
                let mut j4 = [0u16; 4];
                let mut w4 = [0f32; 4];
                let mut m = Mat4::ZERO;
                let mut sum = 0.0;
                for k in 0..4 {
                    let jk = jv[v * 4 + k] as usize;
                    let wk = wv[v * 4 + k].max(0.0);
                    if wk <= 0.0 {
                        continue;
                    }
                    if jk >= sk.bone_of_joint.len() {
                        return err(format!("{mesh_name}: junta {jk} fora da pele"));
                    }
                    j4[k] = sk.bone_of_joint[jk] as u16;
                    w4[k] = wk;
                    m += sk.joint_mats[jk] * wk;
                    sum += wk;
                }
                if sum > 1e-6 {
                    for w in &mut w4 {
                        *w /= sum;
                    }
                    m *= 1.0 / sum;
                } else {
                    m = Mat4::IDENTITY;
                }
                joints.push(j4);
                weights.push(w4);
                mats.push(m);
            }
            part_skin = Some(PartSkin { joints, weights });
            vert_mats = Some(mats);
        } else {
            warnings.push(format!("{mesh_name}: malha com pele sem JOINTS_0/WEIGHTS_0 (fica parada)"));
        }
    }
    let normal_m = Mat3::from_mat4(*world).inverse().transpose();
    let positions: Vec<[f32; 3]> = match &vert_mats {
        Some(ms) => pos.chunks_exact(3).zip(ms).map(|(p, m)| m.transform_point3(Vec3::new(p[0], p[1], p[2])).to_array()).collect(),
        None => pos.chunks_exact(3).map(|p| world.transform_point3(Vec3::new(p[0], p[1], p[2])).to_array()).collect(),
    };
    if vert_mats.is_none() && world.determinant() < 0.0 {
        for t in indices.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
    }
    let normals = match (nor, &vert_mats) {
        (Some(v), Some(ms)) => v.chunks_exact(3).zip(ms).map(|(q, m)| (Mat3::from_mat4(*m) * Vec3::new(q[0], q[1], q[2])).normalize_or(Vec3::Y).to_array()).collect(),
        (Some(v), None) => v.chunks_exact(3).map(|q| (normal_m * Vec3::new(q[0], q[1], q[2])).normalize_or(Vec3::Y).to_array()).collect(),
        (None, _) => smooth_normals(&positions, &indices),
    };
    let (color, material) = material_of(&parsed.json, prim.get("material").and_then(Value::as_u64));
    let name = if n > 1 { format!("{mesh_name}.{pi}") } else { mesh_name.to_string() };
    Ok(Some((Part { name, positions, normals, indices, color, material }, part_skin)))
}

// ------------------------------------------------------------------------------------------------------------- OBJ

fn parse_mtl(text: &str) -> std::collections::BTreeMap<String, [f32; 4]> {
    let mut out = std::collections::BTreeMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("newmtl") => {
                let name = it.collect::<Vec<_>>().join(" ");
                out.insert(name.clone(), [0.8, 0.8, 0.8, 1.0]);
                cur = Some(name);
            }
            Some("Kd") => {
                let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
                if let (Some(c), 3..) = (cur.as_ref().and_then(|n| out.get_mut(n)), v.len()) {
                    c[0] = v[0].clamp(0.0, 1.0);
                    c[1] = v[1].clamp(0.0, 1.0);
                    c[2] = v[2].clamp(0.0, 1.0);
                }
            }
            Some("d") => {
                if let (Some(c), Some(a)) = (cur.as_ref().and_then(|n| out.get_mut(n)), it.next().and_then(|x| x.parse::<f32>().ok())) {
                    c[3] = a.clamp(0.0, 1.0);
                }
            }
            _ => {}
        }
    }
    out
}

fn from_obj(bytes: &[u8], resolver: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model, ImportError> {
    let text = String::from_utf8_lossy(bytes);
    let mut v: Vec<[f32; 3]> = Vec::new();
    let mut vn: Vec<[f32; 3]> = Vec::new();
    let mut mtl = std::collections::BTreeMap::new();
    let mut warnings = Vec::new();
    let mut license = LicenseInfo::default();
    // partes por (objeto/grupo, material), na ordem em que aparecem
    struct Building {
        name: String,
        material: String,
        map: std::collections::HashMap<(usize, usize), u32>,
        positions: Vec<[f32; 3]>,
        normals: Vec<[f32; 3]>,
        has_normals: bool,
        indices: Vec<u32>,
    }
    let mut parts: Vec<Building> = Vec::new();
    let mut group = String::from("objeto");
    let mut material = String::from("padrão");
    let mut cur: Option<usize> = None;
    let mut faces = 0usize;
    for (ln, line) in text.lines().enumerate() {
        let line = line.trim();
        if let Some(c) = line.strip_prefix('#') {
            let c = c.trim();
            let low = c.to_lowercase();
            for (k, field) in [("license:", &mut license.license), ("author:", &mut license.author), ("copyright", &mut license.copyright), ("source:", &mut license.source)] {
                if low.starts_with(k) && field.is_empty() {
                    *field = c[k.len()..].trim_start_matches([':', ' ']).trim().to_string();
                }
            }
            continue;
        }
        let mut it = line.split_whitespace();
        let Some(tag) = it.next() else { continue };
        let num = |s: Option<&str>| -> Result<f32, ImportError> {
            s.and_then(|x| x.parse::<f32>().ok()).filter(|x| x.is_finite()).ok_or_else(|| ImportError(format!("OBJ linha {}: número inválido", ln + 1)))
        };
        match tag {
            "v" => v.push([num(it.next())?, num(it.next())?, num(it.next())?]),
            "vn" => vn.push([num(it.next())?, num(it.next())?, num(it.next())?]),
            "o" | "g" => {
                let n = it.collect::<Vec<_>>().join(" ");
                group = if n.is_empty() { "objeto".into() } else { n };
                cur = None;
            }
            "usemtl" => {
                material = it.collect::<Vec<_>>().join(" ");
                cur = None;
            }
            "mtllib" => {
                let file = it.collect::<Vec<_>>().join(" ");
                match resolver(&file) {
                    Some(b) => mtl.extend(parse_mtl(&String::from_utf8_lossy(&b))),
                    None => warnings.push(format!("arquivo de materiais '{file}' não encontrado (cores padrão)")),
                }
            }
            "f" => {
                let idx = match cur {
                    Some(i) => i,
                    None => {
                        parts.push(Building { name: group.clone(), material: material.clone(), map: Default::default(), positions: Vec::new(), normals: Vec::new(), has_normals: true, indices: Vec::new() });
                        cur = Some(parts.len() - 1);
                        parts.len() - 1
                    }
                };
                let b = &mut parts[idx];
                let mut poly: Vec<u32> = Vec::new();
                for tok in it {
                    let mut f = tok.split('/');
                    let resolve = |s: Option<&str>, len: usize| -> Result<Option<usize>, ImportError> {
                        let Some(s) = s.filter(|s| !s.is_empty()) else { return Ok(None) };
                        let i: i64 = s.parse().map_err(|_| ImportError(format!("OBJ linha {}: índice '{s}' inválido", ln + 1)))?;
                        let r = if i < 0 { len as i64 + i } else { i - 1 };
                        if r < 0 || r as usize >= len {
                            return err(format!("OBJ linha {}: índice {i} fora da lista ({len})", ln + 1));
                        }
                        Ok(Some(r as usize))
                    };
                    let pv = resolve(f.next(), v.len())?.ok_or_else(|| ImportError(format!("OBJ linha {}: face sem vértice", ln + 1)))?;
                    let _vt = f.next();
                    let pn = resolve(f.next(), vn.len())?;
                    if pn.is_none() {
                        b.has_normals = false;
                    }
                    let key = (pv, pn.map(|x| x + 1).unwrap_or(0));
                    let id = match b.map.get(&key) {
                        Some(&id) => id,
                        None => {
                            let id = b.positions.len() as u32;
                            b.positions.push(v[pv]);
                            b.normals.push(pn.map(|n| vn[n]).unwrap_or([0.0, 1.0, 0.0]));
                            b.map.insert(key, id);
                            id
                        }
                    };
                    poly.push(id);
                }
                if poly.len() < 3 {
                    warnings.push(format!("OBJ linha {}: face com menos de 3 vértices pulada", ln + 1));
                    continue;
                }
                for i in 2..poly.len() {
                    b.indices.extend([poly[0], poly[i - 1], poly[i]]);
                }
                faces += poly.len() - 2;
                if faces > MAX_TRIANGLES {
                    return err(format!("o modelo passa de {MAX_TRIANGLES} triângulos (limite da camada 3D; reduza a malha no programa de origem)"));
                }
            }
            _ => {}
        }
    }
    let parts = parts
        .into_iter()
        .map(|b| {
            let normals = if b.has_normals { b.normals.iter().map(|n| Vec3::from(*n).normalize_or(Vec3::Y).to_array()).collect() } else { smooth_normals(&b.positions, &b.indices) };
            let color = mtl.get(&b.material).copied().unwrap_or([0.8, 0.8, 0.8, 1.0]);
            let name = if b.material == "padrão" { b.name } else { format!("{} ({})", b.name, b.material) };
            Part { name, positions: b.positions, normals, indices: b.indices, color, material: b.material }
        })
        .collect();
    Ok(Model { parts, info: ModelInfo { format: "obj".into(), license, warnings, ..Default::default() }, skin: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUBE_OBJ: &str = "# author: teste\n# license: CC0\no Caixa\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\nusemtl vermelho\nf 1 2 3 4\nf 5 8 7 6\nf 1 5 6 2\nf 2 6 7 3\nf 3 7 8 4\nf 5 1 4 8\nmtllib cubo.mtl\n";

    #[test]
    fn obj_with_quads_mtl_and_license() {
        let mtl = "newmtl vermelho\nKd 0.9 0.1 0.1\n";
        let res = |f: &str| (f == "cubo.mtl").then(|| mtl.as_bytes().to_vec());
        // o mtllib depois das faces ainda vale para a cor (é lido antes de montar as partes)
        let m = import_bytes(CUBE_OBJ.as_bytes(), "obj", "cubo.obj", &res).unwrap();
        assert_eq!(m.info.triangles, 12);
        assert_eq!(m.info.parts, 1);
        assert_eq!(m.info.bbox, [[0.0; 3], [1.0; 3]]);
        assert_eq!(m.info.license.author, "teste");
        assert_eq!(m.info.license.license, "CC0");
        assert!((m.parts[0].color[0] - 0.9).abs() < 1e-6);
        assert_eq!(m.parts[0].normals.len(), m.parts[0].positions.len());
    }

    #[test]
    fn bad_files_give_clear_errors() {
        let none = |_: &str| None;
        assert!(import_bytes(b"v 0 0 0\nf 1 2 3\n", "obj", "x.obj", &none).unwrap_err().0.contains("fora da lista"));
        assert!(import_bytes(b"# vazio\n", "obj", "x.obj", &none).unwrap_err().0.contains("nenhuma malha"));
        assert!(import_bytes(b"xx", "vrm", "x.vrm", &none).unwrap_err().0.contains("em breve"));
        assert!(import_bytes(b"not a glb at all, really", "glb", "x.glb", &none).is_err());
        assert!(import_bytes(b"{}", "fbx", "x.fbx", &none).unwrap_err().0.contains("não suportado"));
    }

    #[test]
    fn triangle_limit_is_enforced() {
        let mut s = String::from("v 0 0 0\nv 1 0 0\nv 0 1 0\n");
        for _ in 0..(MAX_TRIANGLES + 1) {
            s.push_str("f 1 2 3\n");
        }
        let e = import_bytes(s.as_bytes(), "obj", "big.obj", &|_| None).unwrap_err();
        assert!(e.0.contains("500000") && e.0.contains("triângulos"), "{e}");
    }
}
