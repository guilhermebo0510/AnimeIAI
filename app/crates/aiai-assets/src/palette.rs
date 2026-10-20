//! Paletas: o payload do `.aiaipal` e os leitores de `.gpl` (GIMP) e `.ase` (Adobe Swatch Exchange, só o que é simples:
//! cores RGB, cinza, CMYK e Lab de entradas e grupos). Cores em sRGB 8 bits com alfa.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Teto de cores por paleta (um arquivo malformado não vira milhões de entradas).
pub const MAX_COLORS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PalColor {
    pub name: String,
    /// "#rrggbb" ou "#rrggbbaa".
    pub hex: String,
    /// Agrupamento (settei: pele, cabelo...). Vazio = sem grupo.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Papel: normal, sombra, realce, linha...
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    /// Variantes de iluminação (entardecer, noite) → "#rrggbb[aa]".
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variants: BTreeMap<String, String>,
}

impl PalColor {
    pub fn rgba(&self) -> Option<[u8; 4]> {
        parse_hex(&self.hex)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<PalColor>,
    /// Itens do arquivo que não deram para ler (linha e motivo).
    pub warnings: Vec<String>,
    /// Degradês guardados com a paleta (cores sempre diretas no arquivo).
    pub gradients: Vec<aiai_core::NamedGradient>,
}

impl Palette {
    /// Payload do envelope: `{ "colors": [...] }` (+ `"gradients": [...]` quando há degradês).
    pub fn to_payload(&self) -> Value {
        if self.gradients.is_empty() { json!({ "colors": self.colors }) } else { json!({ "colors": self.colors, "gradients": self.gradients }) }
    }

    pub fn from_payload(v: &Value) -> Result<Palette, String> {
        let list = v.get("colors").and_then(Value::as_array).ok_or("a paleta não tem `colors`")?;
        if list.len() > MAX_COLORS {
            return Err(format!("paleta com {} cores (máximo {MAX_COLORS})", list.len()));
        }
        let mut p = Palette::default();
        for (i, c) in list.iter().enumerate() {
            match serde_json::from_value::<PalColor>(c.clone()) {
                Ok(c) if c.rgba().is_some() => p.colors.push(c),
                Ok(c) => p.warnings.push(format!("cor {}: '{}' não é #rrggbb", i + 1, c.hex)),
                Err(e) => p.warnings.push(format!("cor {}: {e}", i + 1)),
            }
        }
        for (i, g) in v.get("gradients").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]).iter().take(MAX_COLORS).enumerate() {
            match serde_json::from_value::<aiai_core::NamedGradient>(g.clone()) {
                Ok(g) if g.gradient.validate().is_ok() => p.gradients.push(g),
                Ok(g) => p.warnings.push(format!("degradê {}: '{}' inválido", i + 1, g.name)),
                Err(e) => p.warnings.push(format!("degradê {}: {e}", i + 1)),
            }
        }
        Ok(p)
    }

    /// Cores que servem de amostra (sem as ilegíveis).
    pub fn swatches(&self) -> Vec<[u8; 4]> {
        self.colors.iter().filter_map(PalColor::rgba).collect()
    }
}

/// "#rgb", "#rrggbb" ou "#rrggbbaa" (com ou sem `#`).
pub fn parse_hex(s: &str) -> Option<[u8; 4]> {
    let h = s.trim().trim_start_matches('#');
    if !h.is_ascii() || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    match h.len() {
        3 => {
            let n = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|v| v * 17);
            Some([n(0)?, n(1)?, n(2)?, 255])
        }
        6 => Some([b(0)?, b(2)?, b(4)?, 255]),
        8 => Some([b(0)?, b(2)?, b(4)?, b(6)?]),
        _ => None,
    }
}

pub fn to_hex(c: [u8; 4]) -> String {
    if c[3] == 255 { format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]) } else { format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]) }
}

fn color(name: &str, c: [u8; 4], group: &str) -> PalColor {
    PalColor { name: name.trim().to_owned(), hex: to_hex(c), group: group.to_owned(), role: String::new(), variants: BTreeMap::new() }
}

// ------------------------------------------------------------------------------------------------ extrair de imagem

/// Cores dominantes de uma imagem (k-means no sRGB sobre uma versão reduzida, partida determinística pelo ponto mais
/// distante): até `k` cores, da mais usada para a menos usada, com o nome sendo o hex. Pixels quase transparentes não contam.
pub fn extract_from_image(img: &image::RgbaImage, k: usize) -> Palette {
    let k = k.clamp(1, 32);
    let small = if img.width() > 96 || img.height() > 96 { image::imageops::thumbnail(img, 96, 96) } else { img.clone() };
    let px: Vec<[f32; 3]> = small.pixels().filter(|p| p[3] >= 128).map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let mut p = Palette::default();
    if px.is_empty() {
        return p;
    }
    let d2 = |a: &[f32; 3], b: &[f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
    // centros: a média e depois sempre o pixel mais distante dos centros já escolhidos
    let mean = px.iter().fold([0.0; 3], |a, c| [a[0] + c[0], a[1] + c[1], a[2] + c[2]]).map(|v| v / px.len() as f32);
    let mut centers = vec![*px.iter().max_by(|a, b| d2(a, &mean).total_cmp(&d2(b, &mean))).unwrap_or(&mean)];
    while centers.len() < k {
        let far = px.iter().max_by(|a, b| centers.iter().map(|c| d2(a, c)).fold(f32::MAX, f32::min).total_cmp(&centers.iter().map(|c| d2(b, c)).fold(f32::MAX, f32::min)));
        match far {
            Some(f) if centers.iter().all(|c| d2(f, c) > 1.0) => centers.push(*f),
            _ => break,
        }
    }
    let mut count = vec![0usize; centers.len()];
    for _ in 0..12 {
        let mut sum = vec![[0.0f32; 3]; centers.len()];
        count.iter_mut().for_each(|c| *c = 0);
        for c in &px {
            let i = (0..centers.len()).min_by(|&a, &b| d2(c, &centers[a]).total_cmp(&d2(c, &centers[b]))).unwrap_or(0);
            sum[i] = [sum[i][0] + c[0], sum[i][1] + c[1], sum[i][2] + c[2]];
            count[i] += 1;
        }
        for i in 0..centers.len() {
            if count[i] > 0 {
                centers[i] = sum[i].map(|v| v / count[i] as f32);
            }
        }
    }
    let mut order: Vec<usize> = (0..centers.len()).filter(|i| count[*i] > 0).collect();
    order.sort_by(|a, b| count[*b].cmp(&count[*a]));
    for i in order {
        let c = [centers[i][0].round() as u8, centers[i][1].round() as u8, centers[i][2].round() as u8, 255];
        p.colors.push(color(&to_hex(c), c, ""));
    }
    p
}

// ------------------------------------------------------------------------------------------------ .gpl

/// Paleta do GIMP: `GIMP Palette`, `Name:`, `Columns:`, comentários `#` e linhas `R G B nome`.
pub fn parse_gpl(text: &str) -> Result<Palette, String> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    let first = lines.next().map(str::trim).unwrap_or("");
    if !first.eq_ignore_ascii_case("GIMP Palette") {
        return Err("não é uma paleta do GIMP (a primeira linha deve ser \"GIMP Palette\")".into());
    }
    let mut p = Palette::default();
    for (n, raw) in lines.enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(v) = line.strip_prefix("Name:") {
            p.name = v.trim().to_owned();
            continue;
        }
        if line.starts_with("Columns:") {
            continue;
        }
        let mut it = line.split_whitespace();
        let rgb: Vec<Option<u8>> = (0..3).map(|_| it.next().and_then(|t| t.parse::<i32>().ok()).map(|v| v.clamp(0, 255) as u8)).collect();
        if rgb.iter().any(Option::is_none) {
            p.warnings.push(format!("linha {}: \"{}\" não é \"R G B nome\"", n + 2, line.chars().take(40).collect::<String>()));
            continue;
        }
        if p.colors.len() >= MAX_COLORS {
            return Err(format!("paleta com mais de {MAX_COLORS} cores"));
        }
        let name = it.collect::<Vec<_>>().join(" ");
        let c = [rgb[0].unwrap_or(0), rgb[1].unwrap_or(0), rgb[2].unwrap_or(0), 255];
        let name = if name.is_empty() { to_hex(c) } else { name };
        p.colors.push(color(&name, c, ""));
    }
    if p.colors.is_empty() {
        return Err("a paleta não tem nenhuma cor".into());
    }
    Ok(p)
}

/// `.gpl` da paleta (exportar para o GIMP/Krita; alfa é descartado pelo formato).
pub fn write_gpl(p: &Palette) -> String {
    let mut s = format!("GIMP Palette\nName: {}\nColumns: 8\n#\n", p.name.replace('\n', " "));
    for c in &p.colors {
        if let Some(r) = c.rgba() {
            s.push_str(&format!("{:3} {:3} {:3} {}\n", r[0], r[1], r[2], c.name.replace('\n', " ")));
        }
    }
    s
}

// ------------------------------------------------------------------------------------------------ .ase

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.i.checked_add(n).filter(|e| *e <= self.b.len()).ok_or("arquivo .ase truncado")?;
        let s = &self.b[self.i..end];
        self.i = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }
    /// Nome UTF-16 BE com o comprimento em unidades (inclui o terminador).
    fn name(&mut self) -> Result<String, String> {
        let n = self.u16()? as usize;
        let raw = self.take(n * 2)?;
        let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
        Ok(String::from_utf16_lossy(&units))
    }
}

fn q(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn lab_to_srgb(l: f32, a: f32, b: f32) -> [u8; 3] {
    // Lab (0..100 / -128..127) → XYZ D65 aproximado → sRGB linear → gama
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let f = |t: f32| if t.powi(3) > 0.008856 { t.powi(3) } else { (t - 16.0 / 116.0) / 7.787 };
    let (x, y, z) = (f(fx) * 0.95047, f(fy), f(fz) * 1.08883);
    let lin = [3.2406 * x - 1.5372 * y - 0.4986 * z, -0.9689 * x + 1.8758 * y + 0.0415 * z, 0.0557 * x - 0.2040 * y + 1.0570 * z];
    let g = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
    };
    [q(g(lin[0])), q(g(lin[1])), q(g(lin[2]))]
}

/// Swatch Exchange (`ASEF`): entradas de cor (RGB, Gray, CMYK, LAB) e grupos (o nome do grupo vira `group`).
pub fn parse_ase(bytes: &[u8]) -> Result<Palette, String> {
    if bytes.len() > 16 << 20 {
        return Err("arquivo .ase grande demais".into());
    }
    let mut r = Rd { b: bytes, i: 0 };
    if r.take(4)? != b"ASEF" {
        return Err("não é um arquivo .ase (falta a assinatura ASEF)".into());
    }
    let (maj, _min) = (r.u16()?, r.u16()?);
    if maj != 1 {
        return Err(format!(".ase versão {maj} não suportada"));
    }
    let blocks = r.u32()? as usize;
    let mut p = Palette::default();
    let mut group = String::new();
    for _ in 0..blocks.min(MAX_COLORS * 2) {
        let (ty, len) = (r.u16()?, r.u32()? as usize);
        let body = r.take(len)?.to_vec();
        let mut b = Rd { b: &body, i: 0 };
        match ty {
            0xC001 => group = b.name()?,
            0xC002 => group.clear(),
            0x0001 => {
                let name = b.name()?;
                let model = b.take(4)?.to_vec();
                let rgb = match model.as_slice() {
                    b"RGB " => [q(b.f32()?), q(b.f32()?), q(b.f32()?)],
                    b"Gray" => {
                        let g = q(b.f32()?);
                        [g, g, g]
                    }
                    b"CMYK" => {
                        let (c, m, y, k) = (b.f32()?, b.f32()?, b.f32()?, b.f32()?);
                        [q((1.0 - c) * (1.0 - k)), q((1.0 - m) * (1.0 - k)), q((1.0 - y) * (1.0 - k))]
                    }
                    b"LAB " => {
                        let (l, a, bb) = (b.f32()?, b.f32()?, b.f32()?);
                        // o .ase guarda L em 0..1
                        lab_to_srgb(l * 100.0, a, bb)
                    }
                    other => {
                        p.warnings.push(format!("\"{name}\": modelo de cor {:?} não suportado", String::from_utf8_lossy(other)));
                        continue;
                    }
                };
                if p.colors.len() >= MAX_COLORS {
                    return Err(format!("paleta com mais de {MAX_COLORS} cores"));
                }
                let c = [rgb[0], rgb[1], rgb[2], 255];
                let name = if name.is_empty() { to_hex(c) } else { name };
                p.colors.push(color(&name, c, &group));
            }
            _ => p.warnings.push(format!("bloco {ty:#06x} ignorado")),
        }
    }
    if p.colors.is_empty() {
        return Err("a paleta não tem nenhuma cor que o app entenda".into());
    }
    Ok(p)
}

// ------------------------------------------------------------------------------------------------ JSON solto

/// JSON de paleta que não é o envelope do app: `{"name":.., "colors":[...]}` ou só `["#rrggbb", ...]`; cada cor é um
/// texto "#rrggbb" ou `{name, hex | rgba:[r,g,b,a]}`.
pub fn parse_loose_json(v: &Value) -> Result<Palette, String> {
    let (name, list) = match v {
        Value::Array(a) => (String::new(), a),
        Value::Object(o) => (o.get("name").and_then(Value::as_str).unwrap_or("").to_owned(), o.get("colors").and_then(Value::as_array).ok_or("o JSON não tem `colors`")?),
        _ => return Err("o JSON da paleta deve ser uma lista de cores ou um objeto com `colors`".into()),
    };
    if list.len() > MAX_COLORS {
        return Err(format!("paleta com {} cores (máximo {MAX_COLORS})", list.len()));
    }
    let mut p = Palette { name, ..Default::default() };
    for (i, c) in list.iter().enumerate() {
        let (cname, rgba, group) = match c {
            Value::String(s) => (String::new(), parse_hex(s), String::new()),
            Value::Object(o) => {
                let rgba = o.get("hex").and_then(Value::as_str).and_then(parse_hex).or_else(|| {
                    let v: Vec<f32> = o.get("rgba")?.as_array()?.iter().filter_map(Value::as_f64).map(|x| x as f32).collect();
                    if v.len() < 3 {
                        return None;
                    }
                    // 0..1 (o formato do documento) ou 0..255
                    let s = if v.iter().all(|x| *x <= 1.0) { 1.0 } else { 1.0 / 255.0 };
                    Some([q(v[0] * s), q(v[1] * s), q(v[2] * s), v.get(3).map(|x| q(x * s)).unwrap_or(255)])
                });
                (o.get("name").and_then(Value::as_str).unwrap_or("").to_owned(), rgba, o.get("group").and_then(Value::as_str).unwrap_or("").to_owned())
            }
            _ => (String::new(), None, String::new()),
        };
        match rgba {
            Some(c) => {
                let n = if cname.is_empty() { to_hex(c) } else { cname };
                p.colors.push(color(&n, c, &group));
            }
            None => p.warnings.push(format!("cor {}: formato não reconhecido", i + 1)),
        }
    }
    if p.colors.is_empty() {
        return Err("a paleta não tem nenhuma cor".into());
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPL: &str = "GIMP Palette\nName: Teste de pele\nColumns: 4\n# comentário\n255 224 189 Pele clara\n 141  85  36 Pele escura\n  0   0   0\nlixo aqui\n300 -5 128 Estourada\n";

    #[test]
    fn parses_gpl_names_clamps_and_reports_bad_lines() {
        let p = parse_gpl(GPL).unwrap();
        assert_eq!(p.name, "Teste de pele");
        assert_eq!(p.colors.len(), 4);
        assert_eq!(p.colors[0].name, "Pele clara");
        assert_eq!(p.colors[0].hex, "#ffe0bd");
        assert_eq!(p.colors[2].name, "#000000", "sem nome usa o hex");
        assert_eq!(p.colors[3].hex, "#ff0080", "valores fora de 0..255 são limitados");
        assert_eq!(p.warnings.len(), 1);
        assert!(p.warnings[0].contains("lixo"));
        assert!(parse_gpl("outra coisa\n1 2 3").unwrap_err().contains("GIMP Palette"));
        assert!(parse_gpl("GIMP Palette\nName: x\n").unwrap_err().contains("nenhuma cor"));
    }

    #[test]
    fn gpl_round_trips() {
        let p = parse_gpl(GPL).unwrap();
        let back = parse_gpl(&write_gpl(&p)).unwrap();
        assert_eq!(back.name, p.name);
        assert_eq!(back.colors.iter().map(|c| (&c.name, &c.hex)).collect::<Vec<_>>(), p.colors.iter().map(|c| (&c.name, &c.hex)).collect::<Vec<_>>());
    }

    fn ase_name(s: &str) -> Vec<u8> {
        let units: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        let mut v = (units.len() as u16).to_be_bytes().to_vec();
        units.iter().for_each(|u| v.extend(u.to_be_bytes()));
        v
    }

    fn ase_block(ty: u16, body: Vec<u8>) -> Vec<u8> {
        let mut v = ty.to_be_bytes().to_vec();
        v.extend((body.len() as u32).to_be_bytes());
        v.extend(body);
        v
    }

    fn ase_file(blocks: Vec<Vec<u8>>) -> Vec<u8> {
        let mut v = b"ASEF".to_vec();
        v.extend([0, 1, 0, 0]);
        v.extend((blocks.len() as u32).to_be_bytes());
        blocks.into_iter().for_each(|b| v.extend(b));
        v
    }

    fn rgb_entry(name: &str, c: [f32; 3]) -> Vec<u8> {
        let mut b = ase_name(name);
        b.extend(b"RGB ");
        c.iter().for_each(|f| b.extend(f.to_bits().to_be_bytes()));
        b.extend([0, 2]);
        ase_block(0x0001, b)
    }

    #[test]
    fn parses_ase_rgb_gray_cmyk_and_groups() {
        let mut gray = ase_name("Cinza");
        gray.extend(b"Gray");
        gray.extend(0.5f32.to_bits().to_be_bytes());
        gray.extend([0, 2]);
        let mut cmyk = ase_name("Ciano");
        cmyk.extend(b"CMYK");
        [1.0f32, 0.0, 0.0, 0.0].iter().for_each(|f| cmyk.extend(f.to_bits().to_be_bytes()));
        cmyk.extend([0, 2]);
        let bytes = ase_file(vec![
            ase_block(0xC001, ase_name("Cabelo")),
            rgb_entry("Castanho", [0.4, 0.2, 0.1]),
            ase_block(0xC002, vec![]),
            rgb_entry("Fora do grupo", [1.0, 0.0, 0.0]),
            ase_block(0x0001, gray),
            ase_block(0x0001, cmyk),
        ]);
        let p = parse_ase(&bytes).unwrap();
        assert_eq!(p.colors.len(), 4);
        assert_eq!((p.colors[0].name.as_str(), p.colors[0].group.as_str(), p.colors[0].hex.as_str()), ("Castanho", "Cabelo", "#66331a"));
        assert_eq!(p.colors[1].group, "");
        assert_eq!(p.colors[1].hex, "#ff0000");
        assert_eq!(p.colors[2].hex, "#808080");
        assert_eq!(p.colors[3].hex, "#00ffff");
        // arquivo truncado e assinatura errada são erro, não pânico
        assert!(parse_ase(&bytes[..bytes.len() - 5]).is_err());
        assert!(parse_ase(b"XXXX0000").unwrap_err().contains("ASEF"));
        assert!(parse_ase(&[]).is_err());
    }

    #[test]
    fn loose_json_accepts_hex_lists_and_objects() {
        let p = parse_loose_json(&json!(["#ff0000", "00ff00", "#0000ff80", 5])).unwrap();
        assert_eq!(p.colors.len(), 3);
        assert_eq!(p.colors[2].hex, "#0000ff80");
        assert_eq!(p.warnings.len(), 1);
        let p = parse_loose_json(&json!({"name": "X", "colors": [{"name": "Rosa", "hex": "#ff80c0"}, {"name": "Doc", "rgba": [1.0, 0.0, 0.0, 1.0]}]})).unwrap();
        assert_eq!(p.name, "X");
        assert_eq!(p.colors[0].name, "Rosa");
        assert_eq!(p.colors[1].hex, "#ff0000");
        assert!(parse_loose_json(&json!({"colors": []})).is_err());
    }

    #[test]
    fn extracts_dominant_colors_ordered_by_use() {
        let mut img = image::RgbaImage::from_pixel(40, 40, image::Rgba([250, 10, 10, 255]));
        for y in 0..40 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([10, 10, 240, 255]));
            }
        }
        img.put_pixel(39, 39, image::Rgba([0, 0, 0, 0]));
        let p = extract_from_image(&img, 4);
        assert!(p.colors.len() >= 2);
        assert_eq!(p.colors[0].hex, "#fa0a0a", "a mais usada primeiro: {:?}", p.colors);
        assert!(p.colors.iter().any(|c| c.hex == "#0a0af0"));
        assert!(extract_from_image(&image::RgbaImage::new(4, 4), 4).colors.is_empty(), "imagem transparente não dá cor");
        assert_eq!(extract_from_image(&image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255])), 8).colors.len(), 1);
    }

    #[test]
    fn payload_round_trip_keeps_groups_roles_and_variants() {
        let mut c = color("Pele", [255, 224, 189, 255], "pele");
        c.role = "sombra".into();
        c.variants.insert("noite".into(), "#334466".into());
        let p = Palette { name: "P".into(), colors: vec![c.clone()], warnings: vec![], gradients: vec![] };
        let back = Palette::from_payload(&p.to_payload()).unwrap();
        assert_eq!(back.colors, vec![c]);
        assert_eq!(back.swatches(), vec![[255, 224, 189, 255]]);
        assert!(Palette::from_payload(&json!({})).is_err());
        assert_eq!(parse_hex("#abc"), Some([0xaa, 0xbb, 0xcc, 255]));
        assert_eq!(parse_hex("zz"), None);
    }
}
