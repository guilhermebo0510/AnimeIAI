//! Exportações para outros programas de estúdio:
//! - PSD em camadas (8 bits RGBA): cada camada desenhável do corte renderizada sozinha, recortada no conteúdo, com
//!   nome (inclusive Unicode), visibilidade e modo de mistura, mais a imagem composta.
//! - XDTS (Exchange Digital TimeSheet, formato público da Celsys/Toei, versão 5): timesheet de células por camada
//!   (campo 0) e falas (campo 3), para abrir no Clip Studio Paint EX, TVPaint, OpenToonz.
//! - SVG de um quadro: camadas vetoriais como caminhos (contorno do motor), as outras como imagem.
//! - OTIO (OpenTimelineIO): a edição com transições, créditos, legendas e áudio, para editores de vídeo.

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

/// Nomes das camadas de efeito/ajuste do corte (agem sobre o composto de baixo; não viram camada própria no PSD/SVG).
fn effect_layers(p: &Project, parent: &Id, out: &mut Vec<String>) {
    let Some(n) = p.node(parent) else { return };
    for c in &n.children {
        if let Some(NodeKind::Layer(l)) = p.node(c).map(|x| &x.data) {
            match l.kind {
                LayerType::Group => effect_layers(p, c, out),
                LayerType::Effect | LayerType::Adjustment if l.visible => out.push(p.node(c).map(|x| x.name.clone()).unwrap_or_default()),
                _ => {}
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

// ---------------------------------------------------------------------------------------------------------------
// SVG

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportSvgRequest {
    pub cut: Id,
    #[serde(default)]
    pub frame: u32,
    /// Caminho absoluto do .svg.
    pub out: String,
}

fn svg_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn svg_blend(b: BlendMode) -> Option<&'static str> {
    Some(match b {
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::SoftLight => "soft-light",
        BlendMode::HardLight => "hard-light",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::Difference => "difference",
        BlendMode::Color => "color",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Luminosity => "luminosity",
        BlendMode::Add => "plus-lighter",
        _ => return None,
    })
}

fn hex(c: [f32; 4]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

fn rdp_keep(p: &[[f32; 2]], a: usize, b: usize, tol: f32, keep: &mut [bool]) {
    if b <= a + 1 {
        return;
    }
    let (ax, ay, bx, by) = (p[a][0], p[a][1], p[b][0], p[b][1]);
    let (dx, dy) = (bx - ax, by - ay);
    let l2 = dx * dx + dy * dy;
    let (mut best, mut idx) = (0.0f32, a);
    for (i, q) in p.iter().enumerate().take(b).skip(a + 1) {
        let t = if l2 > 0.0 { (((q[0] - ax) * dx + (q[1] - ay) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let d = ((q[0] - ax - dx * t).powi(2) + (q[1] - ay - dy * t).powi(2)).sqrt();
        if d > best {
            best = d;
            idx = i;
        }
    }
    if best > tol {
        keep[idx] = true;
        rdp_keep(p, a, idx, tol, keep);
        rdp_keep(p, idx, b, tol, keep);
    }
}

fn simplify(p: Vec<[f32; 2]>, tol: f32) -> Vec<[f32; 2]> {
    if p.len() < 3 {
        return p;
    }
    let mut keep = vec![false; p.len()];
    keep[0] = true;
    *keep.last_mut().unwrap() = true;
    rdp_keep(&p, 0, p.len() - 1, tol, &mut keep);
    p.into_iter().zip(keep).filter(|(_, k)| *k).map(|(q, _)| q).collect()
}

/// Contornos (polígonos fechados, regra não-zero) da área que o motor pinta para o traço: as duas bordas paralelas
/// à linha central (deslocadas pelo raio de cada amostra) e as pontas redondas. Na borda de dentro de uma curva
/// fechada demais o deslocamento volta para trás (rabo de andorinha); esses pontos saem, senão viram furo.
pub fn stroke_outline(samples: &[aiai_paint::StrokeSample], closed: bool) -> Vec<Vec<[f32; 2]>> {
    let n = samples.len();
    if n == 0 {
        return Vec::new();
    }
    let circle = |q: &aiai_paint::StrokeSample| -> Vec<[f32; 2]> {
        let k = ((q.r * 2.0).ceil() as usize).clamp(8, 64);
        (0..k).map(|i| {
            let a = i as f32 / k as f32 * std::f32::consts::TAU;
            [q.x + q.r * a.cos(), q.y + q.r * a.sin()]
        }).collect()
    };
    if n == 1 {
        return vec![circle(&samples[0])];
    }
    let tangent = |i: usize| -> [f32; 2] {
        let (a, b) = if closed { (samples[(i + n - 1) % n], samples[(i + 1) % n]) } else { (samples[i.saturating_sub(1)], samples[(i + 1).min(n - 1)]) };
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        [dx / l, dy / l]
    };
    let side = |sign: f32| -> Vec<[f32; 2]> {
        let mut out: Vec<[f32; 2]> = Vec::with_capacity(n);
        for (i, q) in samples.iter().enumerate() {
            let t = tangent(i);
            let p = [q.x - t[1] * q.r * sign, q.y + t[0] * q.r * sign];
            if let Some(last) = out.last() {
                // não anda para trás em relação à direção do traço
                if (p[0] - last[0]) * t[0] + (p[1] - last[1]) * t[1] < -1e-3 {
                    continue;
                }
            }
            out.push(p);
        }
        out
    };
    let (left, mut right) = (side(1.0), side(-1.0));
    if closed {
        right.reverse();
        return vec![simplify(left, 0.04), simplify(right, 0.04)];
    }
    // ponta redonda: meia volta de +normal a -normal passando pela frente (t), no sentido do contorno
    let cap = |q: &aiai_paint::StrokeSample, t: [f32; 2]| -> Vec<[f32; 2]> {
        let k = ((q.r * 1.5).ceil() as usize).clamp(4, 32);
        let base = t[1].atan2(t[0]) + std::f32::consts::FRAC_PI_2;
        (1..k).map(|i| {
            let a = base - (i as f32 / k as f32) * std::f32::consts::PI;
            [q.x + q.r * a.cos(), q.y + q.r * a.sin()]
        }).collect()
    };
    let mut ring = left;
    ring.extend(cap(&samples[n - 1], tangent(n - 1)));
    right.reverse();
    ring.extend(right);
    let t0 = tangent(0);
    ring.extend(cap(&samples[0], [-t0[0], -t0[1]]));
    vec![simplify(ring, 0.04)]
}

fn path_d(contours: &[Vec<[f32; 2]>]) -> String {
    let mut d = String::new();
    for c in contours.iter().filter(|c| c.len() >= 3) {
        for (i, p) in c.iter().enumerate() {
            d.push_str(&format!("{}{:.2} {:.2}", if i == 0 { "M" } else { "L" }, p[0], p[1]));
        }
        d.push('Z');
    }
    d
}

/// A camada sozinha no quadro, sem câmera, a 100% de opacidade, modo normal e visível (as propriedades vão na <g>).
fn solo_render(p: &Project, blobs: &BlobStore, cut: &Id, frame: u32, id: &Id) -> ToolResult<aiai_paint::Image> {
    let mut solo = p.clone();
    if let Some(NodeKind::Layer(sl)) = solo.node_mut(id).map(|n| &mut n.data) {
        sl.opacity = 1.0;
        sl.keys.0.remove("opacity");
        sl.blend = BlendMode::Normal;
        sl.visible = true;
    }
    let opts = RenderOptions { only_layers: Some(vec![id.clone()]), camera: false, include_guides: false, include_reference: false, ..Default::default() };
    Ok(render_frame(&solo, blobs, cut, frame, &opts).map_err(|e| ToolError::Render(e.to_string()))?.image)
}

fn png_bytes(rgba: &[u8], w: u32, h: u32) -> ToolResult<Vec<u8>> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out).write_image(rgba, w, h, image::ExtendedColorType::Rgba8).map_err(|e| ToolError::Invalid(format!("png: {e}")))?;
    Ok(out)
}

/// Quadro do corte em SVG: camada vetorial vira caminhos (o contorno exato que o motor pinta, cor resolvida da
/// paleta, posição com o movimento da camada); as outras camadas (raster, cor, partículas...) entram como imagem
/// PNG recortada no conteúdo. Uma `<g>` por camada, com nome, visibilidade, opacidade e modo de mistura.
pub fn export_svg(p: &Project, blobs: &BlobStore, req: &ExportSvgRequest) -> ToolResult<Value> {
    use base64::Engine;
    let cd = cut_data(p, &req.cut)?;
    if req.frame >= cd.duration {
        return Err(ToolError::Invalid(format!("quadro {} fora do corte (duração {})", req.frame, cd.duration)));
    }
    let (w, h) = aiai_render::cut_canvas(p, cd);
    let mut ids = Vec::new();
    leaf_layers(p, &req.cut, &mut ids);
    let mut body = String::new();
    let mut fx = Vec::new();
    effect_layers(p, &req.cut, &mut fx);
    let mut warnings: Vec<String> = fx.iter().map(|n| format!("{n}: camada de efeito/ajuste age no composto e não vai para o SVG")).collect();
    let (mut n_paths, mut n_images, mut n_groups) = (0usize, 0usize, 0usize);
    for id in &ids {
        let node = node(p, id)?;
        let NodeKind::Layer(l) = &node.data else { continue };
        if matches!(l.kind, LayerType::Guide | LayerType::Reference) {
            continue;
        }
        let opacity = l.keys.eval("opacity", req.frame as f32).unwrap_or(l.opacity);
        let mut style = String::new();
        if !l.visible {
            style.push_str("display:none;");
        }
        if let Some(b) = svg_blend(l.blend) {
            style.push_str(&format!("mix-blend-mode:{b};"));
        }
        if l.kind == LayerType::Vector && (l.clip_to_below || l.mask.is_some()) {
            warnings.push(format!("{}: recorte/máscara de camada vetorial não vai para o SVG", node.name));
        }
        let mut g = String::new();
        let cel = exposed_cel(p, cd, id, req.frame).and_then(|c| match &c.data {
            NodeKind::Cel(c) => Some(c),
            _ => None,
        });
        if l.kind == LayerType::Vector {
            let Some(cel) = cel else { continue };
            if !cel.fills.is_empty() || !cel.tiles.is_empty() {
                warnings.push(format!("{}: preenchimentos/pixels dentro do cel vetorial não vão para o SVG", node.name));
            }
            let a = aiai_render::compose::layer_placement(l, cel.transform, req.frame, w, h);
            if !a.is_identity() {
                let m = a.0;
                let num = |v: f32| {
                    let r = (v * 1e4).round() / 1e4;
                    format!("{}", if r == 0.0 { 0.0 } else { r })
                };
                g.push_str(&format!("<g transform=\"matrix({} {} {} {} {} {})\">", num(m[0]), num(m[3]), num(m[1]), num(m[4]), num(m[2]), num(m[5])));
            }
            for s in &cel.strokes {
                let color = crate::paint::resolve_color(p, &s.color)?;
                let samples = aiai_paint::sample_stroke(s);
                let d = path_d(&stroke_outline(&samples, s.closed));
                if d.is_empty() {
                    continue;
                }
                let alpha = color[3] * s.opacity;
                let fo = if alpha < 0.999 { format!(" fill-opacity=\"{alpha:.3}\"") } else { String::new() };
                g.push_str(&format!("<path d=\"{d}\" fill=\"{}\"{fo}/>", hex(color)));
                n_paths += 1;
            }
            if !a.is_identity() {
                g.push_str("</g>");
            }
        } else {
            // a opacidade vai na <g>; a imagem sai com a camada a 100% (a máscara da camada o render já aplica)
            let mut img = solo_render(p, blobs, &req.cut, req.frame, id)?;
            if l.clip_to_below {
                // recorte na camada de baixo: o render sozinho não tem a base, então o alfa da base entra aqui
                let pos = ids.iter().position(|x| x == id).unwrap_or(0);
                let base = ids[..pos].iter().rev().find(|b| matches!(p.node(b).map(|n| &n.data), Some(NodeKind::Layer(bl)) if !bl.clip_to_below));
                match base {
                    Some(b) => {
                        let bimg = solo_render(p, blobs, &req.cut, req.frame, b)?;
                        for (px, bp) in img.data.iter_mut().zip(&bimg.data) {
                            for c in px.iter_mut() {
                                *c *= bp[3];
                            }
                        }
                    }
                    None => warnings.push(format!("{}: recorte sem camada de baixo", node.name)),
                }
            }
            let rgba = img.to_rgba8();
            let (iw, ih) = (img.width, img.height);
            let (mut x0, mut y0, mut x1, mut y1) = (iw, ih, 0u32, 0u32);
            for y in 0..ih {
                for x in 0..iw {
                    if rgba[((y * iw + x) * 4 + 3) as usize] > 0 {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x + 1);
                        y1 = y1.max(y + 1);
                    }
                }
            }
            if x0 >= x1 {
                continue;
            }
            let (cw, ch) = (x1 - x0, y1 - y0);
            let mut crop = Vec::with_capacity((cw * ch * 4) as usize);
            for y in y0..y1 {
                let a = ((y * iw + x0) * 4) as usize;
                crop.extend_from_slice(&rgba[a..a + (cw * 4) as usize]);
            }
            let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes(&crop, cw, ch)?);
            g.push_str(&format!("<image x=\"{x0}\" y=\"{y0}\" width=\"{cw}\" height=\"{ch}\" href=\"data:image/png;base64,{b64}\"/>"));
            n_images += 1;
        }
        let op = if opacity < 0.999 { format!(" opacity=\"{opacity:.3}\"") } else { String::new() };
        let st = if style.is_empty() { String::new() } else { format!(" style=\"{style}\"") };
        let name = svg_esc(&node.name);
        body.push_str(&format!("<g id=\"layer-{}\" inkscape:groupmode=\"layer\" inkscape:label=\"{name}\"{op}{st}><title>{name}</title>{g}</g>\n", n_groups + 1));
        n_groups += 1;
    }
    let svg = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n<title>{}</title>\n{body}</svg>\n",
        svg_esc(&node(p, &req.cut)?.name)
    );
    std::fs::write(&req.out, &svg).map_err(|e| ToolError::Invalid(format!("não consegui gravar {}: {e}", req.out)))?;
    Ok(json!({ "path": req.out, "width": w, "height": h, "layers": n_groups, "paths": n_paths, "images": n_images, "bytes": svg.len(), "warnings": warnings }))
}

// ---------------------------------------------------------------------------------------------------------------
// OTIO (OpenTimelineIO, JSON .otio)

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportOtioRequest {
    /// Documento de edição.
    pub edit: Id,
    /// Caminho absoluto do .otio.
    pub out: String,
    /// Pasta para a mídia: as falas/efeitos/trilhas (WAV) são gravados aqui e ligados aos clipes de áudio; sem
    /// pasta, os clipes ficam com referência ausente (MissingReference) e o id do asset nos metadados.
    #[serde(default)]
    pub media_dir: Option<String>,
    /// Com `media_dir`: renderiza cada clipe de vídeo (o trecho do corte usado, com as sobras das transições) como
    /// sequência PNG e liga ao clipe (ImageSequenceReference). Demora: um render por quadro.
    #[serde(default)]
    pub render_video: bool,
}

fn rt(v: f64, rate: f64) -> Value {
    json!({ "OTIO_SCHEMA": "RationalTime.1", "rate": rate, "value": v })
}

fn tr(start: f64, dur: f64, rate: f64) -> Value {
    json!({ "OTIO_SCHEMA": "TimeRange.1", "start_time": rt(start, rate), "duration": rt(dur, rate) })
}

fn otio_clip(name: &str, range: Value, media: Value, metadata: Value) -> Value {
    json!({
        "OTIO_SCHEMA": "Clip.2", "name": name, "metadata": metadata, "source_range": range, "effects": [], "markers": [],
        "enabled": true, "media_references": { "DEFAULT_MEDIA": media }, "active_media_reference_key": "DEFAULT_MEDIA"
    })
}

fn otio_gap(dur: f64, rate: f64) -> Value {
    json!({ "OTIO_SCHEMA": "Gap.1", "name": "", "metadata": {}, "source_range": tr(0.0, dur, rate), "effects": [], "markers": [], "enabled": true })
}

fn missing(metadata: Value) -> Value {
    json!({ "OTIO_SCHEMA": "MissingReference.1", "name": "", "metadata": metadata, "available_range": null, "available_image_bounds": null })
}

fn otio_track(name: &str, kind: &str, children: Vec<Value>, markers: Vec<Value>) -> Value {
    json!({ "OTIO_SCHEMA": "Track.1", "name": name, "kind": kind, "metadata": {}, "source_range": null, "effects": [], "markers": markers, "enabled": true, "children": children })
}

fn file_url(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let enc: String = s.chars().map(|c| match c {
        ' ' => "%20".to_string(),
        '#' => "%23".to_string(),
        '%' => "%25".to_string(),
        c => c.to_string(),
    }).collect();
    if enc.starts_with('/') { format!("file://{enc}") } else { format!("file:///{enc}") }
}

fn safe_name(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

/// Edição em OpenTimelineIO: trilha de vídeo com um clipe por corte (trecho usado), transições (dissolve vira
/// SMPTE_Dissolve com o ponto de corte no meio da sobreposição; as outras, Custom_Transition com o tipo nos
/// metadados), créditos como gerador, legendas como marcadores e trilhas de áudio por tipo (falas, efeitos, música,
/// ambiente; uma faixa extra quando dois clipes se sobrepõem). Tempo em quadros do projeto.
pub fn export_otio(p: &Project, blobs: &BlobStore, req: &ExportOtioRequest) -> ToolResult<Value> {
    let doc = crate::edit::load_edit(p, &req.edit)?;
    let fps = p.settings.fps();
    let tl = doc.timeline(p, fps as f32)?;
    let media_dir = req.media_dir.as_ref().map(std::path::PathBuf::from);
    if let Some(d) = &media_dir {
        std::fs::create_dir_all(d).map_err(|e| ToolError::Invalid(format!("pasta de mídia {}: {e}", d.display())))?;
    }
    if req.render_video && media_dir.is_none() {
        return Err(ToolError::Invalid("render_video precisa de media_dir".into()));
    }
    // a primeira trilha de vídeo vista como a sequência com sobreposição do v1 (o OTIO entende transições assim)
    let view = doc.v1_view();
    let n = view.len();
    // ponto de corte das sobreposições: metade antes (entrada do clipe novo), metade depois (saída do anterior)
    let mut in_off = vec![0u32; n];
    let mut out_off = vec![0u32; n];
    for k in 1..n {
        let overlap = (view[k - 1].1.0 + view[k - 1].1.1).saturating_sub(view[k].1.0);
        out_off[k - 1] = overlap / 2;
        in_off[k] = overlap - overlap / 2;
    }
    let mut video = Vec::new();
    let mut rendered = 0usize;
    let mut global = vec![0u32; n]; // início (já aparado) de cada clipe na linha do tempo
    let mut t = 0u32;
    for (k, (cut, (_, dur, off), transition_in)) in view.iter().enumerate() {
        let (dur, off) = (*dur, *off);
        let cut_name = node(p, cut)?.name.clone();
        let used = dur - in_off[k] - out_off[k];
        global[k] = t;
        t += used;
        if let Some(trn) = transition_in.as_ref().filter(|_| k > 0) {
            let (kind, name) = match trn.kind {
                TransitionKindRef::Dissolve => ("SMPTE_Dissolve", "dissolve"),
                TransitionKindRef::Cut => ("", ""),
                _ => ("Custom_Transition", "custom"),
            };
            if !kind.is_empty() {
                video.push(json!({
                    "OTIO_SCHEMA": "Transition.1", "name": name, "transition_type": kind,
                    "metadata": { "aiai": { "kind": trn.kind, "frames": trn.frames } },
                    "in_offset": rt(in_off[k] as f64, fps), "out_offset": rt(out_off[k - 1] as f64, fps)
                }));
            }
        }
        let meta = json!({ "aiai": { "cut": cut, "cut_name": cut_name, "transition_in": transition_in } });
        let media = match (&media_dir, req.render_video) {
            (Some(dir), true) => {
                let sub = dir.join(format!("{:02}_{}", k + 1, safe_name(&cut_name)));
                std::fs::create_dir_all(&sub).map_err(|e| ToolError::Invalid(format!("{}: {e}", sub.display())))?;
                for f in off..off + dur {
                    let opts = RenderOptions { camera: true, ..Default::default() };
                    let img = render_frame(p, blobs, cut, f, &opts).map_err(|e| ToolError::Render(e.to_string()))?.image;
                    img.save_png(&sub.join(format!("f.{f:04}.png"))).map_err(ToolError::Invalid)?;
                    rendered += 1;
                }
                json!({
                    "OTIO_SCHEMA": "ImageSequenceReference.1", "name": cut_name, "metadata": {},
                    "target_url_base": format!("{}/", file_url(&sub)), "name_prefix": "f.", "name_suffix": ".png",
                    "start_frame": off, "frame_step": 1, "rate": fps, "frame_zero_padding": 4, "missing_frame_policy": "error",
                    "available_range": tr(off as f64, dur as f64, fps), "available_image_bounds": null
                })
            }
            _ => missing(json!({ "aiai": { "cut": cut } })),
        };
        video.push(otio_clip(&cut_name, tr((off + in_off[k]) as f64, used as f64, fps), media, meta));
    }
    if tl.total > tl.credits_start {
        let credits_ref = json!({
            "OTIO_SCHEMA": "GeneratorReference.1", "name": "créditos", "metadata": {}, "generator_kind": "aiai_credits",
            "parameters": { "lines": doc.credits }, "available_range": null, "available_image_bounds": null
        });
        video.push(otio_clip("Créditos", tr(0.0, (tl.total - tl.credits_start) as f64, fps), credits_ref, json!({})));
    }
    // legendas (todas as trilhas S) como marcadores
    let markers: Vec<Value> = doc.tracks_of(crate::edit::TrackKind::Subtitle).iter().flat_map(|t| doc.items_on(t.id)).filter_map(|i| i.caption_text().map(|text| json!({
        "OTIO_SCHEMA": "Marker.2", "name": text, "metadata": { "aiai": { "subtitle": true } }, "color": "YELLOW", "comment": "",
        "marked_range": tr(i.start as f64, i.dur as f64, fps)
    }))).collect();
    let mut tracks = vec![otio_track("V1", "Video", video, markers)];

    // áudio: cada clipe de áudio dos cortes, no trecho aparado de cada clipe da edição
    struct Item {
        start: f64,
        dur: f64,
        src: f64,
        clip: Value,
    }
    let mut by_kind: Vec<(AudioTrack, Vec<Item>)> = Vec::new();
    let mut written = std::collections::HashSet::new();
    for (k, (cut, (_, dur, off), _)) in view.iter().enumerate() {
        let (dur, off) = (*dur, *off);
        let (lo, hi) = ((off + in_off[k]) as f64, (off + dur - out_off[k]) as f64);
        let cd = cut_data(p, cut)?;
        for a in &cd.audio {
            let Some(an) = p.node(&a.asset) else { continue };
            let NodeKind::Asset(asset) = &an.data else { continue };
            let full_s = asset.params.get("duration_s").and_then(Value::as_f64);
            let secs = a.duration.map(|d| d as f64).or(full_s.map(|f| f - a.trim_start as f64)).unwrap_or(1.0);
            let (a0, a1) = (a.start_frame as f64, a.start_frame as f64 + secs * fps);
            let (s0, s1) = (a0.max(lo), a1.min(hi));
            if s1 <= s0 {
                continue;
            }
            let src = a.trim_start as f64 * fps + (s0 - a0);
            let media = match (&media_dir, &asset.blob) {
                (Some(dir), Some(hash)) => {
                    let path = dir.join(format!("{}_{}.wav", safe_name(&an.name), &a.asset.0[a.asset.0.len().saturating_sub(8)..]));
                    if written.insert(path.clone()) {
                        let bytes = blobs.get(hash).ok_or_else(|| ToolError::NotFound(format!("blob do áudio {}", an.name)))?;
                        std::fs::write(&path, &bytes[..]).map_err(|e| ToolError::Invalid(format!("{}: {e}", path.display())))?;
                    }
                    json!({
                        "OTIO_SCHEMA": "ExternalReference.1", "name": an.name, "metadata": {}, "target_url": file_url(&path),
                        "available_range": full_s.map(|f| tr(0.0, f * fps, fps)), "available_image_bounds": null
                    })
                }
                _ => missing(json!({ "aiai": { "asset": a.asset } })),
            };
            let meta = json!({ "aiai": { "asset": a.asset, "gain_db": a.gain_db, "pan": a.pan, "fade_in": a.fade_in, "fade_out": a.fade_out,
                                         "character": a.character, "text": asset.params.get("text") } });
            let item = Item { start: global[k] as f64 + (s0 - lo), dur: s1 - s0, src, clip: otio_clip(&an.name, tr(src, s1 - s0, fps), media, meta) };
            match by_kind.iter_mut().find(|(t, _)| *t == a.track) {
                Some((_, v)) => v.push(item),
                None => by_kind.push((a.track, vec![item])),
            }
        }
    }
    let mut audio_clips = 0usize;
    for (kind, mut items) in by_kind {
        items.sort_by(|a, b| a.start.total_cmp(&b.start));
        // faixas sem sobreposição (a primeira que couber)
        let mut lanes: Vec<(f64, Vec<Value>)> = Vec::new();
        for it in items {
            let lane = match lanes.iter().position(|(end, _)| *end <= it.start + 1e-6) {
                Some(i) => i,
                None => {
                    lanes.push((0.0, Vec::new()));
                    lanes.len() - 1
                }
            };
            let (end, v) = &mut lanes[lane];
            if it.start > *end + 1e-6 {
                v.push(otio_gap(it.start - *end, fps));
            }
            let _ = it.src;
            v.push(it.clip);
            *end = it.start + it.dur;
            audio_clips += 1;
        }
        let label = match kind {
            AudioTrack::Dialogue => "Falas",
            AudioTrack::Sfx => "Efeitos",
            AudioTrack::Bgm => "Música",
            AudioTrack::Ambience => "Ambiente",
        };
        for (i, (_, v)) in lanes.into_iter().enumerate() {
            tracks.push(otio_track(&format!("A {label} {}", i + 1), "Audio", v, Vec::new()));
        }
    }
    let name = node(p, &req.edit)?.name.clone();
    let doc_json = json!({
        "OTIO_SCHEMA": "Timeline.1", "name": name,
        "metadata": { "aiai": { "edit": req.edit, "fade_out_end": doc.fade_out_end, "width": p.settings.width, "height": p.settings.height } },
        "global_start_time": rt(0.0, fps),
        "tracks": { "OTIO_SCHEMA": "Stack.1", "name": "tracks", "metadata": {}, "source_range": null, "effects": [], "markers": [], "enabled": true, "children": tracks }
    });
    let text = serde_json::to_string_pretty(&doc_json).map_err(|e| ToolError::Invalid(e.to_string()))?;
    std::fs::write(&req.out, &text).map_err(|e| ToolError::Invalid(format!("não consegui gravar {}: {e}", req.out)))?;
    Ok(json!({ "path": req.out, "clips": n, "frames": tl.total, "audio_clips": audio_clips, "tracks": doc_json["tracks"]["children"].as_array().map(|a| a.len()), "rendered_frames": rendered }))
}

use crate::edit::TransitionKind as TransitionKindRef;
