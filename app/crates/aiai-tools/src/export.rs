//! Exportações para outros programas de estúdio:
//! - PSD em camadas (8 bits RGBA): cada camada desenhável do corte renderizada sozinha, recortada no conteúdo, com
//!   nome (inclusive Unicode), visibilidade e modo de mistura, mais a imagem composta.
//! - XDTS (Exchange Digital TimeSheet, formato público da Celsys/Toei, versão 5): timesheet de células por camada
//!   (campo 0) e falas (campo 3), para abrir no Clip Studio Paint EX, TVPaint, OpenToonz.

use crate::context::*;
use aiai_core::*;
use aiai_render::{RenderOptions, render_frame};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportPsdRequest {
    pub cut: Id,
    #[serde(default)]
    pub frame: u32,
    /// Caminho absoluto do .psd.
    pub out: String,
    /// true = quadro da câmera (tamanho do projeto); false (padrão) = canvas inteiro do corte.
    #[serde(default)]
    pub camera: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportXdtsRequest {
    pub cut: Id,
    /// Caminho absoluto do .xdts.
    pub out: String,
}

fn blend_key(b: BlendMode) -> &'static [u8; 4] {
    match b {
        BlendMode::Multiply => b"mul ",
        BlendMode::Screen => b"scrn",
        BlendMode::Overlay => b"over",
        BlendMode::SoftLight => b"sLit",
        BlendMode::HardLight => b"hLit",
        BlendMode::Add => b"lddg",
        BlendMode::Subtract => b"fsub",
        BlendMode::Darken => b"dark",
        BlendMode::Lighten => b"lite",
        BlendMode::Difference => b"diff",
        BlendMode::Color => b"colr",
        BlendMode::Hue => b"hue ",
        BlendMode::Saturation => b"sat ",
        BlendMode::Luminosity => b"lum ",
        _ => b"norm",
    }
}

/// Camadas desenháveis do corte, de baixo para cima (pastas entram pelos filhos).
fn leaf_layers(p: &Project, parent: &Id, out: &mut Vec<Id>) {
    let Some(n) = p.node(parent) else { return };
    for c in &n.children {
        if let Some(NodeKind::Layer(l)) = p.node(c).map(|x| &x.data) {
            if l.kind == LayerType::Group {
                leaf_layers(p, c, out);
            } else if l.kind.holds_cels() || l.kind == LayerType::Particles {
                out.push(c.clone());
            }
        }
    }
}

struct PsdLayer {
    name: String,
    rect: [i32; 4], // top, left, bottom, right
    planes: [Vec<u8>; 4], // R, G, B, A (ids 0, 1, 2, -1; leitores simples como o do Pillow esperam esta ordem)
    visible: bool,
    blend: BlendMode,
}

fn be16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}
fn be32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// Bytes do PSD (versão 1, RGB 8 bits, sem compressão).
fn psd_bytes(w: u32, h: u32, layers: &[PsdLayer], merged_rgba: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(b"8BPS");
    be16(&mut f, 1);
    f.extend_from_slice(&[0; 6]);
    be16(&mut f, 4); // canais da imagem composta (RGBA)
    be32(&mut f, h);
    be32(&mut f, w);
    be16(&mut f, 8);
    be16(&mut f, 3); // RGB
    be32(&mut f, 0); // modo de cor
    be32(&mut f, 0); // recursos
    // ---- camadas
    let mut info = Vec::new();
    info.extend_from_slice(&(-(layers.len() as i16)).to_be_bytes()); // negativo: o alfa do composto é transparência
    for l in layers {
        for &x in &l.rect {
            info.extend_from_slice(&x.to_be_bytes());
        }
        be16(&mut info, 4);
        for (id, plane) in [0i16, 1, 2, -1].iter().zip(&l.planes) {
            info.extend_from_slice(&id.to_be_bytes());
            be32(&mut info, 2 + plane.len() as u32);
        }
        info.extend_from_slice(b"8BIM");
        info.extend_from_slice(blend_key(l.blend));
        info.push(255); // opacidade (a da camada já está nos pixels)
        info.push(0); // recorte
        info.push(if l.visible { 0 } else { 0x02 }); // bit 1 = oculta
        info.push(0);
        let mut extra = Vec::new();
        be32(&mut extra, 0); // máscara
        be32(&mut extra, 0); // faixas de mistura
        // nome Pascal (ASCII aproximado) com preenchimento a múltiplo de 4
        let ascii: Vec<u8> = l.name.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c as u8 } else { b'_' }).take(255).collect();
        let mut pascal = vec![ascii.len() as u8];
        pascal.extend_from_slice(&ascii);
        while pascal.len() % 4 != 0 {
            pascal.push(0);
        }
        extra.extend_from_slice(&pascal);
        // nome Unicode ("luni")
        let utf16: Vec<u16> = l.name.encode_utf16().collect();
        let mut luni = Vec::new();
        be32(&mut luni, utf16.len() as u32);
        for u in &utf16 {
            be16(&mut luni, *u);
        }
        while luni.len() % 4 != 0 {
            luni.push(0);
        }
        extra.extend_from_slice(b"8BIM");
        extra.extend_from_slice(b"luni");
        be32(&mut extra, luni.len() as u32);
        extra.extend_from_slice(&luni);
        be32(&mut info, extra.len() as u32);
        info.extend_from_slice(&extra);
    }
    for l in layers {
        for plane in &l.planes {
            be16(&mut info, 0); // sem compressão
            info.extend_from_slice(plane);
        }
    }
    if info.len() % 2 != 0 {
        info.push(0);
    }
    let mut lmi = Vec::new();
    be32(&mut lmi, info.len() as u32);
    lmi.extend_from_slice(&info);
    be32(&mut lmi, 0); // máscara global
    be32(&mut f, lmi.len() as u32);
    f.extend_from_slice(&lmi);
    // ---- composto: planos R, G, B, A
    be16(&mut f, 0);
    for ch in [0usize, 1, 2, 3] {
        f.extend(merged_rgba.chunks_exact(4).map(|p| p[ch]));
    }
    f
}

pub fn export_psd(p: &Project, blobs: &BlobStore, req: &ExportPsdRequest) -> ToolResult<Value> {
    let cd = cut_data(p, &req.cut)?;
    if req.frame >= cd.duration {
        return Err(ToolError::Invalid(format!("quadro {} fora do corte (duração {})", req.frame, cd.duration)));
    }
    let mut ids = Vec::new();
    leaf_layers(p, &req.cut, &mut ids);
    let base = RenderOptions { camera: req.camera, include_guides: false, include_reference: false, ..Default::default() };
    let merged = render_frame(p, blobs, &req.cut, req.frame, &base).map_err(|e| ToolError::Render(e.to_string()))?.image;
    let (w, h) = (merged.width, merged.height);
    let mut layers = Vec::new();
    for id in &ids {
        let node = node(p, id)?;
        let NodeKind::Layer(l) = &node.data else { continue };
        if matches!(l.kind, LayerType::Guide | LayerType::Reference) {
            continue;
        }
        let opts = RenderOptions { only_layers: Some(vec![id.clone()]), ..base.clone() };
        let img = render_frame(p, blobs, &req.cut, req.frame, &opts).map_err(|e| ToolError::Render(e.to_string()))?.image;
        let rgba = img.to_rgba8();
        // caixa do conteúdo
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
        for y in 0..h {
            for x in 0..w {
                if rgba[((y * w + x) * 4 + 3) as usize] > 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        let mut planes: [Vec<u8>; 4] = Default::default();
        if x0 < x1 {
            for (k, ch) in [0usize, 1, 2, 3].iter().enumerate() {
                for y in y0..y1 {
                    for x in x0..x1 {
                        planes[k].push(rgba[((y * w + x) * 4) as usize + ch]);
                    }
                }
            }
        } else {
            (x0, y0, x1, y1) = (0, 0, 0, 0);
        }
        layers.push(PsdLayer { name: node.name.clone(), rect: [y0 as i32, x0 as i32, y1 as i32, x1 as i32], planes, visible: l.visible, blend: l.blend });
    }
    let bytes = psd_bytes(w, h, &layers, &merged.to_rgba8());
    std::fs::write(&req.out, &bytes).map_err(|e| ToolError::Invalid(format!("não consegui gravar {}: {e}", req.out)))?;
    Ok(json!({ "path": req.out, "width": w, "height": h, "layers": layers.len(), "bytes": bytes.len() }))
}

pub fn export_xdts(p: &Project, req: &ExportXdtsRequest) -> ToolResult<Value> {
    let cut_node = node(p, &req.cut)?;
    let cd = cut_data(p, &req.cut)?;
    let scene = cut_node.parent.as_ref().and_then(|s| p.node(s)).and_then(|n| match &n.data {
        NodeKind::Scene(s) => Some(s.number),
        _ => None,
    });
    // campo 0: uma trilha por camada com timesheet (de baixo para cima); camadas sem trilha mostram o cel fixo
    let mut ids = Vec::new();
    leaf_layers(p, &req.cut, &mut ids);
    let mut names = Vec::new();
    let mut tracks = Vec::new();
    for id in &ids {
        let n = node(p, id)?;
        let NodeKind::Layer(l) = &n.data else { continue };
        if !matches!(l.kind, LayerType::Vector | LayerType::Raster | LayerType::Fill) {
            continue;
        }
        let at = |f: u32| -> Option<String> {
            match cd.xsheet.track_for_layer(id) {
                Some(t) => match t.exposed_at(f) {
                    Some(CellRef::Cel { number }) => Some(number.to_string()),
                    _ => None,
                },
                None => exposed_cel(p, cd, id, f).and_then(|c| match &c.data {
                    NodeKind::Cel(c) => Some(c.number.to_string()),
                    _ => None,
                }),
            }
        };
        let mut frames = Vec::new();
        let mut last: Option<Option<String>> = None;
        for f in 0..cd.duration {
            let v = at(f);
            if last.as_ref() != Some(&v) {
                frames.push(json!({ "frame": f, "data": [{ "id": 0, "values": [v.clone().unwrap_or_else(|| "SYMBOL_NULL_CELL".into())] }] }));
                last = Some(v);
            }
        }
        // fim do corte: célula vazia depois do último quadro (convenção do formato para fechar a exposição)
        frames.push(json!({ "frame": cd.duration, "data": [{ "id": 0, "values": ["SYMBOL_NULL_CELL"] }] }));
        tracks.push(json!({ "trackNo": names.len(), "frames": frames }));
        names.push(n.name.clone());
    }
    let mut fields = vec![json!({ "fieldId": 0, "tracks": tracks })];
    let mut headers = vec![json!({ "fieldId": 0, "names": names })];
    // campo 3: falas dos clipes de diálogo (quem fala + texto; SYMBOL_HYPHEN até o fim da fala)
    let fps = p.settings.fps();
    let mut speech = Vec::new();
    for a in cd.audio.iter().filter(|a| a.track == AudioTrack::Dialogue) {
        let Some(NodeKind::Asset(asset)) = p.node(&a.asset).map(|n| &n.data) else { continue };
        let text = asset.params.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        let who = a.character.as_ref().and_then(|c| p.node(c)).map(|n| n.name.clone()).unwrap_or_default();
        let secs = a.duration.map(|d| d as f64).or_else(|| asset.params.get("duration_s").and_then(Value::as_f64)).unwrap_or(1.0);
        let len = ((secs * fps).round() as u32).max(1);
        let mut frames = vec![json!({ "frame": a.start_frame, "data": [{ "id": 0, "values": [who, text] }] })];
        for f in a.start_frame + 1..(a.start_frame + len).min(cd.duration) {
            frames.push(json!({ "frame": f, "data": [{ "id": 0, "values": ["SYMBOL_HYPHEN"] }] }));
        }
        speech.push(frames);
    }
    if !speech.is_empty() {
        let tracks: Vec<Value> = speech.into_iter().enumerate().map(|(i, frames)| json!({ "trackNo": i, "frames": frames })).collect();
        let n = tracks.len();
        fields.push(json!({ "fieldId": 3, "tracks": tracks }));
        headers.push(json!({ "fieldId": 3, "names": (0..n).map(|i| format!("S{}", i + 1)).collect::<Vec<_>>() }));
    }
    let doc = json!({
        "header": { "cut": (cd.number.max(1) % 10000).to_string(), "scene": (scene.unwrap_or(1).max(1) % 10000).to_string() },
        "timeTables": [{ "duration": cd.duration, "name": cut_node.name, "fields": fields, "timeTableHeaders": headers }],
        "version": 5
    });
    let text = format!("exchangeDigitalTimeSheet Save Data\n{}", serde_json::to_string(&doc).map_err(|e| ToolError::Invalid(e.to_string()))?);
    std::fs::write(&req.out, &text).map_err(|e| ToolError::Invalid(format!("não consegui gravar {}: {e}", req.out)))?;
    Ok(json!({ "path": req.out, "tracks": headers[0]["names"].as_array().map(|a| a.len()).unwrap_or(0), "duration": cd.duration }))
}
