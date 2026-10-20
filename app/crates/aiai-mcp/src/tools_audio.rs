//! Ferramentas curadas de edição e análise de áudio (S24): `aiai_audio_edit` (passos de edição, efeitos e rack sobre um
//! asset de áudio, resultado em NOVO asset) e `aiai_audio_analyze` (medidores, forma de onda e espectrograma resumidos).
//! Açúcar sobre `aiai_bridge::audio_tools`; rodam no app e no headless.

use crate::context::ToolError;
use crate::tools::Plan;
use crate::tools_preprod::{object_schema, required};
use serde_json::{Map, Value, json};

const AUDIO_EDIT_DESC: &str = r##"Edita um clipe de áudio do projeto (não destrutivo): aplica `steps` em ordem e guarda o resultado como NOVO asset (`params.edited_from` = a fonte e `params.steps` = o que foi feito; a fonte nunca é sobrescrita; o crédito de voz acompanha). Com `cut` posiciona o clipe novo (`track`, `start_frame`, `gain_db`, `pan`, `fade_in`, `fade_out`). `bits`: pcm24 (padrão) | pcm16 | float32. Um passo de undo. Tempos em SEGUNDOS; sem `start_s`/`end_s` o passo vale para o clipe todo; intervalo invertido ou fora do clipe é erro (nada é ignorado em silêncio). Passos (`op`):
- trim {start_s,end_s} (fica só com o intervalo) · delete {start_s,end_s} (recorta e fecha o buraco) · silence {start_s,end_s} · duplicate {start_s,end_s,at_s} (copia e cola) · insert_silence {at_s,duration_s}
- fade {dir: in|out, curve: linear|log|s_curve|equal_power, start_s?,end_s?} · gain {gain_db,start_s?,end_s?} · reverse · invert · speed {factor} (muda altura junto) · resample {rate} · mono · stereo · remix {channels}
- fx {fx:{type,...}, start_s?, end_s?} aplica um efeito ao clipe ou ao intervalo. `type` e parâmetros (todos opcionais, com padrão): amplify {gain_db, allow_clipping}; normalize {mode: peak|loudness, target_db (-1), target_lufs (-23), ceiling_dbtp (-1), remove_dc, independent_channels}; eq {bands:[{kind: peak|low_shelf|high_shelf|low_pass|high_pass|notch|band_pass, freq_hz, gain_db, q, enabled}], output_gain_db} (EQ gráfico = bandas peak em 31,25 62,5 125 250 500 1k 2k 4k 8k 16k Hz com q 1,41); compressor {threshold_db, ratio, attack_ms, release_ms, knee_db, makeup_db, auto_makeup, detector: peak|rms, rms_ms, link}; limiter {ceiling_db, lookahead_ms, release_ms, true_peak}; noise_gate {threshold_db, attack_ms, hold_ms, release_ms, range_db, hysteresis_db}; noise_reduction {reduction_db, sensitivity, freq_smoothing, decay_ms} com o perfil de ruído tirado de um trecho SÓ de ruído do próprio clipe: `profile_start_s`+`profile_end_s` no passo (mín. ~50 ms); reverb {room_size, damping, wet_db, dry_db, width, pre_delay_ms, tail_s}; pitch_shift {semitones, cents} (mantém a duração; voz); time_stretch {ratio} (nova duração/original; mantém a altura; muda a duração); dc_offset {mode: mean|high_pass, cutoff_hz}; filter {kind: high_pass|low_pass, freq_hz, slope_db_oct: 12|24|36|48}.
- rack {rack:{slots:[{fx:{type,...}, bypass?}], bypass?}, start_s?, end_s?}: cadeia ordenada (até 16 posições) aplicada de uma vez.
Devolve o Outcome + asset (o novo), placement, source, duration_s, sample_rate, channels, steps_applied. Para conferir o resultado use aiai_audio_analyze {asset:<novo>} (LUFS, pico, espectrograma) ou aiai_loudness.
Exemplo: {"asset":"<clipe>","steps":[{"op":"trim","start_s":0.2,"end_s":3.5},{"op":"fx","fx":{"type":"noise_reduction","reduction_db":18},"profile_start_s":0,"profile_end_s":0.2},{"op":"fx","fx":{"type":"filter","kind":"high_pass","freq_hz":80}},{"op":"fx","fx":{"type":"normalize","mode":"loudness","target_lufs":-23}},{"op":"fade","dir":"out","start_s":3.0,"end_s":3.3,"curve":"s_curve"}],"name":"Fala limpa"}"##;

const AUDIO_ANALYZE_DESC: &str = r##"Mede e resume um áudio SEM alterar o documento. Fonte: exatamente um de `asset` (clipe de áudio), `cut` (mixagem do corte) ou `path` (WAV sob C:\AnimeIAI); `start_s`/`end_s` recortam a análise. Devolve:
- `meters` (padrão ligado; `meters:false` desliga): LUFS integrado (portas BS.1770/R128), momentâneo e curto prazo máximos, LRA, true peak (dBTP, 4x), pico de amostra, RMS, pico/RMS/DC por canal e amostras estouradas; `series_points` (>0, máx. 1000) inclui as curvas de loudness.
- `waveform_pixels` (1..4096): mín/máx/RMS por coluna e por canal (resumo a partir de uma pirâmide em cache, rápido em qualquer zoom).
- `spectrogram` {width (128), height (64), fft_size (2048), window: hann|hamming|blackman|rectangular, channel?, min_db (-90), log_freq (true), f_lo (20), f_hi (Nyquist)}: grade `rows` de 0..255 (rows[0] = agudos; 0 = min_db, 255 = 0 dB).
Exemplo: {"asset":"<clipe>","waveform_pixels":200,"spectrogram":{"width":96,"height":48},"series_points":50}"##;

pub fn definitions() -> Vec<Value> {
    use aiai_tools::audio_edit as ae;
    vec![
        json!({ "name": "aiai_audio_edit", "description": AUDIO_EDIT_DESC, "inputSchema": object_schema::<ae::AudioEditRequest>() }),
        json!({ "name": "aiai_audio_analyze", "description": AUDIO_ANALYZE_DESC, "inputSchema": object_schema::<ae::AudioAnalyzeRequest>() }),
    ]
}

/// Plano de uma ferramenta de edição/análise de áudio; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    let with_req = |keys: &[&str], action: &'static str| -> Result<Plan, ToolError> {
        required(m, keys)?;
        Ok(Plan { action, params: Value::Object(m.clone()), save_png: None })
    };
    Some(match name {
        "aiai_audio_edit" => with_req(&["asset", "steps"], "AUDIO_EDIT"),
        "aiai_audio_analyze" => {
            if ["asset", "cut", "path"].iter().all(|k| m.get(*k).is_none_or(Value::is_null)) {
                Err(ToolError::invalid("Missing 'asset': informe `asset` (clipe de áudio), `cut` (mixagem) ou `path` (WAV)"))
            } else {
                Ok(Plan { action: "AUDIO_ANALYZE", params: Value::Object(m.clone()), save_png: None })
            }
        }
        _ => return None,
    })
}
