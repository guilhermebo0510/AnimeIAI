//! Verificação de saúde das ferramentas externas (auditoria 4.6g): FFmpeg e ffprobe (vídeo e áudio), VOICEVOX (fala
//! em japonês, servidor local) e Kokoro (fala local por `uv` + modelos ONNX). Cada item diz se está pronto e o que
//! fazer se não estiver; nada é instalado daqui.

use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Health {
    pub name: &'static str,
    /// `Ok(versão ou detalhe)` ou `Err(o que falta)`.
    pub status: Result<String, String>,
}

fn first_line(cmd: &mut Command) -> Result<String, String> {
    let o = cmd.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!("saiu com {}", o.status));
    }
    let s = String::from_utf8_lossy(&o.stdout);
    Ok(s.lines().next().unwrap_or("").trim().chars().take(80).collect())
}

/// Tudo o que o app usa de fora, verificado agora (VOICEVOX espera até 1,5 s).
pub fn check() -> Vec<Health> {
    let mut out = Vec::new();
    out.push(Health {
        name: "FFmpeg",
        status: first_line(Command::new(aiai_media::ffmpeg()).arg("-version")).map_err(|e| format!("não roda ({e}); instale o FFmpeg e ponha no PATH ({})", aiai_media::ffmpeg().display())),
    });
    out.push(Health {
        name: "ffprobe",
        status: first_line(Command::new(aiai_media::ffprobe()).arg("-version")).map_err(|e| format!("não roda ({e}); vem junto com o FFmpeg")),
    });
    let vv = aiai_audio::voicevox::VoicevoxClient::default().with_timeout(Duration::from_millis(1500));
    out.push(Health { name: "VOICEVOX", status: vv.version().map(|v| format!("versão {v}")).map_err(|_| "servidor não responde; abra o VOICEVOX (ou o VOICEVOX ENGINE) antes de gerar falas em japonês".into()) });
    let kokoro = match aiai_audio::kokoro::find_script() {
        None => Err("script tools/kokoro/kokoro_tts.py não encontrado".to_owned()),
        Some(script) if !aiai_audio::kokoro::models_present(&script) => Err("modelos ausentes em sidecars/kokoro (kokoro-v1.0.onnx e voices-v1.0.bin)".to_owned()),
        Some(_) => first_line(Command::new("uv").arg("--version")).map(|v| format!("pronto ({v})")).map_err(|e| format!("`uv` não roda ({e}); instale o uv para a fala local")),
    };
    out.push(Health { name: "Kokoro", status: kokoro });
    out
}
