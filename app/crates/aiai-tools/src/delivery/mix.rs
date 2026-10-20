//! Mixagem da fonte de uma entrega: o som que o `EXPORT_VIDEO` grava (para medir loudness na verificação final e para o
//! WAV) e os stems (um WAV por trilha: diálogo, SE, BGM e ambiente nos cortes; cada trilha de áudio na edição).

use super::{SourceKind, SourceRef};
use crate::context::{ToolResult, cut_data};
use crate::edit::{TrackKind, load_edit, mix_edit};
use aiai_audio::AudioBuf;
use aiai_audio::mixer::{self, MixSettings};
use aiai_core::{AudioTrack, BlobStore, Project};

fn rate(p: &Project) -> u32 {
    p.settings.audio_sample_rate.max(8000)
}

fn from_interleaved(p: &Project, inter: &[f32]) -> Option<AudioBuf> {
    let (mut l, mut r) = (Vec::with_capacity(inter.len() / 2), Vec::with_capacity(inter.len() / 2));
    for fr in inter.chunks_exact(2) {
        l.push(fr[0]);
        r.push(fr[1]);
    }
    let buf = AudioBuf::stereo(rate(p), l, r);
    (buf.peak() > 0.0).then_some(buf)
}

/// Os segmentos (cortes) mixados em sequência: `render` devolve o áudio inteiro do corte.
fn concat_segments(p: &Project, source: &SourceRef, render: impl Fn(&aiai_core::Id, u32) -> ToolResult<AudioBuf>) -> ToolResult<Option<AudioBuf>> {
    let segs = source.segments(p)?;
    let fps = p.settings.fps();
    let rate = rate(p);
    let at = |frame: u64| (frame as f64 / fps * rate as f64).round() as usize;
    let total: u64 = segs.iter().map(|(_, s, e)| (e - s) as u64).sum();
    let mut out = AudioBuf::zeros(rate, 2, at(total));
    let mut cursor = 0u64;
    for (cut, s, e) in &segs {
        let dur = cut_data(p, cut)?.duration;
        let mix = render(cut, dur)?.to_stereo();
        let (a, b) = (at(*s as u64), at(*e as u64));
        out.mix_in(&mix.slice(a, b - a), at(cursor), 1.0);
        cursor += (e - s) as u64;
    }
    Ok((out.peak() > 0.0).then_some(out))
}

/// A mixagem estéreo da fonte (a que o vídeo leva); `None` se não há som nenhum.
pub fn mix_source(p: &Project, blobs: &BlobStore, source: &SourceRef) -> ToolResult<Option<AudioBuf>> {
    match source.kind {
        SourceKind::Edit => {
            let doc = load_edit(p, &source.id)?;
            Ok(from_interleaved(p, &mix_edit(p, blobs, &doc)?))
        }
        _ => {
            let settings = MixSettings { rate: rate(p), ..Default::default() };
            concat_segments(p, source, |cut, dur| crate::audio::mix_cut_with(p, blobs, cut, Some(dur), &settings))
        }
    }
}

/// Um stem por trilha com som: (nome do arquivo, áudio). Trilha muda não gera arquivo.
pub fn stems(p: &Project, blobs: &BlobStore, source: &SourceRef) -> ToolResult<Vec<(String, AudioBuf)>> {
    let mut out = Vec::new();
    match source.kind {
        SourceKind::Edit => {
            let doc = load_edit(p, &source.id)?;
            for t in doc.tracks_of(TrackKind::Audio) {
                // só esta trilha: solo nela e as outras sem solo (o mix da edição já respeita solo e mudo)
                let mut one = doc.clone();
                for tr in one.tracks.iter_mut().filter(|x| x.kind == TrackKind::Audio) {
                    tr.solo = tr.id == t.id;
                    tr.muted = false;
                }
                if let Some(buf) = from_interleaved(p, &mix_edit(p, blobs, &one)?) {
                    out.push((doc.track_label(t.id), buf));
                }
            }
        }
        _ => {
            let settings = MixSettings { rate: rate(p), ..Default::default() };
            for (track, label) in [(AudioTrack::Dialogue, "dialogo"), (AudioTrack::Sfx, "sfx"), (AudioTrack::Bgm, "bgm"), (AudioTrack::Ambience, "ambiente")] {
                let want = crate::audio::mix_track(track);
                let buf = concat_segments(p, source, |cut, dur| {
                    let samples = (dur as f64 / p.settings.fps() * settings.rate as f64).round() as usize;
                    let clips: Vec<_> = crate::audio::cut_clips(p, blobs, cut)?.into_iter().filter(|c| c.track == want).collect();
                    Ok(mixer::render(&clips, samples, &settings))
                })?;
                if let Some(b) = buf {
                    out.push((label.to_owned(), b));
                }
            }
        }
    }
    Ok(out)
}
