//! Pacote de entrega: um `.zip` com o vídeo, as legendas, o texto de créditos (`creditos.txt`, com as linhas de voz
//! exigidas pela licença, como `VOICEVOX:<personagem>`), o pôster e `checksums.sha256` (SHA-256 de cada arquivo, no
//! formato do `sha256sum`). O zip é gravado sem compressão (vídeo e PNG já são comprimidos) por um escritor próprio de
//! poucas linhas, sem dependência nova; abre no Explorador, 7-Zip e `unzip`.

use super::SourceRef;
use super::hash::{Crc32, Sha256, hex};
use super::store::Deliverable;
use crate::context::ToolResult;
use crate::edit::{ItemSource, load_edit};
use aiai_core::{Id, NodeKind, Project};
use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// O que entra num arquivo do zip.
#[derive(Debug, Clone)]
pub enum Content {
    File(PathBuf),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// Caminho dentro do zip (com `/`).
    pub name: String,
    pub content: Content,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EntryReport {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// Lê o conteúdo uma vez para o CRC e o SHA-256 e outra para gravar (arquivos grandes não ficam inteiros na memória).
fn scan(c: &Content) -> Result<(u64, u32, String), String> {
    let mut crc = Crc32::default();
    let mut sha = Sha256::default();
    let mut size = 0u64;
    match c {
        Content::Bytes(b) => {
            crc.update(b);
            sha.update(b);
            size = b.len() as u64;
        }
        Content::File(p) => {
            let mut f = std::fs::File::open(p).map_err(|e| format!("abrir {}: {e}", p.display()))?;
            let mut buf = vec![0u8; 1 << 20];
            loop {
                let n = f.read(&mut buf).map_err(|e| format!("ler {}: {e}", p.display()))?;
                if n == 0 {
                    break;
                }
                crc.update(&buf[..n]);
                sha.update(&buf[..n]);
                size += n as u64;
            }
        }
    }
    Ok((size, crc.finish(), hex(&sha.finish())))
}

fn dos_time(ms: u64) -> (u16, u16) {
    // o zip guarda a hora local do PC
    let ms = (ms as i64 + super::names::local_offset_minutes() * 60_000).max(0) as u64;
    let (date, _) = super::names::date_time_of(ms);
    let secs = (ms / 1000) % 86_400;
    let (y, m, d) = {
        let mut it = date.split('-').map(|x| x.parse::<u32>().unwrap_or(1));
        (it.next().unwrap_or(1980), it.next().unwrap_or(1), it.next().unwrap_or(1))
    };
    let t = (((secs / 3600) as u16) << 11) | ((((secs / 60) % 60) as u16) << 5) | (((secs % 60) / 2) as u16);
    let dd = ((y.saturating_sub(1980).min(127) as u16) << 9) | ((m as u16) << 5) | d as u16;
    (t, dd)
}

/// Grava o zip (sem compressão) em `out` via arquivo temporário. Devolve nome, tamanho e SHA-256 de cada entrada.
pub fn write_zip(out: &Path, entries: &[Entry], now_ms: u64) -> Result<Vec<EntryReport>, String> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("criar {}: {e}", dir.display()))?;
    }
    let tmp = out.with_extension("zip.part");
    let result = (|| -> Result<Vec<EntryReport>, String> {
        let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(|e| format!("criar {}: {e}", tmp.display()))?);
        let (t, d) = dos_time(now_ms);
        let mut central: Vec<u8> = Vec::new();
        let mut reports = Vec::new();
        let mut offset = 0u64;
        for e in entries {
            let (size, crc, sha) = scan(&e.content)?;
            if size >= u32::MAX as u64 || offset >= u32::MAX as u64 {
                return Err(format!("{} passa de 4 GB: o pacote zip simples não comporta (use a pasta de entrega)", e.name));
            }
            let name = e.name.as_bytes();
            let mut h = Vec::with_capacity(30 + name.len());
            h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            h.extend_from_slice(&20u16.to_le_bytes()); // versão necessária
            h.extend_from_slice(&0x0800u16.to_le_bytes()); // nomes em UTF-8
            h.extend_from_slice(&0u16.to_le_bytes()); // sem compressão
            h.extend_from_slice(&t.to_le_bytes());
            h.extend_from_slice(&d.to_le_bytes());
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(size as u32).to_le_bytes());
            h.extend_from_slice(&(size as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(name);
            f.write_all(&h).map_err(|er| er.to_string())?;
            match &e.content {
                Content::Bytes(b) => f.write_all(b).map_err(|er| er.to_string())?,
                Content::File(p) => {
                    let mut src = std::fs::File::open(p).map_err(|er| format!("abrir {}: {er}", p.display()))?;
                    let copied = std::io::copy(&mut src, &mut f).map_err(|er| format!("copiar {}: {er}", p.display()))?;
                    if copied != size {
                        return Err(format!("{} mudou enquanto o pacote era gravado", p.display()));
                    }
                }
            }
            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes()); // feito por
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0x0800u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&t.to_le_bytes());
            central.extend_from_slice(&d.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(size as u32).to_le_bytes());
            central.extend_from_slice(&(size as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0u8; 8]); // extra, comentário, disco, atributos internos
            central.extend_from_slice(&0u32.to_le_bytes()); // atributos externos
            central.extend_from_slice(&(offset as u32).to_le_bytes());
            central.extend_from_slice(name);
            offset += h.len() as u64 + size;
            reports.push(EntryReport { name: e.name.clone(), size, sha256: sha });
        }
        let mut end = Vec::new();
        end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        end.extend_from_slice(&[0u8; 4]); // discos
        end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        end.extend_from_slice(&(central.len() as u32).to_le_bytes());
        end.extend_from_slice(&(offset as u32).to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        f.write_all(&central).map_err(|e| e.to_string())?;
        f.write_all(&end).map_err(|e| e.to_string())?;
        f.flush().map_err(|e| e.to_string())?;
        Ok(reports)
    })();
    match result {
        Ok(r) => {
            std::fs::rename(&tmp, out).map_err(|e| format!("gravar {}: {e}", out.display()))?;
            Ok(r)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Lê de volta os nomes e conteúdos de um zip simples (sem compressão) gravado por [`write_zip`]; serve ao teste e à
/// conferência do pacote.
pub fn read_zip(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let data = std::fs::read(path).map_err(|e| format!("ler {}: {e}", path.display()))?;
    let u16at = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]) as usize;
    let u32at = |i: usize| u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
    if data.len() < 22 {
        return Err("zip curto demais".into());
    }
    let eocd = (0..=data.len() - 22).rev().find(|&i| u32at(i) == 0x0605_4b50).ok_or("fim do zip não encontrado")?;
    let (n, mut cd) = (u16at(eocd + 10), u32at(eocd + 16) as usize);
    let mut out = Vec::new();
    for _ in 0..n {
        if u32at(cd) != 0x0201_4b50 {
            return Err("diretório central corrompido".into());
        }
        let (method, crc, size, nlen, elen, clen, off) = (u16at(cd + 10), u32at(cd + 16), u32at(cd + 24) as usize, u16at(cd + 28), u16at(cd + 30), u16at(cd + 32), u32at(cd + 42) as usize);
        if method != 0 {
            return Err("só entradas sem compressão".into());
        }
        let name = String::from_utf8_lossy(&data[cd + 46..cd + 46 + nlen]).into_owned();
        let start = off + 30 + u16at(off + 26) + u16at(off + 28);
        let body = data[start..start + size].to_vec();
        let mut c = Crc32::default();
        c.update(&body);
        if c.finish() != crc {
            return Err(format!("CRC de {name} não confere"));
        }
        out.push((name, body));
        cd += 46 + nlen + elen + clen;
    }
    Ok(out)
}

/// Linhas de crédito da entrega: os créditos da edição e a origem/licença dos áudios usados (as linhas `VOICEVOX:…` que
/// a licença exige) sem repetir.
pub fn credit_lines(p: &Project, source: &SourceRef) -> ToolResult<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();
    let mut assets: Vec<Id> = Vec::new();
    if source.kind == super::SourceKind::Edit {
        let doc = load_edit(p, &source.id)?;
        lines.extend(doc.credits.iter().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()));
        for it in &doc.items {
            if let ItemSource::Audio { asset } = &it.source {
                assets.push(asset.clone());
            }
        }
    }
    for cut in source.used_cuts(p)? {
        if let Some(NodeKind::Cut(cd)) = p.node(&cut).map(|n| &n.data) {
            assets.extend(cd.audio.iter().map(|a| a.asset.clone()));
        }
    }
    for a in assets {
        if let Some(NodeKind::Asset(ad)) = p.node(&a).map(|n| &n.data) {
            let s = ad.source.trim();
            let own = s.is_empty() || s.to_lowercase().starts_with("procedural") || s.to_lowercase().starts_with("desenhado");
            if !own && !lines.iter().any(|l| l == s) {
                lines.push(s.to_owned());
            }
        }
    }
    Ok(lines)
}

/// `creditos.txt`: título, fonte, data e as linhas de crédito.
pub fn credits_text(project: &str, source: &str, date: &str, lines: &[String]) -> String {
    let mut s = format!("{project}\r\n{source} — {date}\r\n\r\n");
    if lines.is_empty() {
        s.push_str("(sem créditos registrados)\r\n");
    }
    for l in lines {
        s.push_str(l);
        s.push_str("\r\n");
    }
    s
}

/// O que vai no pacote de uma entrega.
#[derive(Debug, Clone, Default)]
pub struct PackagePlan {
    /// Pasta dentro do zip (nome da entrega).
    pub folder: String,
    pub files: Vec<PathBuf>,
    pub subtitles: Vec<PathBuf>,
    pub poster: Option<PathBuf>,
    pub credits: String,
    pub extras: Vec<PathBuf>,
}

/// Monta as entradas do zip (arquivos que não existem mais são erro) e os checksums.
pub fn entries_of(plan: &PackagePlan) -> Result<Vec<Entry>, String> {
    let root = plan.folder.trim_matches('/');
    let join = |sub: &str, p: &Path| -> Result<String, String> {
        let n = p.file_name().ok_or_else(|| format!("{} não é um arquivo", p.display()))?.to_string_lossy().into_owned();
        Ok(if sub.is_empty() { format!("{root}/{n}") } else { format!("{root}/{sub}/{n}") })
    };
    let mut entries = Vec::new();
    for p in &plan.files {
        if !p.is_file() {
            return Err(format!("arquivo da entrega não existe mais: {}", p.display()));
        }
        entries.push(Entry { name: join("", p)?, content: Content::File(p.clone()) });
    }
    for p in &plan.subtitles {
        if !p.is_file() {
            return Err(format!("legenda não existe mais: {}", p.display()));
        }
        entries.push(Entry { name: join("legendas", p)?, content: Content::File(p.clone()) });
    }
    if let Some(p) = &plan.poster {
        if !p.is_file() {
            return Err(format!("pôster não existe mais: {}", p.display()));
        }
        entries.push(Entry { name: format!("{root}/poster.{}", p.extension().and_then(|e| e.to_str()).unwrap_or("png")), content: Content::File(p.clone()) });
    }
    for p in &plan.extras {
        if p.is_file() {
            entries.push(Entry { name: join("handoff", p)?, content: Content::File(p.clone()) });
        }
    }
    entries.push(Entry { name: format!("{root}/creditos.txt"), content: Content::Bytes(plan.credits.clone().into_bytes()) });
    // checksums de tudo que entrou (o próprio arquivo de checksums fica de fora)
    let mut sums = String::new();
    for e in &entries {
        let (_, _, sha) = scan(&e.content)?;
        let rel = e.name.strip_prefix(&format!("{root}/")).unwrap_or(&e.name);
        sums.push_str(&format!("{sha}  {rel}\n"));
    }
    entries.push(Entry { name: format!("{root}/checksums.sha256"), content: Content::Bytes(sums.into_bytes()) });
    Ok(entries)
}

/// Plano do pacote de um entregável (arquivo principal, legendas ao lado, pôster).
pub fn plan_for(d: &Deliverable, credits: String) -> PackagePlan {
    let files: Vec<PathBuf> = d.result.files.iter().filter(|f| !f.to_lowercase().ends_with(".srt")).map(PathBuf::from).collect();
    let subtitles: Vec<PathBuf> = d.result.files.iter().filter(|f| f.to_lowercase().ends_with(".srt")).map(PathBuf::from).collect();
    let folder = files.first().and_then(|f| if d.result.is_sequence { f.parent().and_then(|p| p.file_name()) } else { f.file_stem() }).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "entrega".into());
    PackagePlan { folder, files, subtitles, poster: d.poster.as_ref().map(|p| PathBuf::from(&p.file)), credits, extras: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_round_trips_and_checksums_are_listed() {
        let dir = std::env::temp_dir().join(format!("aiai_pack_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("ep1_v001.mp4");
        std::fs::write(&video, (0..5000u32).map(|i| (i % 253) as u8).collect::<Vec<u8>>()).unwrap();
        let srt = dir.join("ep1_v001.srt");
        std::fs::write(&srt, "1\n00:00:00,000 --> 00:00:01,000\nOlá\n\n").unwrap();
        let poster = dir.join("poster.png");
        std::fs::write(&poster, b"png").unwrap();
        let plan = PackagePlan { folder: "ep1_v001".into(), files: vec![video.clone()], subtitles: vec![srt], poster: Some(poster), credits: credits_text("Última Rosa", "Ep 1", "2026-10-20", &["VOICEVOX:春日部つむぎ".into()]), extras: Vec::new() };
        let entries = entries_of(&plan).unwrap();
        let out = dir.join("pacote.zip");
        let rep = write_zip(&out, &entries, 1_760_000_000_000).unwrap();
        assert!(!out.with_extension("zip.part").exists());
        let names: Vec<&str> = rep.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["ep1_v001/ep1_v001.mp4", "ep1_v001/legendas/ep1_v001.srt", "ep1_v001/poster.png", "ep1_v001/creditos.txt", "ep1_v001/checksums.sha256"]);
        let back = read_zip(&out).unwrap();
        assert_eq!(back.len(), 5);
        assert_eq!(back[0].1, std::fs::read(&video).unwrap());
        let credits = String::from_utf8(back[3].1.clone()).unwrap();
        assert!(credits.contains("VOICEVOX:春日部つむぎ") && credits.contains("Última Rosa"));
        // o arquivo de checksums confere com o conteúdo do vídeo
        let sums = String::from_utf8(back[4].1.clone()).unwrap();
        let want = format!("{}  ep1_v001.mp4", super::super::hash::sha256_hex(&back[0].1));
        assert!(sums.lines().any(|l| l == want), "{sums}");
        assert!(sums.contains("legendas/ep1_v001.srt") && sums.contains("creditos.txt") && !sums.contains("checksums.sha256"));
        // arquivo que sumiu é erro claro, nada é gravado
        let missing = PackagePlan { files: vec![dir.join("nao_existe.mp4")], folder: "x".into(), ..Default::default() };
        assert!(entries_of(&missing).unwrap_err().contains("não existe mais"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn credits_come_from_the_edit_and_the_voice_assets() {
        use super::super::testkit::session_with_cut;
        use aiai_core::*;
        let (mut s, _ep, cut) = session_with_cut(12);
        let wav = crate::audio::wav_bytes(&aiai_audio::AudioBuf::mono(48_000, vec![0.1; 4800]), crate::audio::WavBits::Pcm16).unwrap();
        let blob = s.blobs.put(wav);
        let mut ad = AssetData { asset: AssetType::AudioClip, blob: Some(blob), params: serde_json::Value::Null, source: "VOICEVOX:春日部つむぎ".into() };
        let a1 = s.apply(Command::CreateNode { id: None, parent: None, index: None, name: "fala".into(), data: NodeKind::Asset(ad.clone()) }).unwrap().created[0].clone();
        ad.source = "procedural".into();
        let a2 = s.apply(Command::CreateNode { id: None, parent: None, index: None, name: "sfx".into(), data: NodeKind::Asset(ad) }).unwrap().created[0].clone();
        let mut cd = match &s.project.node(&cut).unwrap().data {
            NodeKind::Cut(c) => c.clone(),
            _ => unreachable!(),
        };
        for a in [a1.clone(), a2, a1] {
            cd.audio.push(AudioPlacement { id: Id(format!("p{}", cd.audio.len())), asset: a, track: AudioTrack::Dialogue, start_frame: 0, trim_start: 0.0, duration: None, gain_db: 0.0, pan: 0.0, fade_in: 0.0, fade_out: 0.0, character: None });
        }
        s.apply(Command::ReplaceData { id: cut.clone(), data: NodeKind::Cut(cd) }).unwrap();
        let lines = credit_lines(&s.project, &SourceRef::cut(cut)).unwrap();
        assert_eq!(lines, vec!["VOICEVOX:春日部つむぎ".to_owned()], "sem repetir e sem os áudios procedurais");
    }
}
