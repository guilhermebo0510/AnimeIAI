//! S17: ferramentas de áudio ponta a ponta sobre o documento — SFX/BGM/fala viram assets + clipes no corte (com undo),
//! lip-sync escreve a coluna de boca do timesheet, mix + normalização medidos. O teste de TTS roda só se o VOICEVOX
//! Engine estiver de pé em 127.0.0.1:50021 (senão é ignorado com aviso).

use aiai_audio::bgm::{BgmTrack, Song};
use aiai_audio::lipsync::{MouthShape, ShapeSet};
use aiai_audio::loudness::LoudnessPreset;
use aiai_audio::sfx::{SfxPreset, SfxSpec};
use aiai_audio::timing::{MoraTiming, Timings, VowelClass};
use aiai_audio::voicevox::{VoiceParams, VoicevoxClient};
use aiai_core::*;
use aiai_tools::audio::*;
use serde_json::json;
use std::collections::BTreeMap;

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

struct Rig {
    s: CoreSession,
    cut: Id,
    mouth: Id,
}

fn rig(duration: u32) -> Rig {
    let mut s = CoreSession::for_tests("audio");
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C001", NodeKind::Cut(CutData::new(duration)));
    let mouth = create(&mut s, Some(&cut), "BOCA", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    for n in 1..=5 {
        create(&mut s, Some(&mouth), &format!("BOCA{n}"), NodeKind::Cel(CelData::new(n)));
    }
    Rig { s, cut, mouth }
}

fn apply_clip(s: &mut CoreSession, out: &ClipOutput, label: &str) {
    s.apply(out.batch(label)).unwrap();
}

fn cut_audio(s: &CoreSession, cut: &Id) -> Vec<AudioPlacement> {
    match &s.project.node(cut).unwrap().data {
        NodeKind::Cut(c) => c.audio.clone(),
        _ => unreachable!(),
    }
}

fn mouth_map() -> BTreeMap<String, u32> {
    [("closed", 1u32), ("a", 2), ("i", 3), ("u", 4), ("e", 3), ("o", 5), ("n", 1), ("half", 3), ("open", 2)].iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

fn mora(text: &str, class: VowelClass, start: f64, end: f64) -> MoraTiming {
    MoraTiming { text: text.into(), consonant: None, vowel: "a".into(), class, devoiced: false, start, vowel_start: start, end, phrase: 0, pitch: 5.5 }
}

#[test]
fn sfx_and_bgm_become_assets_and_placements_with_undo() {
    let mut r = rig(400);
    let before = r.s.project.clone();
    let kira = sfx(&r.s.project, &mut r.s.ids, &mut r.s.blobs, &SfxRequest {
        spec: SfxSpec::new(SfxPreset::Kira, 11),
        name: None,
        track: AudioTrack::Sfx,
        place: PlacementOpts { cut: Some(r.cut.clone()), start_frame: Some(30), gain_db: -3.0, pan: 0.25, ..Default::default() },
    })
    .unwrap();
    assert!(kira.duration_s > 1.0 && kira.duration_frames >= 24);
    apply_clip(&mut r.s, &kira, "SFX kira");
    // O asset tem blob com WAV legível e origem registrada.
    let NodeKind::Asset(a) = &r.s.project.node(&kira.asset).unwrap().data else { panic!() };
    assert_eq!(a.asset, AssetType::AudioClip);
    assert!(a.source.starts_with("procedural"));
    let wav = r.s.blobs.get(a.blob.as_ref().unwrap()).unwrap();
    let decoded = aiai_audio::wav::read_wav(&wav).unwrap();
    assert_eq!(decoded.rate, 48_000);
    assert!(decoded.rms() > 0.01);
    let pl = cut_audio(&r.s, &r.cut);
    assert_eq!(pl.len(), 1);
    assert_eq!((pl[0].start_frame, pl[0].track, pl[0].gain_db, pl[0].pan), (30, AudioTrack::Sfx, -3.0, 0.25));
    // Mesma semente = mesmo blob (regerável).
    let again = sfx(&r.s.project, &mut r.s.ids, &mut r.s.blobs, &SfxRequest { spec: SfxSpec::new(SfxPreset::Kira, 11), name: None, track: AudioTrack::Sfx, place: PlacementOpts::default() }).unwrap();
    let NodeKind::Asset(a2) = &again.commands.iter().find_map(|c| if let Command::CreateNode { data, .. } = c { Some(data.clone()) } else { None }).unwrap() else { panic!() };
    assert_eq!(a2.blob, a.blob);
    // BGM com síntese interna (determinística, sem depender do SoundFont) dentro do corte.
    let mut song = Song::new(120.0);
    song.bars = 2;
    let mut t = BgmTrack::new("lead", 73);
    t.add_text("C5:1 E5:1 G5:2 R:4", 0.0).unwrap();
    song.tracks.push(t);
    let b = bgm(&r.s.project, &mut r.s.ids, &mut r.s.blobs, &BgmRequest { song, soundfont: None, builtin_synth: true, name: Some("BGM teste".into()), track: AudioTrack::Bgm, place: PlacementOpts { cut: Some(r.cut.clone()), start_frame: Some(0), ..Default::default() } }).unwrap();
    assert_eq!(b.engine, aiai_audio::bgm::BgmEngine::Builtin);
    apply_clip(&mut r.s, &b.clip, "BGM");
    assert_eq!(cut_audio(&r.s, &r.cut).len(), 2);
    // Fora do corte: erro claro, nada aplicado.
    let bad = sfx(&r.s.project, &mut r.s.ids, &mut r.s.blobs, &SfxRequest { spec: SfxSpec::new(SfxPreset::Pop, 1), name: None, track: AudioTrack::Sfx, place: PlacementOpts { cut: Some(r.cut.clone()), start_frame: Some(400), ..Default::default() } });
    assert!(bad.is_err());
    // Undo desfaz BGM e depois SFX; o projeto volta ao estado anterior.
    r.s.undo().unwrap();
    assert_eq!(cut_audio(&r.s, &r.cut).len(), 1);
    r.s.undo().unwrap();
    // Estado igual ao inicial (a revisão/proveniência avança no undo; o conteúdo volta): sem clipes e sem assets.
    assert!(cut_audio(&r.s, &r.cut).is_empty());
    assert!(r.s.project.roots.assets.is_empty());
    assert_eq!(r.s.project.nodes.len(), before.nodes.len());
    assert!(r.s.project.nodes.iter().all(|(id, n)| before.nodes.get(id).is_some_and(|b| b.data == n.data && b.children == n.children)));
}

#[test]
fn mix_normalizes_to_arib_target() {
    let mut r = rig(240);
    for (preset, frame, seed) in [(SfxPreset::Impact, 10u32, 1u64), (SfxPreset::Footstep, 100, 2), (SfxPreset::Chime, 140, 3)] {
        let o = sfx(&r.s.project, &mut r.s.ids, &mut r.s.blobs, &SfxRequest { spec: SfxSpec::new(preset, seed), name: None, track: AudioTrack::Sfx, place: PlacementOpts { cut: Some(r.cut.clone()), start_frame: Some(frame), ..Default::default() } }).unwrap();
        apply_clip(&mut r.s, &o, "sfx");
    }
    let mut mix = mix_cut(&r.s.project, &r.s.blobs, &r.cut, None).unwrap();
    assert_eq!(mix.channels(), 2);
    assert_eq!(mix.rate, 48_000);
    // 240 quadros a 24000/1001 = 10,01 s.
    assert!((mix.duration_s() - 240.0 * 1001.0 / 24_000.0).abs() < 0.001, "{}", mix.duration_s());
    assert!(mix.rms() > 0.003);
    let rep = normalize_mix(&mut mix, LoudnessPreset::Arib);
    let l = loudness_report(&mix);
    let i = l.integrated_lufs.expect("deve haver loudness integrado");
    assert!((i + 24.0).abs() <= 1.0, "integrado {i} ({rep:?})");
    assert!(l.true_peak_dbtp.unwrap() <= -1.0 + 0.05, "true peak {:?}", l.true_peak_dbtp);
    // Exportação em disco.
    let dir = std::env::temp_dir().join("aiai_audio_test");
    let path = dir.join("mix.wav");
    export_wav(&mix, &path, WavBits::Pcm16).unwrap();
    let back = aiai_audio::wav::read_wav(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(back.frames(), mix.frames());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn lipsync_moras_writes_mouth_track_and_rewrites_cleanly() {
    let mut r = rig(120);
    // Clipe com tempos de moras na mão: a(0–0,25) pausa(0,25–0,45) i(0,45–0,70) N(0,70–0,90) o(0,90–1,20).
    let wav = {
        let b = aiai_audio::sfx::synth(&SfxSpec::new(SfxPreset::Chime, 1));
        aiai_audio::wav::write_wav(&b, aiai_audio::wav::WavFormat::Pcm16).unwrap()
    };
    let h = r.s.blobs.put(wav);
    let t = Timings {
        moras: vec![mora("あ", VowelClass::A, 0.0, 0.25), mora("、", VowelClass::Pau, 0.25, 0.45), mora("い", VowelClass::I, 0.45, 0.70), mora("ん", VowelClass::N, 0.70, 0.90), mora("お", VowelClass::O, 0.90, 1.20)],
        phonemes: vec![],
        duration: 1.2,
        speech_start: 0.0,
        speech_end: 1.2,
    };
    let asset = create(&mut r.s, None, "fala", NodeKind::Asset(AssetData { asset: AssetType::AudioClip, blob: Some(h), params: json!({ "timings": t }), source: "teste".into() }));
    let pid = Id::from("pl-1");
    r.s.apply(Command::SetProps {
        id: r.cut.clone(),
        name: None,
        data: json!({ "audio": [AudioPlacement { id: pid.clone(), asset: asset.clone(), track: AudioTrack::Dialogue, start_frame: 10, trim_start: 0.0, duration: None, gain_db: 0.0, pan: 0.0, fade_in: 0.0, fade_out: 0.0, character: None }] }),
    })
    .unwrap();
    let req = LipsyncRequest { cut: r.cut.clone(), placement: Some(pid.clone()), asset: None, start_frame: None, mouth_layer: r.mouth.clone(), mapping: mouth_map(), mode: LipsyncMode::Auto, shapes: ShapeSet::Seven, min_hold_frames: None, close_gap_frames: None, lead_frames: None, track_name: None };
    let out = lipsync(&r.s.project, &mut r.s.ids, &r.s.blobs, &req).unwrap();
    assert!(out.used_moras);
    r.s.apply(out.command.clone()).unwrap();
    // Coluna de boca criada, tipo Mouth, ligada à camada.
    let NodeKind::Cut(cd) = &r.s.project.node(&r.cut).unwrap().data else { panic!() };
    let track = cd.xsheet.track_for_layer(&r.mouth).expect("trilha de boca");
    assert_eq!(track.kind, TrackKind::Mouth);
    // Lido de volta: cada quadro do corte expõe o cel esperado pela forma calculada.
    let map = aiai_audio::lipsync::MouthMap::from_pairs(&mouth_map());
    for (i, s) in out.frames.iter().enumerate() {
        let f = 10 + i as u32;
        let want = map.cel(*s).unwrap();
        assert_eq!(track.exposed_at(f), Some(CellRef::Cel { number: want }), "quadro {f} ({s:?})");
    }
    // A pausa de 0,2 s (~4,8 quadros > 2) fecha; o clipe começa no quadro 10 (a boca abre em 'a' = cel 2).
    assert!(out.frames[..5].iter().all(|s| *s == MouthShape::A));
    assert!(out.frames.iter().any(|s| *s == MouthShape::Rest), "pausa longa fecha a boca");
    // Quadro depois do fim volta ao repouso (cel 1).
    let end = 10 + out.frames.len() as u32;
    assert_eq!(track.exposed_at(end), Some(CellRef::Cel { number: 1 }));

    // Reaplicar com 3 formas reescreve o intervalo sem sobras da versão anterior.
    let req3 = LipsyncRequest { shapes: ShapeSet::Three, ..req.clone() };
    let out3 = lipsync(&r.s.project, &mut r.s.ids, &r.s.blobs, &req3).unwrap();
    r.s.apply(out3.command).unwrap();
    let NodeKind::Cut(cd) = &r.s.project.node(&r.cut).unwrap().data else { panic!() };
    let track = cd.xsheet.track_for_layer(&r.mouth).unwrap();
    let frames_in_range: Vec<(&u32, &CellRef)> = track.cells.range(10..=end).collect();
    let expected: Vec<(u32, u32)> = out3.exposures.clone();
    let got: Vec<(u32, u32)> = frames_in_range.iter().map(|(f, c)| (**f, if let CellRef::Cel { number } = c { *number } else { 0 })).collect();
    assert_eq!(got, expected, "sem entradas antigas fora da nova sequência");
    // Em 3 formas só existem cels de fechada(1)/meia(3)/aberta(2).
    assert!(got.iter().all(|(_, c)| [1, 2, 3].contains(c)), "{got:?}");

    // Modo energia no mesmo clipe (sem depender das moras) também funciona e abre a boca.
    let req_e = LipsyncRequest { mode: LipsyncMode::Energy, ..req };
    let oe = lipsync(&r.s.project, &mut r.s.ids, &r.s.blobs, &req_e).unwrap();
    assert!(!oe.used_moras);
    assert!(oe.frames.iter().any(|s| *s == MouthShape::Open));
    // Modo moras exige tempos.
    r.s.apply(Command::SetProps { id: asset.clone(), name: None, data: json!({ "params": { "timings": null } }) }).unwrap();
    let req_m = LipsyncRequest { mode: LipsyncMode::Moras, ..req_e };
    assert!(lipsync(&r.s.project, &mut r.s.ids, &r.s.blobs, &req_m).is_err());
}

#[test]
fn tts_voicevox_line_timings_lipsync_and_regeneration() {
    let client = VoicevoxClient::default();
    if !client.available() {
        eprintln!("VOICEVOX Engine fora do ar em {}: teste de TTS ignorado", aiai_audio::voicevox::DEFAULT_URL);
        return;
    }
    let mut r = rig(240);
    let character = create(&mut r.s, None, "Menina", NodeKind::Character(CharacterData { profile: String::new(), age: String::new(), heads_tall: 6.0, palette: None, voice: serde_json::Value::Null, measurements: serde_json::Value::Null }));
    let profile = VoiceProfile { engine: "voicevox".into(), speaker: 8, speaker_name: "春日部つむぎ".into(), params: VoiceParams { speed_scale: Some(1.0), ..Default::default() } };
    r.s.apply(voice_profile_command(&r.s.project, &character, &profile).unwrap()).unwrap();
    let req = TtsLineRequest {
        text: "おはよう、今日もいい天気だね。".into(),
        kana: None,
        character: Some(character.clone()),
        speaker: None, // vem do perfil
        params: None,
        name: None,
        track: AudioTrack::Dialogue,
        place: PlacementOpts { cut: Some(r.cut.clone()), start_frame: Some(12), ..Default::default() },
    };
    let out = tts_line(&r.s.project, &mut r.s.ids, &mut r.s.blobs, VOICEVOX_URL, &req).unwrap();
    assert_eq!(out.credit, "VOICEVOX:春日部つむぎ");
    apply_clip(&mut r.s, &out.clip, "fala");
    let NodeKind::Asset(a) = &r.s.project.node(&out.clip.asset).unwrap().data else { panic!() };
    assert_eq!(a.source, "VOICEVOX:春日部つむぎ");
    assert_eq!(a.params["text"], "おはよう、今日もいい天気だね。");
    assert_eq!(a.params["speaker"], 8);
    // Tempos coerentes com o áudio real: duração prevista = duração do WAV (±1 quadro do modelo acústico).
    let wav = aiai_audio::wav::read_wav(&r.s.blobs.get(a.blob.as_ref().unwrap()).unwrap()).unwrap();
    assert!((wav.duration_s() - out.timings.duration).abs() <= 1.5 / 93.75, "wav {} vs previsto {}", wav.duration_s(), out.timings.duration);
    assert!(wav.rms() > 0.01, "fala não pode ser silêncio");
    assert!(out.timings.moras.len() >= 12);
    assert!(out.timings.moras.windows(2).all(|w| w[0].end <= w[1].start + 1e-9), "moras em ordem e sem sobreposição");
    // O texto de entrada tem 14 moras de fala ("おはよう、きょうもいいてんきだね"): contar vogais não-pausa.
    let speech: Vec<_> = out.timings.moras.iter().filter(|m| m.class != VowelClass::Pau).collect();
    assert_eq!(speech.len(), 14, "オ ハ ヨ オ キョ オ モ イ イ テ ン キ ダ ネ: {:?}", speech.iter().map(|m| m.text.clone()).collect::<Vec<_>>());
    // Mesmos parâmetros → mesmo áudio (bit a bit): regenerar a partir do asset.
    let regen = tts_regenerate(&r.s.project, &mut r.s.blobs, VOICEVOX_URL, &out.clip.asset).unwrap();
    let Command::SetProps { data, .. } = &regen else { panic!() };
    assert_eq!(data["blob"], serde_json::to_value(a.blob.as_ref().unwrap()).unwrap(), "regeneração idêntica");
    // Lip-sync pelas moras do clipe.
    let ls = lipsync(
        &r.s.project,
        &mut r.s.ids,
        &r.s.blobs,
        &LipsyncRequest { cut: r.cut.clone(), placement: out.clip.placement.clone(), asset: None, start_frame: None, mouth_layer: r.mouth.clone(), mapping: mouth_map(), mode: LipsyncMode::Auto, shapes: ShapeSet::Seven, min_hold_frames: None, close_gap_frames: None, lead_frames: None, track_name: None },
    )
    .unwrap();
    assert!(ls.used_moras);
    // Sem boca aberta em silêncio por mais de 2 quadros (protocolo S17): quadros onde o centro cai em pausa/silêncio.
    let fps = r.s.project.settings.fps();
    let mut open_in_silence = 0usize;
    let mut run = 0usize;
    for (i, s) in ls.frames.iter().enumerate() {
        let tc = (i as f64 + 0.5) / fps;
        let silent = !out.timings.moras.iter().any(|m| m.class != VowelClass::Pau && m.start <= tc && tc < m.end);
        if silent && *s != MouthShape::Rest && *s != MouthShape::N {
            run += 1;
            open_in_silence = open_in_silence.max(run);
        } else {
            run = 0;
        }
    }
    assert!(open_in_silence <= 2, "boca aberta em silêncio por {open_in_silence} quadros");
}

#[test]
fn request_types_have_json_schema_and_roundtrip() {
    // As ações MCP/bridge futuras expõem estes esquemas; precisam gerar e aceitar JSON mínimo com padrões.
    for (name, schema) in [
        ("tts_line", serde_json::to_value(schemars::schema_for!(TtsLineRequest)).unwrap()),
        ("tts_kokoro_line", serde_json::to_value(schemars::schema_for!(KokoroLineRequest)).unwrap()),
        ("lipsync", serde_json::to_value(schemars::schema_for!(LipsyncRequest)).unwrap()),
        ("sfx", serde_json::to_value(schemars::schema_for!(SfxRequest)).unwrap()),
        ("bgm", serde_json::to_value(schemars::schema_for!(BgmRequest)).unwrap()),
        ("voice_profile", serde_json::to_value(schemars::schema_for!(VoiceProfile)).unwrap()),
    ] {
        assert!(schema.get("properties").is_some() || schema.get("definitions").is_some(), "{name}: {schema}");
    }
    let r: TtsLineRequest = serde_json::from_value(json!({ "text": "こんにちは", "speaker": 8, "cut": "c1", "start_frame": 12 })).unwrap();
    assert_eq!((r.track, r.place.start_frame), (AudioTrack::Dialogue, Some(12)));
    let s: SfxRequest = serde_json::from_value(json!({ "preset": "footstep", "seed": 3, "cut": "c1", "start_frame": 0, "pan": -0.5 })).unwrap();
    assert_eq!((s.spec.preset, s.spec.seed, s.track, s.place.pan), (SfxPreset::Footstep, 3, AudioTrack::Sfx, -0.5));
    let b: BgmRequest = serde_json::from_value(json!({ "song": { "tempo_bpm": 90.0, "tracks": [{ "program": 0, "notes": [{ "pitch": 60, "start_beat": 0.0, "length_beats": 1.0 }] }] } })).unwrap();
    assert_eq!((b.track, b.song.beats_per_bar, b.song.tracks[0].notes[0].velocity), (AudioTrack::Bgm, 4, 90));
    let l: LipsyncRequest = serde_json::from_value(json!({ "cut": "c1", "placement": "p1", "mouth_layer": "m1", "mapping": { "a": 2, "closed": 1 } })).unwrap();
    assert_eq!((l.mode, l.shapes), (LipsyncMode::Auto, ShapeSet::Seven));
}

#[test]
fn kokoro_ptbr_line_and_energy_lipsync() {
    let Some(script) = aiai_audio::kokoro::find_script() else {
        eprintln!("tools/kokoro ausente: teste do Kokoro ignorado");
        return;
    };
    if !aiai_audio::kokoro::models_present(&script) || std::process::Command::new("uv").arg("--version").output().is_err() {
        eprintln!("modelos do Kokoro ou `uv` ausentes: teste ignorado");
        return;
    }
    let mut r = rig(240);
    let req = KokoroLineRequest {
        text: "Bom dia! Hoje o jardim amanheceu cheio de rosas.".into(),
        kokoro: aiai_audio::kokoro::KokoroParams::default(),
        character: None,
        name: None,
        track: AudioTrack::Dialogue,
        place: PlacementOpts { cut: Some(r.cut.clone()), start_frame: Some(6), ..Default::default() },
    };
    let out = tts_kokoro_line(&r.s.project, &mut r.s.ids, &mut r.s.blobs, &req).unwrap();
    apply_clip(&mut r.s, &out, "fala kokoro");
    let NodeKind::Asset(a) = &r.s.project.node(&out.asset).unwrap().data else { panic!() };
    assert_eq!(a.source, "Kokoro-82M (Apache-2.0)");
    assert_eq!(a.params["kokoro"]["voice"], "pf_dora");
    let wav = aiai_audio::wav::read_wav(&r.s.blobs.get(a.blob.as_ref().unwrap()).unwrap()).unwrap();
    assert!(wav.duration_s() > 2.0 && wav.duration_s() < 8.0, "{}", wav.duration_s());
    assert!(wav.rms() > 0.02, "fala não pode ser silêncio");
    // Sem moras: o modo automático cai para energia e a boca abre e fecha.
    let ls = lipsync(&r.s.project, &mut r.s.ids, &r.s.blobs, &LipsyncRequest { cut: r.cut.clone(), placement: out.placement.clone(), asset: None, start_frame: None, mouth_layer: r.mouth.clone(), mapping: mouth_map(), mode: LipsyncMode::Auto, shapes: ShapeSet::Three, min_hold_frames: None, close_gap_frames: None, lead_frames: None, track_name: None }).unwrap();
    assert!(!ls.used_moras);
    assert!(ls.frames.iter().any(|s| *s == MouthShape::Open) && ls.frames.iter().any(|s| *s == MouthShape::Rest));
    assert!(ls.exposures.len() >= 6, "{:?}", ls.exposures);
}

// ---------------------------------------------------------------------------------------------------------------
// Evidência da S17 (prova de uso). Roda sob demanda, com o VOICEVOX de pé:
//   powershell -File tools\voicevox\start.ps1
//   cargo test -p aiai-tools -j 2 --test audio s17_evidence -- --ignored --nocapture
// Escreve em sprints/registro/evidencias/S17/ (relatórios, tabela de lip-sync, prévia curta) e em sidecars/out/s17/ (WAVs grandes, fora do git).
// ---------------------------------------------------------------------------------------------------------------

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

fn rms_db_between(b: &aiai_audio::AudioBuf, t0: f64, t1: f64) -> f64 {
    let m = b.to_mono();
    let a = ((t0.max(0.0)) * m.rate as f64) as usize;
    let z = ((t1 * m.rate as f64) as usize).min(m.frames());
    if z <= a {
        return -120.0;
    }
    db((m.ch[0][a..z].iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / (z - a) as f64).sqrt())
}

/// Primeiro e último instante em que o envelope (janelas de 10 ms) passa de `rel_db` abaixo do pico das janelas.
fn active_span(b: &aiai_audio::AudioBuf, rel_db: f64) -> (f64, f64) {
    let m = b.to_mono();
    let w = (m.rate / 100) as usize;
    let lv: Vec<f64> = m.ch[0].chunks(w).map(|c| db((c.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / c.len() as f64).sqrt())).collect();
    let peak = lv.iter().cloned().fold(-200.0, f64::max);
    let first = lv.iter().position(|&l| l > peak + rel_db).unwrap_or(0);
    let last = lv.iter().rposition(|&l| l > peak + rel_db).unwrap_or(0);
    (first as f64 * 0.01, (last + 1) as f64 * 0.01)
}

#[test]
#[ignore]
fn s17_evidence() {
    use aiai_audio::lipsync::frame_table_md;
    use aiai_audio::mixer::{MixSettings, ReverbSend};
    use aiai_audio::voicevox::MoraEdit;
    use std::fs;
    let client = VoicevoxClient::default();
    assert!(client.available(), "VOICEVOX Engine fora do ar: rode tools\\voicevox\\start.ps1");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().unwrap();
    let ev = root.join("sprints/registro/evidencias/S17");
    let out = root.join("sidecars/out/s17");
    fs::create_dir_all(&ev).unwrap();
    fs::create_dir_all(&out).unwrap();

    // ---- projeto: corte de 15 s (360 quadros a 24000/1001), duas camadas de boca (uma por personagem)
    let mut r = rig(360);
    let fps = r.s.project.settings.fps();
    create(&mut r.s, Some(&r.mouth), "BOCA6", NodeKind::Cel(CelData::new(6)));
    let mouth_b = create(&mut r.s, Some(&r.cut), "BOCA-B", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    for n in 1..=6 {
        create(&mut r.s, Some(&mouth_b), &format!("BOCA-B{n}"), NodeKind::Cel(CelData::new(n)));
    }
    let map: BTreeMap<String, u32> = [("closed", 1u32), ("a", 2), ("i", 3), ("u", 4), ("e", 5), ("o", 6), ("n", 1)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let mk_char = |s: &mut CoreSession, name: &str| create(s, None, name, NodeKind::Character(CharacterData { profile: String::new(), age: String::new(), heads_tall: 6.0, palette: None, voice: serde_json::Value::Null, measurements: serde_json::Value::Null }));
    let haru = mk_char(&mut r.s, "Haru");
    let mio = mk_char(&mut r.s, "Mio");

    // ---- perfis de voz originais (estilos do VOICEVOX + ajustes próprios)
    let line1 = "おはよう、今日もいい天気だね。";
    let line2 = "うん、バラがきれいに咲いたよ。";
    let q2 = client.audio_query(line2, 10).unwrap();
    let last_phrase = q2.accent_phrases.len() - 1;
    let last_mora = q2.accent_phrases[last_phrase].moras.len() - 1;
    let prof_haru = VoiceProfile {
        engine: "voicevox".into(),
        speaker: 8,
        speaker_name: "春日部つむぎ".into(),
        params: VoiceParams { speed_scale: Some(1.05), pitch_scale: Some(0.03), intonation_scale: Some(1.2), pre_phoneme_length: Some(0.05), post_phoneme_length: Some(0.1), ..Default::default() },
    };
    let prof_mio = VoiceProfile {
        engine: "voicevox".into(),
        speaker: 10,
        speaker_name: "雨晴はう".into(),
        params: VoiceParams {
            speed_scale: Some(0.93),
            pitch_scale: Some(-0.04),
            intonation_scale: Some(1.0),
            pre_phoneme_length: Some(0.05),
            post_phoneme_length: Some(0.15),
            mora_edits: vec![MoraEdit { phrase: last_phrase, mora: last_mora, pitch_delta: Some(0.25), vowel_length: Some(0.22), ..Default::default() }],
            ..Default::default()
        },
    };
    r.s.apply(voice_profile_command(&r.s.project, &haru, &prof_haru).unwrap()).unwrap();
    r.s.apply(voice_profile_command(&r.s.project, &mio, &prof_mio).unwrap()).unwrap();

    // ---- falas
    let req1 = TtsLineRequest { text: line1.into(), kana: None, character: Some(haru.clone()), speaker: None, params: None, name: Some("Haru: fala 1".into()), track: AudioTrack::Dialogue, place: PlacementOpts { cut: Some(r.cut.clone()), start_frame: Some(36), pan: -0.1, ..Default::default() } };
    let t1 = tts_line(&r.s.project, &mut r.s.ids, &mut r.s.blobs, VOICEVOX_URL, &req1).unwrap();
    apply_clip(&mut r.s, &t1.clip, "fala 1");
    let start2 = 36 + t1.clip.duration_frames + 14;
    let req2 = TtsLineRequest { text: line2.into(), character: Some(mio.clone()), name: Some("Mio: fala 2".into()), place: PlacementOpts { cut: Some(r.cut.clone()), start_frame: Some(start2), pan: 0.1, gain_db: 3.0, ..Default::default() }, ..req1.clone() };
    let t2 = tts_line(&r.s.project, &mut r.s.ids, &mut r.s.blobs, VOICEVOX_URL, &req2).unwrap();
    apply_clip(&mut r.s, &t2.clip, "fala 2");
    assert!(start2 + t2.clip.duration_frames < 360, "as duas falas cabem nos 15 s");

    // ---- SFX: passos antes da fala 1 e brilho (kira) na mora "サ" (咲) da fala 2
    let mut steps = Vec::new();
    for (k, frame) in [4u32, 17, 30].iter().enumerate() {
        let mut spec = SfxSpec::new(SfxPreset::Footstep, 100 + k as u64);
        spec.intensity = 0.9;
        let o = sfx(
            &r.s.project,
            &mut r.s.ids,
            &mut r.s.blobs,
            &SfxRequest { spec, name: Some(format!("Passo {}", k + 1)), track: AudioTrack::Sfx, place: PlacementOpts { cut: Some(r.cut.clone()), start_frame: Some(*frame), gain_db: -2.0, pan: -0.3 + 0.25 * k as f32, ..Default::default() } },
        )
        .unwrap();
        apply_clip(&mut r.s, &o, "passo");
        steps.push(o);
    }
    let sa = t2.timings.moras.iter().find(|m| m.text == "サ").expect("mora サ na fala 2");
    let kira_frame = start2 + (sa.start * fps).round() as u32;
    let kira = sfx(
        &r.s.project,
        &mut r.s.ids,
        &mut r.s.blobs,
        &SfxRequest { spec: SfxSpec::new(SfxPreset::Kira, 7), name: Some("Kira (rosa floresce)".into()), track: AudioTrack::Sfx, place: PlacementOpts { cut: Some(r.cut.clone()), start_frame: Some(kira_frame), gain_db: -10.0, pan: 0.2, ..Default::default() } },
    )
    .unwrap();
    apply_clip(&mut r.s, &kira, "kira");

    // ---- BGM de 15 s em laço (96 bpm, 6 compassos de 4/4 = 15,000 s): cordas, piano arpejado, baixo, flauta, percussão leve
    let mut song = Song::new(96.0);
    song.bars = 6;
    song.looped = true;
    song.tail_s = 2.5;
    let chords: [(&str, &str, &str, &str); 6] = [
        ("C4+E4+G4", "C2:2 G2:2", "C4 E4 G4 E4 C5 G4 E4 G4", "R:2 E5:1 G5:1"),
        ("B3+D4+G4", "G2:2 D3:2", "B3 D4 G4 D4 B4 G4 D4 G4", "A5:2 G5:1 D5:1"),
        ("A3+C4+E4", "A2:2 E3:2", "A3 C4 E4 C4 A4 E4 C4 E4", "E5:1.5 G5:0.5 A5:2"),
        ("A3+C4+F4", "F2:2 C3:2", "A3 C4 F4 C4 A4 F4 C4 F4", "C6:1 A5:1 G5:1 F5:1"),
        ("A3+D4+F4", "D2:2 A2:2", "A3 D4 F4 D4 A4 F4 D4 F4", "A5:2 F5:1 D5:1"),
        ("B3+D4+G4", "G2:2 D3:2", "B3 D4 G4 D4 B4 G4 D4 G4", "G5:1.5 F5:0.5 D5:1 R:1"),
    ];
    let (mut pad, mut piano, mut bass, mut flute, mut perc) = (BgmTrack::new("cordas", 48), BgmTrack::new("piano", 0), BgmTrack::new("baixo", 32), BgmTrack::new("flauta", 73), BgmTrack::new("percussao", 0));
    pad.volume = 70;
    pad.pan = 54;
    piano.volume = 85;
    piano.pan = 74;
    bass.volume = 95;
    flute.volume = 80;
    flute.pan = 50;
    flute.reverb = 70;
    perc.drums = true;
    perc.volume = 60;
    for (bar, (chord, b, arp, mel)) in chords.iter().enumerate() {
        let t0 = bar as f64 * 4.0;
        pad.add_text(&format!("{chord}:4@62"), t0).unwrap();
        bass.add_text(b, t0).unwrap();
        let arp_txt: String = arp.split(' ').enumerate().map(|(i, n)| format!("{n}:0.5@{}", if i % 2 == 0 { 76 } else { 58 })).collect::<Vec<_>>().join(" ");
        piano.add_text(&arp_txt, t0).unwrap();
        flute.add_text(mel, t0).unwrap();
        perc.add_text("m36:1@55 m42:1@34 m36:1@48 m42:1@34", t0).unwrap();
        perc.add_text("R:1 m37:1@40 R:1 m37:1@40", t0).unwrap();
    }
    song.tracks = vec![pad, piano, bass, flute, perc];
    let bg = bgm(
        &r.s.project,
        &mut r.s.ids,
        &mut r.s.blobs,
        &BgmRequest { song, soundfont: None, builtin_synth: false, name: Some("BGM 15 s (laço)".into()), track: AudioTrack::Bgm, place: PlacementOpts { cut: Some(r.cut.clone()), start_frame: Some(0), gain_db: 0.0, ..Default::default() } },
    )
    .unwrap();
    assert_eq!(bg.engine, aiai_audio::bgm::BgmEngine::SoundFont, "a prova usa o SoundFont (FluidR3_GM)");
    apply_clip(&mut r.s, &bg.clip, "bgm");
    let bgm_audio = asset_audio(&r.s.project, &r.s.blobs, &bg.clip.asset).unwrap();
    assert_eq!(bgm_audio.frames(), 15 * 48_000, "BGM em laço dura exatamente 15,000 s");

    // ---- lip-sync (moras) para cada fala, na coluna de boca do personagem
    let ls_req = |placement: Option<Id>, layer: &Id, shapes: ShapeSet, mapping: BTreeMap<String, u32>, name: Option<&str>| LipsyncRequest {
        cut: r.cut.clone(),
        placement,
        asset: None,
        start_frame: None,
        mouth_layer: layer.clone(),
        mapping,
        mode: LipsyncMode::Auto,
        shapes,
        min_hold_frames: None,
        close_gap_frames: None,
        lead_frames: None,
        track_name: name.map(String::from),
    };
    let l1 = lipsync(&r.s.project, &mut r.s.ids, &r.s.blobs, &ls_req(t1.clip.placement.clone(), &r.mouth, ShapeSet::Seven, map.clone(), Some("BOCA Haru"))).unwrap();
    r.s.apply(l1.command.clone()).unwrap();
    let l2 = lipsync(&r.s.project, &mut r.s.ids, &r.s.blobs, &ls_req(t2.clip.placement.clone(), &mouth_b, ShapeSet::Seven, map.clone(), Some("BOCA Mio"))).unwrap();
    r.s.apply(l2.command.clone()).unwrap();
    // Variante "anime 3 formas" da fala 1 (só para a tabela comparativa; não é aplicada).
    let map3: BTreeMap<String, u32> = [("closed", 1u32), ("half", 3), ("open", 2)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let l1_3 = lipsync(&r.s.project, &mut r.s.ids, &r.s.blobs, &ls_req(t1.clip.placement.clone(), &r.mouth, ShapeSet::Three, map3, None)).unwrap();

    let mut md = String::from("# Lip-sync S17 — tabela de quadros\n\nGerado por `cargo test -p aiai-tools --test audio s17_evidence -- --ignored`. Projeto a 24000/1001 fps (23,976). Quadros 0-based como no núcleo (o timesheet de papel mostra 1-based). Mapeamento de cels: closed/n=1, a=2, i=3, u=4, e=5, o=6.\n\nRegras: boca mínima de 2 quadros; pausa de até 2 quadros mantém a forma; pausa maior fecha; vogal → forma (a,i,u,e,o), ん/っ → fechada.\n\n");
    for (title, text, who, start, lo, tm) in [("Fala 1 — Haru (VOICEVOX:春日部つむぎ)", line1, "春日部つむぎ", 36u32, &l1, &t1.timings), ("Fala 2 — Mio (VOICEVOX:雨晴はう)", line2, "雨晴はう", start2, &l2, &t2.timings)] {
        md.push_str(&format!("## {title}\n\nTexto: 「{text}」 — voz {who}; início da fala no quadro {start}.\n\nMoras (início–fim em s, relativo ao início do clipe): "));
        md.push_str(&tm.moras.iter().map(|m| format!("{}[{:.3}–{:.3}]", m.text, m.start, m.end)).collect::<Vec<_>>().join(" "));
        md.push_str(&format!("\n\nExposições gravadas na coluna de boca (quadro absoluto → cel): {}\n\n", lo.exposures.iter().map(|(f, c)| format!("{f}→{c}")).collect::<Vec<_>>().join(", ")));
        md.push_str(&frame_table_md(&lo.frames, Some(tm), fps, start));
        md.push('\n');
    }
    md.push_str("## Variante de 3 formas (fechada/meia/aberta) — fala 1\n\nCels: fechada=1, meia=3, aberta=2. Não aplicada ao corte de prova.\n\n");
    md.push_str(&frame_table_md(&l1_3.frames, Some(&t1.timings), fps, 36));
    fs::write(ev.join("lipsync_tabela.md"), &md).unwrap();

    // ---- mixagem → normalização ARIB (-24 LKFS / -1 dBTP)
    let mut settings = MixSettings::default();
    settings.dialogue.compressor = Some(aiai_audio::dsp::CompressorParams { threshold_db: -24.0, ratio: 2.5, attack_ms: 8.0, release_ms: 140.0, knee_db: 8.0, makeup_db: 2.0, rms_ms: 25.0 });
    settings.sfx.reverb = Some(ReverbSend { level_db: -24.0, params: aiai_audio::dsp::ReverbParams { room: 0.35, damp: 0.6, width: 1.0 } });
    settings.bgm.gain_db = -3.0;
    if let Some(d) = settings.ducking.as_mut() {
        d.depth_db = 10.0;
    }
    let raw = mix_cut_with(&r.s.project, &r.s.blobs, &r.cut, None, &settings).unwrap();
    let raw_report = loudness_report(&raw);
    let mut mix = raw.clone();
    let norm = normalize_mix(&mut mix, LoudnessPreset::Arib);
    let final_report = loudness_report(&mix);
    export_wav(&mix, &out.join("mix_normalizada_48k.wav"), WavBits::Pcm16).unwrap();
    export_wav(&raw, &out.join("mix_bruta_48k_f32.wav"), WavBits::Float32).unwrap();
    // Trilhas isoladas (para os gráficos): só fala, só SFX, só BGM, com os mesmos ganhos e sem ducking.
    for (name, keep) in [("fala", aiai_audio::mixer::MixTrack::Dialogue), ("sfx", aiai_audio::mixer::MixTrack::Sfx), ("bgm", aiai_audio::mixer::MixTrack::Bgm)] {
        let mut clips = cut_clips(&r.s.project, &r.s.blobs, &r.cut).unwrap();
        clips.retain(|c| c.track == keep);
        let mut st = settings.clone();
        st.ducking = None;
        let b = aiai_audio::mixer::render(&clips, raw.frames(), &st);
        export_wav(&b, &out.join(format!("stem_{name}.wav")), WavBits::Pcm16).unwrap();
    }
    // Prévia curta para o git (< 2 MB): estéreo, 24 kHz, 16 bits.
    let preview = aiai_audio::resample::resample(&mix, 24_000);
    export_wav(&preview, &ev.join("mix_previa_24k.wav"), WavBits::Pcm16).unwrap();
    let size = fs::metadata(ev.join("mix_previa_24k.wav")).unwrap().len();
    assert!(size < 2_000_000, "prévia deve ter < 2 MB, tem {size}");
    // Reabre o WAV de 16 bits salvo e mede de novo (o que vai para o vídeo é o arquivo, não o buffer em memória).
    let saved = aiai_audio::wav::read_wav(&fs::read(out.join("mix_normalizada_48k.wav")).unwrap()).unwrap();
    let saved_report = loudness_report(&saved);
    let clip_count = saved.ch.iter().flat_map(|c| c.iter()).filter(|s| s.abs() >= 0.99997).count();
    fs::write(
        ev.join("loudness_relatorio.json"),
        serde_json::to_string_pretty(&json!({
            "arquivo_medido": "sidecars/out/s17/mix_normalizada_48k.wav (PCM16, 48 kHz, estéreo)",
            "preset": "ARIB TR-B32: -24 LKFS, true peak <= -1 dBTP",
            "normalizacao": norm,
            "bruta": { "integrated_lufs": raw_report.integrated_lufs, "true_peak_dbtp": raw_report.true_peak_dbtp, "lra_lu": raw_report.lra_lu },
            "final_buffer": final_report,
            "final_arquivo_pcm16": saved_report,
            "amostras_no_limite_do_pcm16": clip_count,
        }))
        .unwrap(),
    )
    .unwrap();

    // ---- medições por elemento
    let mut elems = Vec::new();
    for (label, t, text) in [("fala 1", &t1, line1), ("fala 2", &t2, line2)] {
        let NodeKind::Asset(a) = &r.s.project.node(&t.clip.asset).unwrap().data else { panic!() };
        let wav = aiai_audio::wav::read_wav(&r.s.blobs.get(a.blob.as_ref().unwrap()).unwrap()).unwrap();
        let (on, off) = active_span(&wav, -35.0);
        let pause = t.timings.phonemes.iter().find(|p| p.kind == aiai_audio::timing::SegKind::Pause);
        let pause_db = pause.map(|p| rms_db_between(&wav, p.start + 0.08, p.end - 0.08));
        let speech_db = rms_db_between(&wav, t.timings.speech_start, t.timings.speech_end);
        let moras = t.timings.moras.iter().filter(|m| m.class != VowelClass::Pau).count();
        elems.push(json!({
            "elemento": label, "texto": text, "credito": t.credit,
            "duracao_prevista_pelos_tempos_s": t.timings.duration, "duracao_wav_s": wav.duration_s(),
            "diferenca_ms": (wav.duration_s() - t.timings.duration).abs() * 1000.0,
            "moras_de_fala": moras, "moras_por_segundo": moras as f64 / (t.timings.speech_end - t.timings.speech_start),
            "inicio_fala_previsto_s": t.timings.speech_start, "inicio_fala_medido_no_audio_s": on,
            "fim_fala_previsto_s": t.timings.speech_end, "fim_fala_medido_no_audio_s": off,
            "rms_fala_dbfs": speech_db, "rms_pausa_dbfs": pause_db,
            "pico_dbfs": db(wav.peak() as f64), "taxa_wav": wav.rate,
        }));
        assert!((wav.duration_s() - t.timings.duration).abs() <= 1.5 / 93.75, "{label}: duração");
        assert!(speech_db > -45.0, "{label}: fala não pode ser silêncio ({speech_db} dBFS)");
        if let Some(pd) = pause_db {
            assert!(pd < speech_db - 12.0, "{label}: a pausa prevista deve ser silenciosa ({pd} vs {speech_db})");
        }
        assert!((on - t.timings.speech_start).abs() < 0.12 && (off - t.timings.speech_end).abs() < 0.2, "{label}: início/fim medidos {on:.3}/{off:.3} vs previstos {:.3}/{:.3}", t.timings.speech_start, t.timings.speech_end);
    }
    for (label, o, preset) in [("kira", &kira, "kira"), ("passo 1", &steps[0], "footstep")] {
        let NodeKind::Asset(a) = &r.s.project.node(&o.asset).unwrap().data else { panic!() };
        let wav = aiai_audio::wav::read_wav(&r.s.blobs.get(a.blob.as_ref().unwrap()).unwrap()).unwrap();
        let mut hp = wav.ch[0].clone();
        aiai_audio::dsp::filter_in_place(&mut hp, aiai_audio::dsp::BiquadCoeffs::highpass(48_000.0, 3000.0, 0.7));
        let hi = hp.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / wav.ch[0].iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
        elems.push(json!({ "elemento": label, "preset": preset, "duracao_s": wav.duration_s(), "rms_dbfs": db(wav.rms()), "pico_dbfs": db(wav.peak() as f64), "fracao_de_energia_acima_de_3khz": hi }));
    }
    let seam = (bgm_audio.ch[0][bgm_audio.frames() - 1] - bgm_audio.ch[0][0]).abs();
    elems.push(json!({ "elemento": "bgm", "duracao_s": bgm_audio.duration_s(), "rms_dbfs": db(bgm_audio.rms()), "pico_dbfs": db(bgm_audio.peak() as f64), "motor": "rustysynth + FluidR3_GM (MIT)", "soundfont": bg.soundfont, "descontinuidade_na_emenda_do_laco": seam }));
    let open_frames = |f: &[MouthShape]| f.iter().filter(|s| !matches!(s, MouthShape::Rest | MouthShape::N)).count();
    let meas = json!({
        "projeto": { "fps": fps, "corte_quadros": 360, "corte_s": 360.0 / fps, "mix_s": mix.duration_s(), "taxa": mix.rate },
        "posicoes_quadros": { "fala1": 36, "fala2": start2, "kira": kira_frame, "passos": [4, 17, 30], "bgm": 0 },
        "elementos": elems,
        "lipsync": { "fala1_quadros_total": l1.frames.len(), "fala1_quadros_boca_aberta": open_frames(&l1.frames), "fala1_exposicoes": l1.exposures.len(), "fala2_quadros_total": l2.frames.len(), "fala2_quadros_boca_aberta": open_frames(&l2.frames), "fala2_exposicoes": l2.exposures.len(), "fala1_3formas_exposicoes": l1_3.exposures.len() },
        "mix": { "lufs_integrado": saved_report.integrated_lufs, "alvo_lufs": -24.0, "true_peak_dbtp": saved_report.true_peak_dbtp, "teto_dbtp": -1.0, "lra_lu": saved_report.lra_lu, "pico_de_amostra_dbfs": saved_report.sample_peak_dbfs, "ganho_aplicado_db": norm.gain_db, "limitador_atuou": norm.limited },
        "creditos_obrigatorios": [t1.credit, t2.credit],
    });
    fs::write(ev.join("medicoes.json"), serde_json::to_string_pretty(&meas).unwrap()).unwrap();
    fs::write(ev.join("creditos_do_video.txt"), format!("{}\n{}\n", t1.credit, t2.credit)).unwrap();
    // Dados para o gráfico (mouth por quadro e posições).
    fs::write(
        ev.join("lipsync_quadros.json"),
        serde_json::to_string_pretty(&json!({
            "fps": fps,
            "fala1": { "inicio": 36, "formas": l1.frames.iter().map(|s| s.name()).collect::<Vec<_>>() },
            "fala2": { "inicio": start2, "formas": l2.frames.iter().map(|s| s.name()).collect::<Vec<_>>() },
            "kira": kira_frame, "passos": [4, 17, 30],
        }))
        .unwrap(),
    )
    .unwrap();
    let i = saved_report.integrated_lufs.unwrap();
    assert!((i + 24.0).abs() <= 1.0, "mix a -24 LKFS ±1: {i}");
    assert!(saved_report.true_peak_dbtp.unwrap() <= -1.0 + 0.05, "true peak {:?}", saved_report.true_peak_dbtp);
    assert_eq!(clip_count, 0, "sem clipping no PCM16");
    let NodeKind::Cut(cd) = &r.s.project.node(&r.cut).unwrap().data else { panic!() };
    fs::write(ev.join("corte_audio.json"), serde_json::to_string_pretty(&json!({ "audio": cd.audio, "xsheet_trilhas": cd.xsheet.tracks.iter().map(|t| json!({ "nome": t.name, "tipo": t.kind, "entradas": t.cells.len() })).collect::<Vec<_>>() })).unwrap()).unwrap();
    println!("integrado {i:.2} LUFS, true peak {:?} dBTP, LRA {:?}", saved_report.true_peak_dbtp, saved_report.lra_lu);
}
