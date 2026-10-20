//! Biblioteca de recursos (S24 2.7–2.8) do lado do documento: listar/buscar, importar, usar no projeto, salvar do projeto na
//! biblioteca, editar metadados, duplicar e apagar (lixeira). A pasta, o índice e a importação são do `aiai-assets`; aqui
//! ficam as operações que mexem no documento (um modelo vira camada 3D, uma textura vira camada de referência, uma
//! paleta entra no projeto) e que viram comandos do núcleo (um passo de undo), iguais para a UI, a bridge/MCP e o headless.

use crate::context::{ToolError, ToolResult};
use crate::model3d::{self, Model3dOp, Model3dRequest};
use aiai_assets::envelope::Envelope;
use aiai_assets::import::{self, ImportOpts};
use aiai_assets::index::{Entry, Index, Query, Sort, entry_json};
use aiai_assets::palette::{PalColor, Palette, parse_hex, to_hex};
use aiai_assets::store::{self, MetaEdit};
use aiai_assets::{Kind, parse_tags};
use aiai_core::*;
use aiai_paint::brush_pack::{self, Package};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LibraryOp {
    /// Lista os recursos da biblioteca (com `kind`, `query`, `tags`, `sort`, `limit` filtra e ordena).
    List,
    /// Igual a `list` (o nome que o agente procura).
    Search,
    /// Importa um arquivo (`path`) para a biblioteca.
    Import,
    /// Usa um recurso (`id`) no projeto.
    Use,
    /// Salva na biblioteca uma paleta ou um modelo 3D do projeto.
    Save,
    /// Edita os metadados de um recurso (`id`).
    Update,
    /// Duplica um recurso (`id`) com id novo.
    Duplicate,
    /// Manda um recurso (`id`) para a `_Lixeira` da pasta do tipo.
    Delete,
}

/// Pedido `LIBRARY`. Chave desconhecida é erro.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct LibraryRequest {
    pub op: Option<LibraryOp>,
    /// list/search: tipo (`model3d`, `material`, `texture`, `palette`, `pose`, `brush`). import: `texture` (padrão) ou
    /// `material` para uma imagem; `pose` ou `palette` para um `.json` (sem isso: lista de cores ou objeto com `colors`
    /// é paleta, o resto é pose).
    #[serde(default)]
    pub kind: Option<String>,
    /// list/search: texto (nome, etiquetas, autor, licença, fonte; sem acento nem maiúsculas; todas as palavras).
    #[serde(default)]
    pub query: Option<String>,
    /// list/search: só recursos com TODAS estas etiquetas. import/update: etiquetas do recurso (update substitui).
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// list/search: `name` (padrão), `newest`, `size`, `kind`.
    #[serde(default)]
    pub sort: Option<String>,
    /// list/search: no máximo tantos recursos (padrão 200).
    #[serde(default)]
    pub limit: Option<usize>,
    /// import: caminho absoluto do arquivo (.glb .gltf .obj, .png .jpg .webp .bmp, .gpl .ase .aiaipal .json, .aiaipose,
    /// .aiaibrush .aiaibrushes).
    #[serde(default)]
    pub path: Option<String>,
    /// import/save/update: nome do recurso.
    #[serde(default)]
    pub name: Option<String>,
    /// import/save/update: licença (id SPDX como CC0-1.0 / CC-BY-4.0 ou texto livre). Sem resposta na importação vira
    /// "desconhecida — verifique com o autor" (ou a que o arquivo declara).
    #[serde(default)]
    pub license: Option<String>,
    /// import/save/update: autor do recurso (a chave `author` é de quem faz a mudança no documento).
    #[serde(default)]
    pub asset_author: Option<String>,
    /// import/save/update: fonte (endereço ou arquivo de origem).
    #[serde(default)]
    pub source: Option<String>,
    /// update: descrição.
    #[serde(default)]
    pub description: Option<String>,
    /// import/save: subpasta dentro da pasta do tipo.
    #[serde(default)]
    pub folder: Option<String>,
    /// use/update/duplicate/delete: id do recurso (vem de `list`).
    #[serde(default)]
    pub id: Option<String>,
    /// use: corte onde criar a camada (modelo 3D, textura).
    #[serde(default)]
    pub cut: Option<Id>,
    /// use: `layer` (padrão com `cut`: cria a camada) ou `asset` (só copia para o projeto). Material toon: a camada 3D a
    /// que ele se aplica vai em `layer_id`.
    #[serde(default)]
    pub mode: Option<String>,
    /// use (material toon): camada 3D que recebe as cores do material.
    #[serde(default)]
    pub layer_id: Option<Id>,
    /// save: `palette` (precisa de `palette`) ou `model` (precisa de `asset` ou de `layer_id` de uma camada 3D).
    #[serde(default)]
    pub from: Option<String>,
    /// save: paleta do projeto.
    #[serde(default)]
    pub palette: Option<Id>,
    /// save: ativo 3D do projeto.
    #[serde(default)]
    pub asset: Option<Id>,
}

/// Resultado: comando do documento (se a operação muda o projeto) e o que a resposta leva.
pub struct LibraryOutcome {
    pub command: Option<Command>,
    pub value: Value,
}

fn invalid<T>(m: impl Into<String>) -> ToolResult<T> {
    Err(ToolError::Invalid(m.into()))
}

fn need<'a>(v: &'a Option<String>, what: &str) -> ToolResult<&'a str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| ToolError::Invalid(format!("falta `{what}`")))
}

fn kind_arg(v: &Option<String>) -> ToolResult<Option<Kind>> {
    match v.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => Kind::from_key(s).map(Some).ok_or_else(|| ToolError::Invalid(format!("tipo '{s}' desconhecido (model3d, material, texture, palette, pose, brush)"))),
    }
}

fn opts_of(r: &LibraryRequest) -> ToolResult<ImportOpts> {
    Ok(ImportOpts {
        author: r.asset_author.clone().unwrap_or_default(),
        license: r.license.clone().unwrap_or_default(),
        source: r.source.clone().unwrap_or_default(),
        tags: r.tags.clone().map(|t| parse_tags(&t.join(","))).unwrap_or_default(),
        as_kind: match kind_arg(&r.kind)? {
            None => None,
            Some(k @ (Kind::Texture | Kind::Material | Kind::Palette | Kind::Pose)) => Some(k),
            Some(other) => return invalid(format!("import: `kind` só vale para arquivo ambíguo (imagem: texture ou material; .json: palette ou pose), não '{}'", other.key())),
        },
        name: r.name.clone(),
        folder: r.folder.clone().unwrap_or_default(),
    })
}

fn find<'a>(idx: &'a Index, id: &str) -> ToolResult<&'a Entry> {
    idx.get(id).ok_or_else(|| ToolError::NotFound(format!("recurso '{id}' da biblioteca (veja `list`)")))
}

/// Lista pronta para a resposta: os recursos filtrados, as contagens por tipo, as etiquetas e os arquivos com problema.
pub fn list_json(idx: &Index, q: &Query, limit: usize) -> Value {
    let all = idx.search(q);
    let total = all.len();
    let items: Vec<Value> = all.into_iter().take(limit).map(entry_json).collect();
    let counts: BTreeMap<&str, usize> = Kind::ALL.iter().map(|k| (k.key(), idx.count(Some(*k)))).collect();
    json!({
        "enabled": true,
        "root": idx.root.display().to_string(),
        "total": total,
        "count": items.len(),
        "items": items,
        "counts": counts,
        "tags": idx.tag_counts(q.kind).into_iter().take(60).map(|(t, n)| json!({ "tag": t, "count": n })).collect::<Vec<_>>(),
        "problems": idx.problems.iter().map(|p| json!({ "path": p.path.display().to_string(), "reason": p.reason })).collect::<Vec<_>>(),
    })
}

pub fn run(root: &Path, p: &Project, ids: &mut IdGen, blobs: &mut BlobStore, r: &LibraryRequest) -> ToolResult<LibraryOutcome> {
    let op = r.op.ok_or_else(|| ToolError::Invalid("falta `op` (list | search | import | use | save | update | duplicate | delete)".into()))?;
    match op {
        LibraryOp::List | LibraryOp::Search => {
            let sort = match r.sort.as_deref() {
                None => Sort::Name,
                Some(s) => Sort::from_key(s).ok_or_else(|| ToolError::Invalid(format!("sort '{s}' desconhecido (name, newest, size, kind)")))?,
            };
            let q = Query { kind: kind_arg(&r.kind)?, text: r.query.clone().unwrap_or_default(), tags: r.tags.clone().unwrap_or_default(), sort };
            Ok(LibraryOutcome { command: None, value: list_json(&Index::scan(root), &q, r.limit.unwrap_or(200).clamp(1, 5000)) })
        }
        LibraryOp::Import => {
            let path = need(&r.path, "path")?;
            let src = Path::new(path);
            if !src.is_absolute() || src.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                return invalid("'path' deve ser um caminho absoluto sem '..'");
            }
            let imported = import::import_file(root, src, &opts_of(r)?).map_err(ToolError::Invalid)?;
            let items: Vec<Value> = imported.iter().map(|i| json!({ "id": i.id, "kind": i.kind.key(), "name": i.name, "path": i.path.display().to_string(), "warnings": i.warnings })).collect();
            aiai_assets::bump();
            Ok(LibraryOutcome { command: None, value: json!({ "imported": items, "count": items.len() }) })
        }
        LibraryOp::Update => {
            let idx = Index::scan(root);
            let e = find(&idx, need(&r.id, "id")?)?;
            let edit = MetaEdit {
                name: r.name.clone(),
                description: r.description.clone(),
                tags: r.tags.clone().map(|t| parse_tags(&t.join(","))),
                author: r.asset_author.clone(),
                license: r.license.clone(),
                source: r.source.clone(),
                category: None,
            };
            if edit.is_empty() {
                return invalid("update sem nada a mudar (name, description, tags, asset_author, license, source)");
            }
            store::update_meta(e, &edit).map_err(ToolError::Invalid)?;
            let after = Index::scan(root);
            let item = after.get(&e.id).map(entry_json);
            Ok(LibraryOutcome { command: None, value: json!({ "item": item }) })
        }
        LibraryOp::Duplicate => {
            let idx = Index::scan(root);
            let e = find(&idx, need(&r.id, "id")?)?;
            let to = store::duplicate(e).map_err(ToolError::Invalid)?;
            let after = Index::scan(root);
            let item = after.entries.iter().find(|x| x.path == to).map(entry_json);
            Ok(LibraryOutcome { command: None, value: json!({ "item": item }) })
        }
        LibraryOp::Delete => {
            let idx = Index::scan(root);
            let e = find(&idx, need(&r.id, "id")?)?;
            let to = store::trash(root, e).map_err(ToolError::Invalid)?;
            Ok(LibraryOutcome { command: None, value: json!({ "deleted": e.id, "trash": to.display().to_string() }) })
        }
        LibraryOp::Save => save(root, p, blobs, r),
        LibraryOp::Use => use_item(root, p, ids, blobs, r),
    }
}

// ------------------------------------------------------------------------------------------------ paleta ↔ documento

/// Paleta da biblioteca → dados da paleta do documento (ids novos).
pub fn palette_data_of(pal: &Palette, ids: &mut IdGen) -> PaletteData {
    let rgba = |h: &str| parse_hex(h).map(|c| Rgba([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0]));
    let colors = pal
        .colors
        .iter()
        .filter_map(|c| {
            Some(NamedColor {
                id: ids.next(),
                name: c.name.clone(),
                group: c.group.clone(),
                role: if c.role.is_empty() { "normal".into() } else { c.role.clone() },
                rgba: rgba(&c.hex)?,
                variants: c.variants.iter().filter_map(|(k, v)| Some((k.clone(), rgba(v)?))).collect(),
            })
        })
        .collect();
    PaletteData { colors, active_variant: String::new() }
}

/// Paleta do documento → paleta da biblioteca.
pub fn palette_of(name: &str, pd: &PaletteData) -> Palette {
    let hex = |c: &Rgba| to_hex([(c.0[0].clamp(0.0, 1.0) * 255.0).round() as u8, (c.0[1].clamp(0.0, 1.0) * 255.0).round() as u8, (c.0[2].clamp(0.0, 1.0) * 255.0).round() as u8, (c.0[3].clamp(0.0, 1.0) * 255.0).round() as u8]);
    Palette {
        name: name.to_owned(),
        colors: pd
            .colors
            .iter()
            .map(|c| PalColor {
                name: c.name.clone(),
                hex: hex(&c.rgba),
                group: c.group.clone(),
                role: if c.role == "normal" { String::new() } else { c.role.clone() },
                variants: c.variants.iter().map(|(k, v)| (k.clone(), hex(v))).collect(),
            })
            .collect(),
        warnings: Vec::new(),
    }
}

// ------------------------------------------------------------------------------------------------ salvar do projeto

fn save(root: &Path, p: &Project, blobs: &BlobStore, r: &LibraryRequest) -> ToolResult<LibraryOutcome> {
    let from = need(&r.from, "from")?;
    let mut o = opts_of(r)?;
    o.as_kind = None;
    let imported = match from {
        "palette" => {
            let id = r.palette.as_ref().ok_or_else(|| ToolError::Invalid("save from=palette precisa de `palette` (id da paleta do projeto)".into()))?;
            let n = p.node(id).ok_or_else(|| ToolError::NotFound(format!("paleta {id}")))?;
            let NodeKind::Palette(pd) = &n.data else { return invalid(format!("{id} não é uma paleta")) };
            if pd.colors.is_empty() {
                return invalid(format!("a paleta '{}' não tem cores", n.name));
            }
            if o.name.as_deref().is_none_or(|s| s.trim().is_empty()) {
                o.name = Some(n.name.clone());
            }
            let pal = palette_of(&n.name, pd);
            import::add_palette(root, &pal, &o, &format!("projeto: {}", n.name)).map_err(ToolError::Invalid)?
        }
        "model" => {
            let asset = match (&r.asset, &r.layer_id) {
                (Some(a), _) => a.clone(),
                (None, Some(l)) => model3d::layer_params(p, l)?.1.asset,
                (None, None) => return invalid("save from=model precisa de `asset` (ativo 3D) ou `layer_id` (camada 3D)"),
            };
            let n = p.node(&asset).ok_or_else(|| ToolError::NotFound(format!("ativo {asset}")))?;
            let NodeKind::Asset(a) = &n.data else { return invalid(format!("{asset} não é um ativo")) };
            if a.asset != AssetType::Model3d {
                return invalid(format!("{asset} não é um ativo 3D"));
            }
            let hash = a.blob.clone().ok_or_else(|| ToolError::Invalid(format!("ativo 3D {asset} sem blob")))?;
            let bytes = blobs.get(&hash).ok_or_else(|| ToolError::Invalid(format!("blob do ativo 3D {asset} ausente")))?;
            let info: aiai_3d_model::ModelInfo = a.params.get("model3d").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
            if o.name.as_deref().is_none_or(|s| s.trim().is_empty()) {
                o.name = Some(n.name.clone());
            }
            // o modelo veio da biblioteca: a licença e o autor que ela tinha acompanham a cópia salva
            if o.license.trim().is_empty() {
                o.license = a.params.pointer("/library/license").and_then(Value::as_str).unwrap_or("").to_owned();
            }
            import::add_model_glb(root, &bytes, &info, &o).map_err(ToolError::Invalid)?
        }
        other => return invalid(format!("save `from` '{other}' desconhecido (palette ou model)")),
    };
    aiai_assets::bump();
    let idx = Index::scan(root);
    let item = idx.get(&imported.id).map(entry_json);
    Ok(LibraryOutcome { command: None, value: json!({ "saved": { "id": imported.id, "kind": imported.kind.key(), "name": imported.name, "path": imported.path.display().to_string() }, "item": item }) })
}

// ------------------------------------------------------------------------------------------------ usar no projeto

/// Marca de procedência do recurso no ativo do projeto.
fn library_mark(e: &Entry) -> Value {
    json!({ "id": e.id, "version": e.env.version, "license": e.env.license_text(), "name": e.env.name })
}

fn set_asset_library(cmds: &mut [Command], mark: Value) {
    for c in cmds {
        if let Command::CreateNode { data: NodeKind::Asset(a), .. } = c {
            if !a.params.is_object() {
                a.params = json!({});
            }
            if let Some(o) = a.params.as_object_mut() {
                o.insert("library".into(), mark.clone());
            }
        }
    }
}

fn want_layer(r: &LibraryRequest) -> ToolResult<bool> {
    match r.mode.as_deref().map(str::trim) {
        None | Some("") => Ok(r.cut.is_some()),
        Some("layer") => {
            if r.cut.is_none() {
                return invalid("use mode=layer precisa de `cut` (o corte onde criar a camada)");
            }
            Ok(true)
        }
        Some("asset") => Ok(false),
        Some(o) => invalid(format!("mode '{o}' desconhecido (layer ou asset)")),
    }
}

fn use_item(root: &Path, p: &Project, ids: &mut IdGen, blobs: &mut BlobStore, r: &LibraryRequest) -> ToolResult<LibraryOutcome> {
    let idx = Index::scan(root);
    let e = find(&idx, need(&r.id, "id")?)?;
    let mark = library_mark(e);
    match e.kind {
        Kind::Model3d => {
            let layer = want_layer(r)?;
            // o mesmo modelo da biblioteca já copiado para o projeto é reaproveitado (sem duplicar o ativo)
            let existing = p.nodes.iter().find_map(|(id, n)| match &n.data {
                NodeKind::Asset(a) if a.asset == AssetType::Model3d && a.params.pointer("/library/id").and_then(Value::as_str) == Some(e.id.as_str()) && a.params.pointer("/library/version").and_then(Value::as_str) == Some(e.env.version.as_str()) && a.blob.as_ref().is_some_and(|b| blobs.contains(b)) => Some(id.clone()),
                _ => None,
            });
            if existing.is_some() && !layer {
                return Ok(LibraryOutcome { command: None, value: json!({ "item": entry_json(e), "asset": existing, "reused": true }) });
            }
            let req = Model3dRequest { op: Some(Model3dOp::Import), path: existing.is_none().then(|| e.path.display().to_string()), asset: existing.clone(), cut: layer.then(|| r.cut.clone()).flatten(), name: Some(e.env.name.clone()), ..Default::default() };
            let mut out = model3d::import(p, ids, blobs, &req)?;
            set_asset_library(&mut out.commands, mark);
            let cmd = out.batch("Usar modelo 3D da biblioteca");
            Ok(LibraryOutcome { command: Some(cmd), value: json!({ "item": entry_json(e), "asset": out.asset, "layer": out.layer, "info": out.info, "reused": existing.is_some() }) })
        }
        Kind::Texture | Kind::Material if e.stats.width.is_some() || aiai_assets::kind::IMAGE_EXTS.contains(&aiai_assets::kind::ext_of(&e.path).as_str()) => {
            let layer = want_layer(r)?;
            let path = e.path.display().to_string();
            if layer {
                let cut = r.cut.clone().ok_or_else(|| ToolError::Invalid("falta `cut`".into()))?;
                let cd = crate::context::cut_data(p, &cut)?;
                let (w, h) = crate::context::canvas_size(p, cd);
                let img = crate::import::fitted_image(&path, w, h, crate::import::ImageFit::Contain)?;
                let png = img.encode_png().map_err(ToolError::Render)?;
                let hash = blobs.put(png);
                let (lid, cid) = (ids.next(), ids.next());
                let mut ld = LayerData::new(LayerType::Reference);
                ld.opacity = 0.6;
                let mut cel = CelData::new(1);
                cel.image = Some(hash);
                let cmd = Command::Batch {
                    label: Some("Usar textura da biblioteca".into()),
                    commands: vec![
                        Command::CreateNode { id: Some(lid.clone()), parent: Some(cut), index: None, name: format!("ref: {}", e.env.name), data: NodeKind::Layer(ld) },
                        Command::CreateNode { id: Some(cid), parent: Some(lid.clone()), index: None, name: "1".into(), data: NodeKind::Cel(cel) },
                    ],
                };
                Ok(LibraryOutcome { command: Some(cmd), value: json!({ "item": entry_json(e), "layer": lid }) })
            } else {
                let img = ::image::open(&e.path).map_err(|er| ToolError::Invalid(format!("{path}: {er}")))?;
                let mut png = std::io::Cursor::new(Vec::new());
                img.write_to(&mut png, ::image::ImageFormat::Png).map_err(|er| ToolError::Render(er.to_string()))?;
                let hash = blobs.put(png.into_inner());
                let id = ids.next();
                let cmd = Command::CreateNode {
                    id: Some(id.clone()),
                    parent: None,
                    index: None,
                    name: e.env.name.clone(),
                    data: NodeKind::Asset(AssetData { asset: AssetType::Texture, blob: Some(hash), params: json!({ "library": mark, "width": img.width(), "height": img.height() }), source: format!("biblioteca: {}; licença: {}", e.env.name, e.env.license_text()) }),
                };
                Ok(LibraryOutcome { command: Some(cmd), value: json!({ "item": entry_json(e), "asset": id }) })
            }
        }
        Kind::Material => {
            // material toon: cores no payload; aplica à camada 3D dada
            let layer = r.layer_id.clone().ok_or_else(|| ToolError::Invalid("use de material toon precisa de `layer_id` (camada 3D que recebe as cores)".into()))?;
            let pl = &e.env.payload;
            let hex = |k: &str| pl.get(k).and_then(Value::as_str).map(str::to_owned);
            let req = Model3dRequest {
                op: Some(Model3dOp::Mode),
                layer: Some(layer),
                color: hex("base_color"),
                outline_color: hex("outline_color"),
                outline_px: pl.get("outline_width").and_then(Value::as_f64).map(|v| v as f32),
                ..Default::default()
            };
            if req.color.is_none() && req.outline_color.is_none() && req.outline_px.is_none() {
                return invalid(format!("o material '{}' não tem base_color, outline_color nem outline_width no payload", e.env.name));
            }
            let cmd = model3d::edit_command(p, blobs, &req)?;
            Ok(LibraryOutcome { command: cmd, value: json!({ "item": entry_json(e) }) })
        }
        Kind::Texture => invalid(format!("'{}' não é uma imagem que o app leia", e.env.name)),
        Kind::Palette => {
            let pal = Palette::from_payload(&e.env.payload).map_err(ToolError::Invalid)?;
            if pal.colors.is_empty() {
                return invalid(format!("a paleta '{}' não tem cores", e.env.name));
            }
            let data = palette_data_of(&pal, ids);
            let id = ids.next();
            let cmd = Command::CreateNode { id: Some(id.clone()), parent: None, index: None, name: e.env.name.clone(), data: NodeKind::Palette(data) };
            Ok(LibraryOutcome { command: Some(cmd), value: json!({ "item": entry_json(e), "palette": id, "colors": pal.colors.len() }) })
        }
        Kind::Brush => {
            // pincel: o preset (com as imagens da ponta/textura já no projeto) para `aiai_paint {preset}`
            let bytes = std::fs::read(&e.path).map_err(|er| ToolError::Invalid(format!("{}: {er}", e.path.display())))?;
            let Package::Brush(b) = brush_pack::read(&bytes).map_err(ToolError::Invalid)? else { return invalid("não é um pincel") };
            for bytes in b.assets.values() {
                blobs.put(bytes.clone());
            }
            let preset = serde_json::to_value(&b.preset).map_err(|er| ToolError::Invalid(er.to_string()))?;
            Ok(LibraryOutcome { command: None, value: json!({ "item": entry_json(e), "preset": preset }) })
        }
        Kind::Pose => invalid("poses são aplicadas pela Modelagem; o recurso fica na biblioteca"),
    }
}

/// Envelope do recurso (para quem precisa do manifesto inteiro: o painel e os testes).
pub fn envelope_of(root: &Path, id: &str) -> ToolResult<Envelope> {
    Ok(find(&Index::scan(root), id)?.env.clone())
}


#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("aiai-tools-lib-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn session() -> (CoreSession, Id) {
        let mut s = CoreSession::for_tests("lib");
        s.project.settings.width = 96;
        s.project.settings.height = 54;
        let mk = |s: &mut CoreSession, parent: Option<&Id>, data: NodeKind| s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: "n".into(), data }).unwrap().created[0].clone();
        let ep = mk(&mut s, None, NodeKind::Episode(EpisodeData::default()));
        let sc = mk(&mut s, Some(&ep), NodeKind::Scene(SceneData::default()));
        let cut = mk(&mut s, Some(&sc), NodeKind::Cut(CutData::new(12)));
        (s, cut)
    }

    fn go(root: &Path, s: &mut CoreSession, r: LibraryRequest) -> ToolResult<Value> {
        let out = run(root, &s.project, &mut s.ids, &mut s.blobs, &r)?;
        if let Some(c) = out.command {
            s.apply(c).unwrap();
        }
        Ok(out.value)
    }

    fn req(op: LibraryOp) -> LibraryRequest {
        LibraryRequest { op: Some(op), ..Default::default() }
    }

    fn files(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("cubo.obj"), "# license: CC0\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\nf 1 2 3 4\nf 5 8 7 6\nf 1 5 6 2\nf 2 6 7 3\nf 3 7 8 4\nf 5 1 4 8\n").unwrap();
        image::RgbaImage::from_pixel(12, 6, image::Rgba([180, 120, 60, 255])).save(dir.join("papel.png")).unwrap();
        std::fs::write(dir.join("peles.gpl"), "GIMP Palette\nName: Peles\n255 224 189 Clara\n141 85 36 Escura\n").unwrap();
    }

    fn id_of(root: &Path, s: &mut CoreSession, kind: &str) -> String {
        go(root, s, LibraryRequest { kind: Some(kind.into()), ..req(LibraryOp::Search) }).unwrap()["items"][0]["id"].as_str().unwrap().to_owned()
    }

    #[test]
    fn rejects_bad_requests_with_clear_messages() {
        let root = tmp("bad");
        let (mut s, _) = session();
        let mut err = |r: LibraryRequest| go(&root, &mut s, r).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err(LibraryRequest::default()).contains("falta `op`"));
        assert!(err(LibraryRequest { id: Some("nao-existe".into()), ..req(LibraryOp::Use) }).contains("nao-existe"));
        assert!(err(LibraryRequest { path: Some("relativo.glb".into()), ..req(LibraryOp::Import) }).contains("absoluto"));
        assert!(err(LibraryRequest { kind: Some("fonte".into()), ..req(LibraryOp::List) }).contains("desconhecido"));
        assert!(err(LibraryRequest { sort: Some("cor".into()), ..req(LibraryOp::List) }).contains("sort"));
        assert!(err(LibraryRequest { from: Some("palette".into()), ..req(LibraryOp::Save) }).contains("palette"));
        assert!(err(req(LibraryOp::Update)).contains("id"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn import_list_use_save_and_edit_flow() {
        let (root, src) = (tmp("flow"), tmp("flow-src"));
        files(&src);
        let (mut s, cut) = session();
        // importar um de cada tipo
        let opts = |p: &str| LibraryRequest { path: Some(src.join(p).display().to_string()), license: Some("CC0-1.0".into()), asset_author: Some("Ana".into()), tags: Some(vec!["teste".into()]), ..req(LibraryOp::Import) };
        for f in ["cubo.obj", "papel.png", "peles.gpl"] {
            let r = go(&root, &mut s, opts(f)).unwrap();
            assert_eq!(r["count"], 1, "{f}: {r}");
        }
        let all = go(&root, &mut s, req(LibraryOp::List)).unwrap();
        assert_eq!(all["total"], 3);
        assert_eq!(all["counts"]["model3d"], 1);
        assert_eq!(all["tags"][0]["tag"], "teste");
        let q = go(&root, &mut s, LibraryRequest { query: Some("PELES".into()), ..req(LibraryOp::Search) }).unwrap();
        assert_eq!(q["total"], 1, "a busca ignora maiúsculas");

        // usar o modelo: camada 3D no corte, ativo com a procedência da biblioteca; um passo de undo
        let model = id_of(&root, &mut s, "model3d");
        let before = s.project.nodes.len();
        let u = go(&root, &mut s, LibraryRequest { id: Some(model.clone()), cut: Some(cut.clone()), ..req(LibraryOp::Use) }).unwrap();
        assert_eq!(s.project.nodes.len(), before + 2);
        let asset: Id = serde_json::from_value(u["asset"].clone()).unwrap();
        let NodeKind::Asset(a) = &s.project.node(&asset).unwrap().data else { panic!() };
        assert_eq!(a.params["library"]["id"], model.as_str());
        assert_eq!(a.asset, AssetType::Model3d);
        // usar de novo só como ativo: reaproveita (sem ativo duplicado)
        let again = go(&root, &mut s, LibraryRequest { id: Some(model.clone()), mode: Some("asset".into()), ..req(LibraryOp::Use) }).unwrap();
        assert_eq!(again["reused"], true);
        assert_eq!(s.project.nodes.len(), before + 2);
        s.undo().unwrap();
        assert_eq!(s.project.nodes.len(), before, "um passo de undo desfaz ativo e camada");

        // textura: camada de referência com a imagem no cel; e como ativo
        let tex = id_of(&root, &mut s, "texture");
        let u = go(&root, &mut s, LibraryRequest { id: Some(tex.clone()), cut: Some(cut.clone()), ..req(LibraryOp::Use) }).unwrap();
        let layer: Id = serde_json::from_value(u["layer"].clone()).unwrap();
        let cel = s.project.node(&layer).unwrap().children[0].clone();
        let NodeKind::Cel(c) = &s.project.node(&cel).unwrap().data else { panic!() };
        assert!(c.image.as_ref().is_some_and(|h| s.blobs.contains(h)));
        assert!(matches!(&s.project.node(&layer).unwrap().data, NodeKind::Layer(l) if l.kind == LayerType::Reference));
        let ua = go(&root, &mut s, LibraryRequest { id: Some(tex), mode: Some("asset".into()), ..req(LibraryOp::Use) }).unwrap();
        let ta: Id = serde_json::from_value(ua["asset"].clone()).unwrap();
        assert!(matches!(&s.project.node(&ta).unwrap().data, NodeKind::Asset(a) if a.asset == AssetType::Texture && a.blob.is_some()));

        // paleta: entra no documento com as cores e nomes
        let pal = id_of(&root, &mut s, "palette");
        let u = go(&root, &mut s, LibraryRequest { id: Some(pal.clone()), ..req(LibraryOp::Use) }).unwrap();
        let pid: Id = serde_json::from_value(u["palette"].clone()).unwrap();
        let NodeKind::Palette(pd) = &s.project.node(&pid).unwrap().data else { panic!() };
        assert_eq!(pd.colors.len(), 2);
        assert_eq!(pd.colors[1].name, "Escura");
        assert_eq!(pd.colors[1].rgba.to_hex(), "#8d5524ff");

        // salvar do projeto: a paleta e o modelo
        let saved = go(&root, &mut s, LibraryRequest { from: Some("palette".into()), palette: Some(pid.clone()), name: Some("Peles do projeto".into()), license: Some("MIT".into()), ..req(LibraryOp::Save) }).unwrap();
        assert_eq!(saved["item"]["name"], "Peles do projeto");
        assert_eq!(saved["item"]["license"], "MIT");
        assert_eq!(saved["item"]["stats"]["colors"], 2);
        let again_use = go(&root, &mut s, LibraryRequest { id: saved["saved"]["id"].as_str().map(str::to_owned), ..req(LibraryOp::Use) }).unwrap();
        assert!(again_use["palette"].is_string());
        let u = go(&root, &mut s, LibraryRequest { id: Some(model.clone()), cut: Some(cut.clone()), ..req(LibraryOp::Use) }).unwrap();
        let layer3d: Id = serde_json::from_value(u["layer"].clone()).unwrap();
        let sm = go(&root, &mut s, LibraryRequest { from: Some("model".into()), layer_id: Some(layer3d), name: Some("Cubo salvo".into()), ..req(LibraryOp::Save) }).unwrap();
        assert_eq!(sm["item"]["kind"], "model3d");
        assert_eq!(sm["item"]["stats"]["triangles"], 12);
        assert_eq!(sm["item"]["license"], "CC0-1.0", "a licença dada na importação acompanha o modelo");

        // editar metadados, duplicar e apagar
        let up = go(&root, &mut s, LibraryRequest { id: Some(model.clone()), name: Some("Cubo de teste".into()), tags: Some(vec!["a".into(), "b".into()]), license: Some("CC-BY-4.0".into()), ..req(LibraryOp::Update) }).unwrap();
        assert_eq!((up["item"]["name"].as_str(), up["item"]["license"].as_str()), (Some("Cubo de teste"), Some("CC-BY-4.0")));
        assert_eq!(up["item"]["tags"], json!(["a", "b"]));
        let dup = go(&root, &mut s, LibraryRequest { id: Some(model.clone()), ..req(LibraryOp::Duplicate) }).unwrap();
        assert_ne!(dup["item"]["id"], model.as_str());
        let del = go(&root, &mut s, LibraryRequest { id: dup["item"]["id"].as_str().map(str::to_owned), ..req(LibraryOp::Delete) }).unwrap();
        assert!(Path::new(del["trash"].as_str().unwrap()).exists());
        let after = go(&root, &mut s, req(LibraryOp::List)).unwrap();
        assert!(after["items"].as_array().unwrap().iter().all(|i| i["id"] != dup["item"]["id"]));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn brush_use_returns_the_preset_and_pose_is_refused() {
        let root = tmp("brush");
        let mut s = session().0;
        let preset = aiai_paint::brush::BrushPreset::round("Giz", 20.0, 0.8);
        let f = brush_pack::BrushFile::from_preset(&preset, None, &|_| None).unwrap();
        let dir = root.join("Pincéis");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Giz.aiaibrush"), brush_pack::write_brush(&f).unwrap()).unwrap();
        std::fs::create_dir_all(root.join("Poses")).unwrap();
        std::fs::write(root.join("Poses").join("t.json"), "{\"bones\": {}}").unwrap();
        let brush = id_of(&root, &mut s, "brush");
        let u = go(&root, &mut s, LibraryRequest { id: Some(brush), ..req(LibraryOp::Use) }).unwrap();
        assert_eq!(u["preset"]["name"], "Giz");
        let pose = id_of(&root, &mut s, "pose");
        assert!(go(&root, &mut s, LibraryRequest { id: Some(pose), ..req(LibraryOp::Use) }).unwrap_err().to_string().contains("Modelagem"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
