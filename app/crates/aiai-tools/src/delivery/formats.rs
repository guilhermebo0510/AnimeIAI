//! Formatos de entrega (predefinições): cada um diz o que sai (vídeo, sequência de imagens, WAV da mixagem, WAV por
//! trilha, folha de contato), o codec e o ajuste de qualidade, o alvo de loudness e se grava a legenda ao lado. A tabela
//! é a mesma para o painel Predefinições, a fila, a verificação final e o MCP (`aiai_delivery`).
//!
//! H.265 e WebM (VP9) usam o ffmpeg que o PC já tem como processo externo (nada novo é linkado): só ficam disponíveis se
//! `ffmpeg -encoders` listar o codificador; senão a predefinição aparece indisponível com o motivo.

use aiai_media::VideoCodec;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Vídeo (MP4/MOV/WebM) pelo `EXPORT_VIDEO`.
    Video,
    /// Sequência de PNG numa pasta.
    PngSequence,
    /// Um WAV com a mixagem.
    Wav,
    /// Um WAV por trilha de áudio (diálogo, SE, BGM, ambiente; ou cada trilha de áudio da edição).
    Stems,
    /// Uma imagem com vários quadros (folha de contato).
    ContactSheet,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormatPreset {
    /// Identificador estável (`tv`, `youtube`...): vai na fila persistida e no MCP.
    pub id: &'static str,
    /// Chaves i18n do nome e da dica.
    pub key: &'static str,
    pub tip: &'static str,
    pub kind: Kind,
    /// Valor de `codec` do EXPORT_VIDEO (vazio fora de vídeo).
    pub codec: &'static str,
    pub crf: Option<u32>,
    /// `arib` | `ebu` | `streaming`; `None` = sem normalizar.
    pub loudness: Option<&'static str>,
    /// Leva o áudio (WAV, vídeo com som). `false` = vídeo mudo.
    pub audio: bool,
    pub ext: &'static str,
    pub sidecar_srt: bool,
    /// Reduz o vídeo a esta altura (só se o projeto for mais alto).
    pub height: Option<u32>,
    /// Velocidade do x264/x265.
    pub speed: Option<&'static str>,
}

const fn video(id: &'static str, key: &'static str, tip: &'static str, codec: &'static str, crf: Option<u32>, loudness: Option<&'static str>, ext: &'static str, sidecar_srt: bool) -> FormatPreset {
    FormatPreset { id, key, tip, kind: Kind::Video, codec, crf, loudness, audio: true, ext, sidecar_srt, height: None, speed: None }
}

pub const PRESETS: [FormatPreset; 13] = [
    video("tv", "deliver.p.tv", "deliver.p.tv.tip", "h264", Some(18), Some("arib"), "mp4", true),
    video("web", "deliver.p.web", "deliver.p.web.tip", "h264", Some(20), Some("streaming"), "mp4", true),
    FormatPreset { height: Some(1080), ..video("youtube", "deliver.p.youtube", "deliver.p.youtube.tip", "h264", Some(18), Some("streaming"), "mp4", true) },
    FormatPreset { speed: Some("slow"), ..video("master", "deliver.p.master", "deliver.p.master.tip", "h264", Some(10), Some("ebu"), "mp4", false) },
    video("nvenc", "deliver.p.nvenc", "deliver.p.nvenc.tip", "h264_nvenc", Some(19), Some("arib"), "mp4", false),
    video("prores", "deliver.p.prores", "deliver.p.prores.tip", "prores", None, Some("ebu"), "mov", false),
    video("h265", "deliver.p.h265", "deliver.p.h265.tip", "h265", Some(23), Some("streaming"), "mp4", true),
    video("webm", "deliver.p.webm", "deliver.p.webm.tip", "vp9", Some(32), Some("streaming"), "webm", true),
    FormatPreset { height: Some(360), speed: Some("veryfast"), ..video("preview", "deliver.p.preview", "deliver.p.preview.tip", "h264", Some(30), None, "mp4", false) },
    FormatPreset { kind: Kind::PngSequence, audio: false, ..video("png", "deliver.p.png", "deliver.p.png.tip", "png_sequence", None, None, "png", false) },
    FormatPreset { kind: Kind::Wav, codec: "", ..video("wav", "deliver.p.wav", "deliver.p.wav.tip", "", None, None, "wav", false) },
    FormatPreset { kind: Kind::Stems, codec: "", ..video("stems", "deliver.p.stems", "deliver.p.stems.tip", "", None, None, "wav", false) },
    FormatPreset { kind: Kind::ContactSheet, codec: "", audio: false, ..video("contact", "deliver.p.contact", "deliver.p.contact.tip", "", None, None, "png", false) },
];

pub fn preset(id: &str) -> Option<&'static FormatPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

impl FormatPreset {
    pub fn video_codec(&self) -> Option<VideoCodec> {
        if self.kind != Kind::Video && self.kind != Kind::PngSequence {
            return None;
        }
        serde_json::from_value(json!(self.codec)).ok()
    }

    /// Pronta para usar neste PC? `Err` traz o motivo (ffmpeg ausente, codificador que falta).
    pub fn availability(&self) -> Result<(), String> {
        match self.video_codec() {
            Some(c) => c.availability(),
            None => Ok(()),
        }
    }

    /// Tamanho do arquivo/saída em estimativa grosseira (para avisar de disco cheio), em bytes.
    pub fn estimate_bytes(&self, frames: u64, fps: f64, w: u32, h: u32, audio_rate: u32) -> u64 {
        let secs = frames as f64 / fps.max(1.0);
        let audio = secs * audio_rate as f64 * 2.0 * 2.0;
        // a redução de altura (YouTube, prévia) só vale se o projeto for mais alto
        let px = match self.height.filter(|t| *t < h) {
            Some(t) => w as f64 * (t as f64 / h as f64) * t as f64,
            None => w as f64 * h as f64,
        };
        let video = match self.kind {
            Kind::PngSequence => frames as f64 * px * 1.2,
            Kind::Wav => 0.0,
            Kind::Stems => audio * 3.0,
            Kind::ContactSheet => px * 4.0,
            Kind::Video => {
                let crf = self.crf.unwrap_or(18) as f64;
                // bits por pixel por quadro; anime (cores chapadas) comprime bem, a estimativa é de teto
                let bpp = match self.codec {
                    "prores" => 3.5,
                    "h265" => 0.10 * 2f64.powf((23.0 - crf) / 6.0),
                    "vp9" => 0.10 * 2f64.powf((32.0 - crf) / 6.0),
                    _ => 0.16 * 2f64.powf((18.0 - crf) / 6.0),
                };
                px * bpp * fps.max(1.0) / 8.0 * secs
            }
        };
        let with_audio = if self.audio && matches!(self.kind, Kind::Video | Kind::Wav) { audio * if self.kind == Kind::Wav { 1.0 } else { 0.1 } } else { 0.0 };
        (video + with_audio).max(0.0) as u64
    }

    /// Alvo de loudness em LUFS do `preset` do EXPORT_VIDEO (`None` = não normaliza).
    pub fn loudness_target(&self) -> Option<f64> {
        match self.loudness {
            Some("arib") => Some(-24.0),
            Some("ebu") => Some(-23.0),
            Some("streaming") => Some(-14.0),
            _ => None,
        }
    }
}

/// Parâmetros do `EXPORT_VIDEO` para a fonte (`{cut}`, `{episode}` ou `{edit}`, com `start`/`end` se houver), o formato e o arquivo.
pub fn export_params(source: &Value, p: &FormatPreset, out: &str, crf: Option<u32>) -> Value {
    let mut v = source.clone();
    v["out"] = json!(out);
    v["codec"] = json!(p.codec);
    if let Some(c) = crf.or(p.crf) {
        v["crf"] = json!(c);
    }
    if !p.audio || p.kind == Kind::PngSequence {
        v["audio"] = json!(false);
    } else if let Some(l) = p.loudness {
        v["preset"] = json!(l);
    }
    if p.sidecar_srt {
        v["sidecar_srt"] = json!(true);
    }
    if let Some(h) = p.height {
        v["height"] = json!(h);
    }
    if let Some(s) = p.speed {
        v["speed"] = json!(s);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_every_preset_resolves() {
        let mut ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len());
        for p in PRESETS {
            assert_eq!(preset(p.id).unwrap().id, p.id);
            if matches!(p.kind, Kind::Video | Kind::PngSequence) {
                assert!(p.video_codec().is_some(), "{}: codec {} desconhecido", p.id, p.codec);
            }
        }
        assert!(preset("nada").is_none());
    }

    #[test]
    fn video_params_follow_the_preset() {
        let src = json!({ "cut": "c1" });
        let v = export_params(&src, preset("tv").unwrap(), "C:\\x\\a.mp4", None);
        assert_eq!((v["cut"].as_str(), v["codec"].as_str(), v["crf"].as_u64(), v["preset"].as_str(), v["sidecar_srt"].as_bool()), (Some("c1"), Some("h264"), Some(18), Some("arib"), Some(true)));
        let v = export_params(&json!({ "edit": "e" }), preset("png").unwrap(), "C:\\x\\a_%05d.png", Some(22));
        assert_eq!((v["audio"].as_bool(), v["crf"].as_u64()), (Some(false), Some(22)));
        assert!(v.get("preset").is_none());
        // prévia: pequena, rápida e com som sem normalizar
        let v = export_params(&src, preset("preview").unwrap(), "C:\\x\\p.mp4", None);
        assert_eq!((v["height"].as_u64(), v["speed"].as_str(), v["crf"].as_u64()), (Some(360), Some("veryfast"), Some(30)));
        assert!(v.get("audio").is_none() && v.get("preset").is_none());
        let v = export_params(&src, preset("youtube").unwrap(), "C:\\x\\y.mp4", None);
        assert_eq!((v["height"].as_u64(), v["preset"].as_str()), (Some(1080), Some("streaming")));
        // webm e h265: o codec vai do jeito que o EXPORT_VIDEO entende
        assert_eq!(export_params(&src, preset("webm").unwrap(), "x.webm", None)["codec"], "vp9");
        assert_eq!(export_params(&src, preset("h265").unwrap(), "x.mp4", None)["codec"], "h265");
    }

    #[test]
    fn estimates_grow_with_quality_and_length() {
        let tv = preset("tv").unwrap();
        let a = tv.estimate_bytes(2400, 24.0, 1920, 1080, 48_000);
        let b = tv.estimate_bytes(4800, 24.0, 1920, 1080, 48_000);
        assert!(b > a && a > 1_000_000);
        assert!(preset("prores").unwrap().estimate_bytes(2400, 24.0, 1920, 1080, 48_000) > a);
        assert!(preset("preview").unwrap().estimate_bytes(2400, 24.0, 1920, 1080, 48_000) < a / 4);
        assert!(preset("stems").unwrap().estimate_bytes(2400, 24.0, 1920, 1080, 48_000) > preset("wav").unwrap().estimate_bytes(2400, 24.0, 1920, 1080, 48_000));
    }
}
