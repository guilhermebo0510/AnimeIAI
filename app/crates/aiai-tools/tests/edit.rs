//! Edição: dois cortes com dissolve e fade, legenda queimada, créditos rolando; exportação MP4 conferida por ffprobe.

use aiai_core::*;
use aiai_media::{EncodeOptions, Encoder, VideoCodec, probe};
use aiai_paint::texture::SkySpec;
use aiai_tools::background::{SkyRequest, sky_command};
use aiai_tools::edit::{EditClip, EditDoc, SubtitleEntry, Transition, TransitionKind, edit_command, load_edit, render_edit_frame};
use serde_json::json;

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn sky_cut(s: &mut CoreSession, scene: &Id, name: &str, tod: &str, frames: u32) -> Id {
    let cut = create(s, Some(scene), name, NodeKind::Cut(CutData::new(frames)));
    let l = create(s, Some(&cut), "BG", NodeKind::Layer(LayerData::new(LayerType::Raster)));
    create(s, Some(&l), "1", NodeKind::Cel(CelData::new(1)));
    let req = SkyRequest { cut: cut.clone(), frame: 0, layer: l, sky: SkySpec { time_of_day: tod.into(), zenith: None, horizon: None, horizon_y: 140.0, clouds: 0.4, cloud_scale: 80.0, seed: 3 } };
    let c = sky_command(&s.project, &mut s.blobs, &req).unwrap();
    s.apply(c).unwrap();
    cut
}

#[test]
fn edit_with_transitions_subtitles_credits_and_export() {
    let mut s = CoreSession::for_tests("edit");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 320, "height": 180}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let a = sky_cut(&mut s, &sc, "C001", "day", 24);
    let b = sky_cut(&mut s, &sc, "C002", "sunset", 24);
    let doc = EditDoc {
        clips: vec![
            EditClip { cut: a.clone(), in_frame: 0, out_frame: None, transition_in: Some(Transition { kind: TransitionKind::FadeBlack, frames: 6 }) },
            EditClip { cut: b.clone(), in_frame: 0, out_frame: None, transition_in: Some(Transition { kind: TransitionKind::Dissolve, frames: 8 }) },
        ],
        subtitles: vec![SubtitleEntry { start_frame: 4, end_frame: 20, text: "Bom dia!".into() }],
        credits: vec!["Produzido no AnimeIAI".into(), "VOICEVOX:春日部つむぎ".into()],
        credits_seconds: 1.0,
        fade_out_end: 4,
    };
    let cmd = edit_command(&s.project, &mut s.ids, "Edição", &doc, None).unwrap();
    let id = s.apply(cmd).unwrap().created[0].clone();
    let doc = load_edit(&s.project, &id).unwrap();
    let fps = s.project.settings.fps() as f32;
    let tl = doc.timeline(&s.project, fps).unwrap();
    assert_eq!(tl.items[1].0, 16, "dissolve de 8 quadros sobrepõe o 2º clipe");
    assert_eq!(tl.total, 40 + 24, "40 quadros de cortes + 1 s de créditos");
    let f0 = render_edit_frame(&s.project, &s.blobs, &doc, 0, true).unwrap();
    assert!(f0.get(160, 20)[0] < 0.05, "começa do preto");
    let mid = render_edit_frame(&s.project, &s.blobs, &doc, 20, true).unwrap();
    let pure_a = render_edit_frame(&s.project, &s.blobs, &doc, 10, false).unwrap();
    assert_ne!(mid.content_hash(), pure_a.content_hash(), "dissolve mistura os dois");
    let sub = render_edit_frame(&s.project, &s.blobs, &doc, 10, true).unwrap();
    assert_ne!(sub.content_hash(), pure_a.content_hash(), "legenda queimada");
    // exporta
    let dir = std::env::var("AIAI_EVIDENCE_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("aiai-edit-test"));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("edicao_teste.mp4");
    let mut enc = Encoder::start(&out, &EncodeOptions { width: 320, height: 180, fps_num: 24000, fps_den: 1001, codec: VideoCodec::H264, crf: 20, audio: None, burn_subtitles: None, background: [0.0; 3] }).unwrap();
    for f in 0..tl.total {
        let img = render_edit_frame(&s.project, &s.blobs, &doc, f, true).unwrap();
        if f == 10 || f == 20 || f == 50 {
            img.save_png(&dir.join(format!("edicao_q{f:02}.png"))).unwrap();
        }
        enc.push(&img).unwrap();
    }
    enc.finish().unwrap();
    let info = probe(&out).unwrap();
    assert_eq!(info.frames, Some(tl.total as u64));
}
