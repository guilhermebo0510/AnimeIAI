//! Modelos 3D de referência do AnimeIAI (S24 3D, fases 2 a 5): importação glTF/GLB/OBJ ([`import`]) para o modelo
//! normalizado ([`model`]) guardado como GLB ([`glb`]); câmera orbital e gizmo em matemática pura ([`camera`],
//! [`gizmo`], [`gizmo_apply`]); cena para o renderer cel da GPU ([`scene`]) e rasterizador CPU de reserva ([`raster`]).
//! Sem wgpu e sem egui. Procedência das partes vendorizadas do ANIGO em `VENDOR.md`.

pub mod camera;
pub mod gizmo;
pub mod gizmo_apply;
pub mod glb;
pub mod gltf;
pub mod import;
pub mod mannequin;
pub mod model;
pub mod pose;
pub mod raster;
pub mod scene;

pub use import::{EXTENSIONS, ImportError, MAX_TRIANGLES, import_bytes, import_path};
pub use model::{LicenseInfo, Model, ModelInfo, Part, PartSkin, Skin, SkinBone};

use aiai_core::{BlobHash, BlobStore};
use std::sync::{Arc, Mutex, OnceLock};

/// Versão do caminho de render (entra na chave do cache de rasters: subir quando o desenho mudar).
pub const RENDER_VERSION: u32 = 1;

/// Modelos já lidos do blob, por hash (poucos: um projeto costuma ter um punhado de modelos).
static MODELS: OnceLock<Mutex<Vec<(BlobHash, Arc<Model>)>>> = OnceLock::new();
const MODEL_CACHE: usize = 8;

/// Modelo do blob `hash` (lido e guardado na primeira vez).
pub fn load_model(blobs: &BlobStore, hash: &BlobHash) -> Result<Arc<Model>, String> {
    let cache = MODELS.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut c) = cache.lock() {
        if let Some(i) = c.iter().position(|(h, _)| h == hash) {
            let e = c.remove(i);
            let m = Arc::clone(&e.1);
            c.push(e);
            return Ok(m);
        }
    }
    let bytes = blobs.get(hash).ok_or_else(|| format!("blob do modelo 3D {} ausente", hash.0))?;
    let m = Arc::new(glb::read_glb(&bytes).map_err(|e| e.0)?);
    if let Ok(mut c) = cache.lock() {
        c.push((hash.clone(), Arc::clone(&m)));
        while c.len() > MODEL_CACHE {
            c.remove(0);
        }
    }
    Ok(m)
}

/// Modelo do blob na pose do estado (skinning na CPU, com cache); sem pose ou sem pele, o modelo como está.
pub fn load_posed(blobs: &BlobStore, hash: &BlobHash, pose: Option<&aiai_core::model3d::Pose3d>) -> Result<Arc<Model>, String> {
    let m = load_model(blobs, hash)?;
    Ok(pose::posed(hash, &m, pose))
}
