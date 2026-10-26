//! Importar PSD e ORA em camadas (auditoria 4.2d): cada camada de pixels do arquivo vira uma camada raster do corte,
//! com nome, visibilidade, opacidade e modo de mistura. Leitores próprios: PSD RGB de 8 bits (canais sem compressão ou
//! RLE/PackBits, máscaras ignoradas, pastas achatadas na ordem) e ORA (ZIP com `stack.xml` e PNGs; ZIP guardado ou
//! deflate). Os pixels ficam no lugar do arquivo (canto superior esquerdo do canvas).

use crate::context::*;
use crate::paint::tiles_for_region;
use aiai_core::*;
use aiai_paint::Image;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Camada lida, já no tamanho do documento (de baixo para cima).
#[derive(Debug, Clone)]
pub struct ImportedLayer {
    pub name: String,
    pub image: Image,
    pub opacity: f32,
    pub visible: bool,
    pub blend: BlendMode,
}

#[derive(Debug, Clone)]
pub struct ImportedDoc {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<ImportedLayer>,
}

fn blend_of(key: &[u8]) -> BlendMode {
    match key {
        b"mul " => BlendMode::Multiply,
        b"scrn" => BlendMode::Screen,
        b"over" => BlendMode::Overlay,
        b"sLit" => BlendMode::SoftLight,
        b"hLit" => BlendMode::HardLight,
        b"lddg" => BlendMode::Add,
        b"fsub" => BlendMode::Subtract,
        b"dark" => BlendMode::Darken,
        b"lite" => BlendMode::Lighten,
        b"diff" => BlendMode::Difference,
        b"colr" => BlendMode::Color,
        b"hue " => BlendMode::Hue,
        b"sat " => BlendMode::Saturation,
        b"lum " => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
}

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.i.checked_add(n).filter(|e| *e <= self.b.len()).ok_or("arquivo cortado (PSD)")?;
        let s = &self.b[self.i..end];
        self.i = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
}

/// PackBits de uma linha.
fn unpack(src: &[u8], out_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(out_len);
    let mut i = 0;
    while i < src.len() && out.len() < out_len {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let k = n as usize + 1;
            out.extend_from_slice(&src[i..(i + k).min(src.len())]);
            i += k;
        } else if n != -128 {
            let k = (-(n as i32)) as usize + 1;
            if let Some(&v) = src.get(i) {
                out.extend(std::iter::repeat_n(v, k));
            }
            i += 1;
        }
    }
    out.resize(out_len, 0);
    out
}

/// Lê um PSD (RGB, 8 bits). Pastas viram só a ordem das camadas de dentro.
pub fn read_psd(bytes: &[u8]) -> Result<ImportedDoc, String> {
    let mut r = Rd { b: bytes, i: 0 };
    if r.take(4)? != b"8BPS" {
        return Err("não é um PSD (assinatura 8BPS)".into());
    }
    if r.u16()? != 1 {
        return Err("PSB (documento grande) não é suportado".into());
    }
    r.take(6)?;
    let _channels = r.u16()?;
    let h = r.u32()?;
    let w = r.u32()?;
    let depth = r.u16()?;
    let mode = r.u16()?;
    if depth != 8 || mode != 3 {
        return Err(format!("só PSD RGB de 8 bits (este tem {depth} bits, modo {mode})"));
    }
    let n = r.u32()? as usize;
    r.take(n)?; // dados do modo de cor
    let n = r.u32()? as usize;
    r.take(n)?; // recursos
    let lmi_len = r.u32()? as usize;
    let lmi_end = r.i + lmi_len;
    let mut layers = Vec::new();
    if lmi_len > 0 {
        let info_len = r.u32()? as usize;
        if info_len > 0 {
            let count = r.i16()?.unsigned_abs() as usize;
            struct Rec {
                rect: [i32; 4],
                chans: Vec<(i16, u32)>,
                blend: BlendMode,
                opacity: u8,
                visible: bool,
                name: String,
                group_marker: bool,
            }
            let mut recs = Vec::with_capacity(count);
            for _ in 0..count {
                let rect = [r.i32()?, r.i32()?, r.i32()?, r.i32()?];
                let nc = r.u16()? as usize;
                let mut chans = Vec::with_capacity(nc);
                for _ in 0..nc {
                    chans.push((r.i16()?, r.u32()?));
                }
                if r.take(4)? != b"8BIM" {
                    return Err("registro de camada inválido".into());
                }
                let blend = blend_of(r.take(4)?);
                let opacity = r.u8()?;
                let _clip = r.u8()?;
                let flags = r.u8()?;
                r.u8()?;
                let extra = r.u32()? as usize;
                let extra_end = r.i + extra;
                let m = r.u32()? as usize;
                r.take(m)?;
                let br = r.u32()? as usize;
                r.take(br)?;
                let nl = r.u8()? as usize;
                let mut name = String::from_utf8_lossy(r.take(nl)?).to_string();
                let pad = (4 - (nl + 1) % 4) % 4;
                r.take(pad)?;
                let mut group_marker = false;
                // blocos adicionais: nome unicode (luni) e marcador de pasta (lsct)
                while r.i + 12 <= extra_end {
                    let sig = r.take(4)?;
                    if sig != b"8BIM" && sig != b"8B64" {
                        break;
                    }
                    let key: [u8; 4] = r.take(4)?.try_into().unwrap_or([0; 4]);
                    let len = r.u32()? as usize;
                    let body = r.take(len.min(extra_end.saturating_sub(r.i)))?;
                    match &key {
                        b"luni" if body.len() >= 4 => {
                            let n = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
                            let units: Vec<u16> = body[4..].chunks_exact(2).take(n).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                            name = String::from_utf16_lossy(&units);
                        }
                        b"lsct" | b"lsdk" if body.len() >= 4 => {
                            let t = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                            group_marker = t != 0;
                        }
                        _ => {}
                    }
                }
                r.i = extra_end;
                recs.push(Rec { rect, chans, blend, opacity, visible: flags & 0x02 == 0, name, group_marker });
            }
            for rec in &recs {
                let [top, left, bottom, right] = rec.rect;
                let (lw, lh) = ((right - left).max(0) as usize, (bottom - top).max(0) as usize);
                let mut planes: std::collections::HashMap<i16, Vec<u8>> = Default::default();
                for &(id, len) in &rec.chans {
                    let data = r.take(len as usize)?;
                    if id < -1 || lw == 0 || lh == 0 || data.len() < 2 {
                        continue;
                    }
                    let comp = u16::from_be_bytes([data[0], data[1]]);
                    let body = &data[2..];
                    let plane = match comp {
                        0 => body.get(..lw * lh).map(|s| s.to_vec()).ok_or("canal cortado")?,
                        1 => {
                            let counts: Vec<usize> = body.chunks_exact(2).take(lh).map(|c| u16::from_be_bytes([c[0], c[1]]) as usize).collect();
                            let mut at = lh * 2;
                            let mut out = Vec::with_capacity(lw * lh);
                            for c in counts {
                                let row = body.get(at..at + c).unwrap_or(&[]);
                                out.extend(unpack(row, lw));
                                at += c;
                            }
                            out
                        }
                        _ => return Err(format!("compressão {comp} (ZIP) não suportada na camada '{}'", rec.name)),
                    };
                    planes.insert(id, plane);
                }
                if rec.group_marker || lw == 0 || lh == 0 {
                    continue;
                }
                let mut img = Image::new(w, h);
                for y in 0..lh {
                    let cy = top + y as i32;
                    if cy < 0 || cy >= h as i32 {
                        continue;
                    }
                    for x in 0..lw {
                        let cx = left + x as i32;
                        if cx < 0 || cx >= w as i32 {
                            continue;
                        }
                        let k = y * lw + x;
                        let ch = |id: i16, d: u8| planes.get(&id).map(|p| p[k]).unwrap_or(d) as f32 / 255.0;
                        let a = ch(-1, 255);
                        img.data[cy as usize * w as usize + cx as usize] = [ch(0, 0) * a, ch(1, 0) * a, ch(2, 0) * a, a];
                    }
                }
                layers.push(ImportedLayer { name: rec.name.clone(), image: img, opacity: rec.opacity as f32 / 255.0, visible: rec.visible, blend: rec.blend });
            }
        }
    }
    let _ = lmi_end;
    if layers.is_empty() {
        return Err("o PSD não tem camadas de pixels (só a imagem achatada)".into());
    }
    Ok(ImportedDoc { width: w, height: h, layers })
}

// ------------------------------------------------------------------------------------------------------------ ORA

/// Arquivos de um ZIP (guardados ou deflate): nome → conteúdo.
fn unzip(bytes: &[u8]) -> Result<std::collections::HashMap<String, Vec<u8>>, String> {
    let le16 = |i: usize| bytes.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize).ok_or("ZIP cortado");
    let le32 = |i: usize| bytes.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize).ok_or("ZIP cortado");
    let eocd = (0..bytes.len().saturating_sub(21)).rev().find(|&i| bytes[i..].starts_with(&[0x50, 0x4b, 0x05, 0x06])).ok_or("não é um ZIP (sem diretório central)")?;
    let n = le16(eocd + 10)?;
    let mut at = le32(eocd + 16)?;
    let mut out = std::collections::HashMap::new();
    for _ in 0..n {
        if le32(at)? != 0x0201_4b50 {
            return Err("diretório central do ZIP inválido".into());
        }
        let method = le16(at + 10)?;
        let csize = le32(at + 20)?;
        let (nl, xl, cl) = (le16(at + 28)?, le16(at + 30)?, le16(at + 32)?);
        let local = le32(at + 42)?;
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + nl).ok_or("ZIP cortado")?).to_string();
        at += 46 + nl + xl + cl;
        let (lnl, lxl) = (le16(local + 26)?, le16(local + 28)?);
        let start = local + 30 + lnl + lxl;
        let data = bytes.get(start..start + csize).ok_or("ZIP cortado")?;
        let content = match method {
            0 => data.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(data).map_err(|e| format!("deflate de {name}: {e:?}"))?,
            m => return Err(format!("{name}: compressão {m} não suportada")),
        };
        out.insert(name, content);
    }
    Ok(out)
}

/// Atributo `k="..."` de uma tag XML simples.
fn attr(tag: &str, k: &str) -> Option<String> {
    let pat = format!("{k}=\"");
    let i = tag.find(&pat)? + pat.len();
    let j = tag[i..].find('"')? + i;
    Some(tag[i..j].replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'"))
}

/// Lê um ORA (OpenRaster). As pilhas de dentro são achatadas na ordem; no XML a primeira camada é a de cima.
pub fn read_ora(bytes: &[u8]) -> Result<ImportedDoc, String> {
    let files = unzip(bytes)?;
    let xml = String::from_utf8_lossy(files.get("stack.xml").ok_or("ORA sem stack.xml")?).to_string();
    let img_tag = xml.split("<image").nth(1).ok_or("stack.xml sem <image>")?;
    let w: u32 = attr(img_tag, "w").and_then(|v| v.parse().ok()).ok_or("largura do ORA")?;
    let h: u32 = attr(img_tag, "h").and_then(|v| v.parse().ok()).ok_or("altura do ORA")?;
    let mut layers = Vec::new();
    for tag in xml.split("<layer").skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        let Some(src) = attr(tag, "src") else { continue };
        let Some(png) = files.get(&src) else { continue };
        let dec = image::load_from_memory(png).map_err(|e| format!("{src}: {e}"))?.to_rgba8();
        let (x, y) = (attr(tag, "x").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0), attr(tag, "y").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0));
        let mut img = Image::new(w, h);
        for (px, py, p) in dec.enumerate_pixels() {
            let (cx, cy) = (x + px as i32, y + py as i32);
            if cx < 0 || cy < 0 || cx >= w as i32 || cy >= h as i32 {
                continue;
            }
            let a = p[3] as f32 / 255.0;
            img.data[cy as usize * w as usize + cx as usize] = [p[0] as f32 / 255.0 * a, p[1] as f32 / 255.0 * a, p[2] as f32 / 255.0 * a, a];
        }
        let blend = match attr(tag, "composite-op").as_deref().unwrap_or("svg:src-over") {
            "svg:multiply" => BlendMode::Multiply,
            "svg:screen" => BlendMode::Screen,
            "svg:overlay" => BlendMode::Overlay,
            "svg:soft-light" => BlendMode::SoftLight,
            "svg:hard-light" => BlendMode::HardLight,
            "svg:plus" => BlendMode::Add,
            "svg:darken" => BlendMode::Darken,
            "svg:lighten" => BlendMode::Lighten,
            "svg:difference" => BlendMode::Difference,
            "svg:color" => BlendMode::Color,
            "svg:hue" => BlendMode::Hue,
            "svg:saturation" => BlendMode::Saturation,
            "svg:luminosity" => BlendMode::Luminosity,
            _ => BlendMode::Normal,
        };
        layers.push(ImportedLayer {
            name: attr(tag, "name").unwrap_or_else(|| src.clone()),
            image: img,
            opacity: attr(tag, "opacity").and_then(|v| v.parse().ok()).unwrap_or(1.0),
            visible: attr(tag, "visibility").as_deref() != Some("hidden"),
            blend,
        });
    }
    if layers.is_empty() {
        return Err("o ORA não tem camadas".into());
    }
    layers.reverse(); // de baixo para cima
    Ok(ImportedDoc { width: w, height: h, layers })
}

// --------------------------------------------------------------------------------------------------------- comando

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImportLayersRequest {
    pub cut: Id,
    /// Caminho absoluto do .psd ou .ora.
    pub path: String,
}

/// Lote que cria uma camada raster (com o cel 1) por camada do arquivo, no topo do corte, na ordem do arquivo.
pub fn import_layers_command(p: &Project, blobs: &mut BlobStore, idgen: &mut IdGen, req: &ImportLayersRequest) -> ToolResult<Command> {
    let cd = cut_data(p, &req.cut)?;
    let (cw, ch) = canvas_size(p, cd);
    let bytes = std::fs::read(&req.path).map_err(|e| ToolError::Invalid(format!("não consegui ler {}: {e}", req.path)))?;
    let lower = req.path.to_lowercase();
    let doc = if lower.ends_with(".ora") { read_ora(&bytes) } else { read_psd(&bytes) }.map_err(ToolError::Invalid)?;
    let mut cmds = Vec::new();
    for l in &doc.layers {
        let layer_id = idgen.next();
        let mut data = LayerData::new(LayerType::Raster);
        data.visible = l.visible;
        data.opacity = l.opacity.clamp(0.0, 1.0);
        data.blend = l.blend;
        cmds.push(Command::CreateNode { id: Some(layer_id.clone()), parent: Some(req.cut.clone()), index: None, name: l.name.clone(), data: NodeKind::Layer(data) });
        let cel_id = idgen.next();
        cmds.push(Command::CreateNode { id: Some(cel_id.clone()), parent: Some(layer_id), index: None, name: "1".into(), data: NodeKind::Cel(CelData::new(1)) });
        // o arquivo pode ter outro tamanho que o corte: recorta no canvas
        let mut img = Image::new(cw, ch);
        for y in 0..doc.height.min(ch) {
            for x in 0..doc.width.min(cw) {
                img.data[(y * cw + x) as usize] = l.image.data[(y * doc.width + x) as usize];
            }
        }
        let tiles = tiles_for_region(blobs, &img, [0, 0, cw as i32, ch as i32]);
        cmds.push(Command::SetTiles { cel: cel_id, tiles });
    }
    let n = doc.layers.len();
    Ok(Command::Batch { commands: cmds, label: Some(format!("Importar {n} camada(s)")) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits_rows() {
        // literal de 3, repetição de 4
        assert_eq!(unpack(&[2, 1, 2, 3, (-3i8) as u8, 9], 7), vec![1, 2, 3, 9, 9, 9, 9]);
    }

    #[test]
    fn zip_stored_and_deflate() {
        // ZIP mínimo feito à mão: um arquivo guardado ("a.txt" = "oi") e um deflate ("b.txt" = "aaaa...")
        let deflated = miniz_oxide::deflate::compress_to_vec(b"aaaaaaaaaaaaaaaa", 6);
        let mut z = Vec::new();
        let mut central = Vec::new();
        for (name, method, data, size) in [("a.txt", 0u16, b"oi".to_vec(), 2u32), ("b.txt", 8u16, deflated.clone(), 16u32)] {
            let off = z.len() as u32;
            z.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0]);
            z.extend_from_slice(&method.to_le_bytes());
            z.extend_from_slice(&[0; 8]);
            z.extend_from_slice(&(data.len() as u32).to_le_bytes());
            z.extend_from_slice(&size.to_le_bytes());
            z.extend_from_slice(&(name.len() as u16).to_le_bytes());
            z.extend_from_slice(&0u16.to_le_bytes());
            z.extend_from_slice(name.as_bytes());
            z.extend_from_slice(&data);
            central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0]);
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0; 8]);
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&size.to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&off.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_off = z.len() as u32;
        z.extend_from_slice(&central);
        z.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 2, 0, 2, 0]);
        z.extend_from_slice(&(central.len() as u32).to_le_bytes());
        z.extend_from_slice(&cd_off.to_le_bytes());
        z.extend_from_slice(&[0, 0]);
        let files = unzip(&z).unwrap();
        assert_eq!(files["a.txt"], b"oi");
        assert_eq!(files["b.txt"], b"aaaaaaaaaaaaaaaa");
    }
}
