//! Mídia via FFmpeg como processo externo (não linkado; a build "full" do gyan é GPL e só é chamada, nunca embutida).
//! Quadros vão por stdin em RGB24 cru; o áudio entra como WAV já mixado e normalizado.

use aiai_paint::Image;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

pub fn ffmpeg() -> PathBuf {
    std::env::var_os("AIAI_FFMPEG").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

pub fn ffprobe() -> PathBuf {
    std::env::var_os("AIAI_FFPROBE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("ffprobe"))
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    /// H.264 (libx264) — entrega padrão (MP4).
    #[default]
    H264,
    /// H.264 na GPU NVIDIA (h264_nvenc), se disponível.
    H264Nvenc,
    /// ProRes 422 HQ (MOV) — intermediário de edição.
    Prores,
    /// Sequência PNG (o `out` deve conter `%05d`).
    PngSequence,
    /// H.265/HEVC (libx265, MP4 com a marca hvc1 para tocar no Apple/Windows) — só se o ffmpeg tiver o codificador.
    H265,
    /// VP9 em WebM (libvpx-vp9, áudio Opus) — só se o ffmpeg tiver os codificadores.
    Vp9,
}

impl VideoCodec {
    /// Codificadores do ffmpeg que este codec exige (vazio = o ffmpeg padrão serve).
    pub fn required_encoders(self) -> &'static [&'static str] {
        match self {
            VideoCodec::H264 => &["libx264"],
            VideoCodec::H264Nvenc => &["h264_nvenc"],
            VideoCodec::Prores => &["prores_ks"],
            VideoCodec::PngSequence => &["png"],
            VideoCodec::H265 => &["libx265"],
            VideoCodec::Vp9 => &["libvpx-vp9", "libopus"],
        }
    }

    /// `Ok` se o ffmpeg deste PC tem tudo o que o codec precisa; senão o motivo em texto (para a interface).
    pub fn availability(self) -> Result<(), String> {
        let have = encoders();
        if have.is_empty() {
            return Err(format!("não consegui consultar o ffmpeg ({})", ffmpeg().display()));
        }
        let missing: Vec<&str> = self.required_encoders().iter().copied().filter(|e| !have.contains(*e)).collect();
        if missing.is_empty() { Ok(()) } else { Err(format!("o ffmpeg não tem o codificador {}", missing.join(", "))) }
    }
}

/// Codificadores que o ffmpeg do PC lista (`ffmpeg -encoders`), consultados uma vez por processo. Vazio se o ffmpeg não roda.
pub fn encoders() -> &'static std::collections::BTreeSet<String> {
    static CACHE: std::sync::OnceLock<std::collections::BTreeSet<String>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        let out = Command::new(ffmpeg()).args(["-hide_banner", "-encoders"]).stdin(Stdio::null()).output();
        let Ok(out) = out else { return Default::default() };
        parse_encoders(&String::from_utf8_lossy(&out.stdout))
    })
}

/// Linhas de `ffmpeg -encoders`: depois do cabeçalho "------", cada uma é "<flags> <nome> <descrição>".
pub fn parse_encoders(text: &str) -> std::collections::BTreeSet<String> {
    let mut after = false;
    let mut set = std::collections::BTreeSet::new();
    for line in text.lines() {
        if line.trim_start().starts_with("------") {
            after = true;
            continue;
        }
        if !after {
            continue;
        }
        let mut it = line.split_whitespace();
        if let (Some(flags), Some(name)) = (it.next(), it.next()) {
            if flags.len() == 6 {
                set.insert(name.to_owned());
            }
        }
    }
    set
}

/// Ajustes finos da codificação que não fazem parte do contrato de [`EncodeOptions`] (prévia pequena e rápida).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EncodeTuning {
    /// Altura final (largura proporcional, par): a imagem renderizada é reduzida pelo ffmpeg depois das legendas queimadas.
    pub scale_height: Option<u32>,
    /// Velocidade do x264/x265 (`veryfast`, `medium`, `slow`...); `None` = `slow` no x264 e `medium` no x265.
    pub speed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EncodeOptions {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    #[serde(default)]
    pub codec: VideoCodec,
    /// Qualidade (CRF do x264 / CQ do NVENC). 16–20 = alta.
    #[serde(default = "d_crf")]
    pub crf: u32,
    /// WAV a muxar (AAC 320k no MP4; PCM no MOV).
    #[serde(default)]
    pub audio: Option<PathBuf>,
    /// Legendas SRT queimadas na imagem (filtro subtitles) — opcional.
    #[serde(default)]
    pub burn_subtitles: Option<PathBuf>,
    /// Cor de fundo sob a transparência (sRGB).
    #[serde(default)]
    pub background: [f32; 3],
}

fn d_crf() -> u32 {
    18
}

/// Codificador em andamento: escreva quadros com `push` e termine com `finish`.
pub struct Encoder {
    child: Child,
    stdin: Option<ChildStdin>,
    opts: EncodeOptions,
    frames: u64,
    out: PathBuf,
}

impl Encoder {
    pub fn start(out: &Path, opts: &EncodeOptions) -> Result<Encoder, String> {
        Self::start_tuned(out, opts, &EncodeTuning::default())
    }

    pub fn start_tuned(out: &Path, opts: &EncodeOptions, tune: &EncodeTuning) -> Result<Encoder, String> {
        if opts.width % 2 != 0 || opts.height % 2 != 0 {
            return Err("largura e altura precisam ser pares (yuv420p)".into());
        }
        let mut cmd = Command::new(ffmpeg());
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24"]);
        cmd.args(["-s", &format!("{}x{}", opts.width, opts.height), "-framerate", &format!("{}/{}", opts.fps_num, opts.fps_den), "-i", "-"]);
        if let Some(a) = &opts.audio {
            cmd.arg("-i").arg(a);
        }
        let mut filters: Vec<String> = Vec::new();
        if let Some(srt) = &opts.burn_subtitles {
            // o filtro subtitles exige escapar ':' e '\' do caminho no Windows
            let p = srt.to_string_lossy().replace('\\', "/").replace(':', "\\:");
            filters.push(format!("subtitles='{p}'"));
        }
        if let Some(h) = tune.scale_height.filter(|h| *h > 0 && *h < opts.height) {
            filters.push(format!("scale=-2:{}:flags=lanczos", h & !1));
        }
        if !filters.is_empty() {
            cmd.args(["-vf", &filters.join(",")]);
        }
        let speed = |default: &'static str| tune.speed.clone().unwrap_or_else(|| default.to_owned());
        match opts.codec {
            VideoCodec::H265 => {
                cmd.args(["-c:v", "libx265", "-preset", &speed("medium"), "-crf", &opts.crf.to_string(), "-pix_fmt", "yuv420p", "-tag:v", "hvc1", "-movflags", "+faststart"]);
                cmd.args(["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709"]);
            }
            VideoCodec::Vp9 => {
                cmd.args(["-c:v", "libvpx-vp9", "-crf", &opts.crf.to_string(), "-b:v", "0", "-row-mt", "1", "-pix_fmt", "yuv420p"]);
                cmd.args(["-deadline", if tune.speed.as_deref().is_some_and(|s| s.contains("fast")) { "realtime" } else { "good" }, "-cpu-used", "4"]);
            }
            VideoCodec::H264 => {
                cmd.args(["-c:v", "libx264", "-preset", &speed("slow"), "-crf", &opts.crf.to_string(), "-pix_fmt", "yuv420p", "-movflags", "+faststart"]);
                cmd.args(["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709"]);
            }
            VideoCodec::H264Nvenc => {
                cmd.args(["-c:v", "h264_nvenc", "-preset", "p6", "-rc", "vbr", "-cq", &opts.crf.to_string(), "-pix_fmt", "yuv420p", "-movflags", "+faststart"]);
            }
            VideoCodec::Prores => {
                cmd.args(["-c:v", "prores_ks", "-profile:v", "3", "-pix_fmt", "yuv422p10le"]);
            }
            VideoCodec::PngSequence => {
                cmd.args(["-c:v", "png"]);
            }
        }
        if opts.audio.is_some() {
            match opts.codec {
                VideoCodec::Prores => cmd.args(["-c:a", "pcm_s24le"]),
                VideoCodec::PngSequence => cmd.args(["-an"]),
                VideoCodec::Vp9 => cmd.args(["-c:a", "libopus", "-b:a", "192k", "-shortest"]),
                _ => cmd.args(["-c:a", "aac", "-b:a", "320k", "-shortest"]),
            };
        }
        cmd.arg(out);
        cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("não consegui iniciar o ffmpeg ({}): {e}", ffmpeg().display()))?;
        let stdin = child.stdin.take();
        Ok(Encoder { child, stdin, opts: opts.clone(), frames: 0, out: out.to_path_buf() })
    }

    pub fn push(&mut self, img: &Image) -> Result<(), String> {
        if img.width != self.opts.width || img.height != self.opts.height {
            return Err(format!("quadro {}x{} ≠ {}x{}", img.width, img.height, self.opts.width, self.opts.height));
        }
        let rgb = img.to_rgb8_over(self.opts.background);
        self.stdin.as_mut().ok_or("ffmpeg já finalizado")?.write_all(&rgb).map_err(|e| format!("ffmpeg recusou o quadro: {e}"))?;
        self.frames += 1;
        Ok(())
    }

    /// Desiste da codificação (cancelada): mata o ffmpeg e apaga o arquivo parcial.
    pub fn abort(mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.out);
    }

    pub fn finish(mut self) -> Result<EncodeReport, String> {
        drop(self.stdin.take());
        let out = self.child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("ffmpeg falhou: {}", String::from_utf8_lossy(&out.stderr)));
        }
        Ok(EncodeReport { frames: self.frames, path: self.out })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EncodeReport {
    pub frames: u64,
    pub path: PathBuf,
}

/// Informação do arquivo (ffprobe JSON resumido).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProbeInfo {
    pub duration: f64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub frames: Option<u64>,
}

pub fn probe(path: &Path) -> Result<ProbeInfo, String> {
    let out = Command::new(ffprobe())
        .args(["-v", "error", "-show_format", "-show_streams", "-count_frames", "-of", "json"])
        .arg(path)
        .output()
        .map_err(|e| format!("ffprobe: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let vs = streams.iter().find(|s| s["codec_type"] == "video");
    let au = streams.iter().find(|s| s["codec_type"] == "audio");
    Ok(ProbeInfo {
        duration: v["format"]["duration"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        width: vs.and_then(|s| s["width"].as_u64()).map(|x| x as u32),
        height: vs.and_then(|s| s["height"].as_u64()).map(|x| x as u32),
        fps: vs.and_then(|s| s["r_frame_rate"].as_str()).map(String::from),
        video_codec: vs.and_then(|s| s["codec_name"].as_str()).map(String::from),
        audio_codec: au.and_then(|s| s["codec_name"].as_str()).map(String::from),
        sample_rate: au.and_then(|s| s["sample_rate"].as_str()).and_then(|s| s.parse().ok()),
        channels: au.and_then(|s| s["channels"].as_u64()).map(|x| x as u32),
        frames: vs.and_then(|s| s["nb_read_frames"].as_str()).and_then(|s| s.parse().ok()),
    })
}

/// Extrai um quadro (tempo em segundos) como imagem.
pub fn extract_frame(video: &Path, time: f64) -> Result<Image, String> {
    let out = Command::new(ffmpeg())
        .args(["-hide_banner", "-loglevel", "error", "-ss", &format!("{time:.4}"), "-i"])
        .arg(video)
        .args(["-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"])
        .output()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(format!("não extraiu quadro: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Image::decode(&out.stdout)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Subtitle {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

fn srt_time(t: f64) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, (ms / 60_000) % 60, (ms / 1000) % 60, ms % 1000)
}

pub fn write_srt(subs: &[Subtitle]) -> String {
    let mut out = String::new();
    for (i, s) in subs.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, srt_time(s.start), srt_time(s.end), s.text.trim()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_list_is_parsed_and_codecs_report_what_is_missing() {
        let txt = "Encoders:
 V..... = Video
 ------
 V....D libx265              libx265 H.265 / HEVC (codec hevc)
 A....D aac                  AAC
";
        let set = parse_encoders(txt);
        assert!(set.contains("libx265") && set.contains("aac") && !set.contains("Video"));
        assert_eq!(VideoCodec::H265.required_encoders(), &["libx265"]);
        assert!(VideoCodec::Vp9.required_encoders().contains(&"libopus"));
    }

    #[test]
    fn srt_format() {
        let s = write_srt(&[Subtitle { start: 1.5, end: 3.25, text: "Bom dia!".into() }, Subtitle { start: 3661.0, end: 3662.0, text: "x".into() }]);
        assert!(s.starts_with("1\n00:00:01,500 --> 00:00:03,250\nBom dia!\n\n2\n01:01:01,000"));
    }

    #[test]
    fn encode_and_probe_short_clip() {
        // depende do ffmpeg no PATH (instalado neste PC); pula com aviso se não houver
        if Command::new(ffmpeg()).arg("-version").output().is_err() {
            eprintln!("ffmpeg ausente: teste pulado");
            return;
        }
        let dir = std::env::temp_dir().join(format!("aiai-media-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("t.mp4");
        let opts = EncodeOptions { width: 64, height: 36, fps_num: 24000, fps_den: 1001, codec: VideoCodec::H264, crf: 20, audio: None, burn_subtitles: None, background: [0.0; 3] };
        let mut enc = Encoder::start(&out, &opts).unwrap();
        for k in 0..24 {
            let v = k as f32 / 23.0;
            enc.push(&Image::filled(64, 36, [v, 0.2, 1.0 - v, 1.0])).unwrap();
        }
        let rep = enc.finish().unwrap();
        assert_eq!(rep.frames, 24);
        let info = probe(&out).unwrap();
        assert_eq!((info.width, info.height), (Some(64), Some(36)));
        assert_eq!(info.fps.as_deref(), Some("24000/1001"));
        assert_eq!(info.frames, Some(24));
        let f = extract_frame(&out, 0.5).unwrap();
        assert_eq!((f.width, f.height), (64, 36));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
