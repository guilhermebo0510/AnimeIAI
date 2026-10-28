//! Edição: dois cortes com dissolve e fade, legenda queimada, créditos rolando; exportação MP4 conferida por ffprobe.
//! v2 (S24): a migração v1→v2 desenha e soa igual ao v1 (quadro a quadro, amostra a amostra), transições no meio,
//! operações de montagem (navalha, ripple, inserir, sobrescrever, slip, slide, roll) e ida e volta de SRT/VTT.

use aiai_core::*;
use aiai_media::{EncodeOptions, Encoder, VideoCodec, probe};
use aiai_paint::texture::SkySpec;
use aiai_tools::background::{SkyRequest, sky_command};
use aiai_tools::edit::legacy::{EditDocV1, mix_v1, render_v1_frame};
use aiai_tools::edit::ops::{Edge, no_media_limit};
use aiai_tools::edit::{EditClip, EditDoc, EditItem, ItemSource, SubtitleEntry, TrackKind, Transition, TransitionAlign, TransitionKind, captions, edit_command, load_edit, mix_edit, render_edit_frame};
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
            EditClip { cut: a.clone(), in_frame: 0, out_frame: None, transition_in: Some(Transition::new(TransitionKind::FadeBlack, 6)) },
            EditClip { cut: b.clone(), in_frame: 0, out_frame: None, transition_in: Some(Transition::new(TransitionKind::Dissolve, 8)) },
        ],
        subtitles: vec![SubtitleEntry { start_frame: 4, end_frame: 20, text: "Bom dia!".into() }],
        credits: vec!["Produzido no AnimeIAI".into(), "VOICEVOX:春日部つむぎ".into()],
        credits_seconds: 1.0,
        fade_out_end: 4,
        ..Default::default()
    };
    let cmd = edit_command(&s.project, &mut s.ids, "Edição", &doc, None).unwrap();
    let id = s.apply(cmd).unwrap().created[0].clone();
    let doc = load_edit(&s.project, &id).unwrap();
    let fps = s.project.settings.fps() as f32;
    let tl = doc.timeline(&s.project, fps).unwrap();
    let v1 = doc.tracks_of(TrackKind::Video)[0].id;
    let pics = doc.items_on(v1);
    assert_eq!((pics[1].start, pics[1].src_in, pics[1].transition_in.as_ref().map(|t| (t.frames, t.align))), (24, 8, Some((8, TransitionAlign::End))), "o dissolve de 8 do v1 vira transição que termina no corte, usando a alça do 2º clipe");
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

fn sfx(s: &mut CoreSession, cut: &Id, start: u32, preset: &str, seed: u32) {
    let req: aiai_tools::audio::SfxRequest = serde_json::from_value(json!({"preset": preset, "seed": seed, "cut": cut, "start_frame": start})).unwrap();
    let out = aiai_tools::audio::sfx(&s.project, &mut s.ids, &mut s.blobs, &req).unwrap();
    for c in out.commands {
        s.apply(c).unwrap();
    }
}

/// Três cortes com som; v1 com fade do preto, dissolve, cortina, flash, trecho aparado, legendas, fade final e créditos.
fn v1_fixture() -> (CoreSession, EditDocV1, EditDoc) {
    let mut s = CoreSession::for_tests("edit-mig");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 96, "height": 54}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let a = sky_cut(&mut s, &sc, "C001", "day", 20);
    let b = sky_cut(&mut s, &sc, "C002", "sunset", 18);
    let c = sky_cut(&mut s, &sc, "C003", "night", 16);
    sfx(&mut s, &a, 2, "impact", 1);
    sfx(&mut s, &b, 0, "whoosh", 2);
    sfx(&mut s, &c, 5, "impact", 3);
    let clips = vec![
        EditClip { cut: a.clone(), in_frame: 0, out_frame: None, transition_in: Some(Transition::new(TransitionKind::FadeBlack, 5)) },
        EditClip { cut: b.clone(), in_frame: 2, out_frame: Some(16), transition_in: Some(Transition::new(TransitionKind::Dissolve, 6)) },
        EditClip { cut: c.clone(), in_frame: 1, out_frame: None, transition_in: Some(Transition::new(TransitionKind::WipeLeft, 4)) },
        EditClip { cut: a.clone(), in_frame: 4, out_frame: Some(12), transition_in: Some(Transition::new(TransitionKind::Flash, 3)) },
    ];
    let subs = vec![SubtitleEntry { start_frame: 3, end_frame: 14, text: "Bom dia!".into() }, SubtitleEntry { start_frame: 20, end_frame: 33, text: "Uma legenda longa que precisa quebrar em duas linhas".into() }];
    let v1 = EditDocV1 { clips: clips.clone(), subtitles: subs.clone(), credits: vec!["Fim".into()], credits_seconds: 0.5, fade_out_end: 3 };
    // grava como v1 cru (como um documento antigo) e lê: load_edit converte
    let raw = json!({"clips": clips, "subtitles": subs, "credits": ["Fim"], "credits_seconds": 0.5, "fade_out_end": 3});
    let id = create(&mut s, None, "Edição v1", NodeKind::Document(DocumentData { doc: DocumentType::Edit, content: raw }));
    let migrated = load_edit(&s.project, &id).unwrap();
    (s, v1, migrated)
}

#[test]
fn v1_migration_renders_and_sounds_identical() {
    let (mut s, v1, doc) = v1_fixture();
    assert_eq!(doc.version, 2);
    assert!(doc.clips.is_empty() && doc.subtitles.is_empty(), "o v1 virou trilhas");
    doc.validate(&s.project).unwrap();
    let fps = s.project.settings.fps() as f32;
    let (t1, t2) = (v1.timeline(&s.project, fps).unwrap(), doc.timeline(&s.project, fps).unwrap());
    assert_eq!((t1.credits_start, t1.total), (t2.credits_start, t2.total));
    // trilhas: V1 com os 4 cortes, o som vinculado em A1/A2 (o do clipe com dissolve sobrepõe o anterior), S1 com as legendas
    let v = doc.tracks_of(TrackKind::Video)[0].id;
    assert_eq!(doc.items_on(v).len(), 4);
    assert!(doc.items.iter().filter(|i| matches!(i.source, ItemSource::CutAudio { .. })).all(|i| i.link.is_some()));
    for f in 0..t1.total {
        for burn in [true, false] {
            let a = render_v1_frame(&s.project, &s.blobs, &v1, f, burn).unwrap();
            let b = render_edit_frame(&s.project, &s.blobs, &doc, f, burn).unwrap();
            assert_eq!(a.content_hash(), b.content_hash(), "quadro {f} (legenda {burn}) difere do v1");
        }
    }
    let (m1, m2) = (mix_v1(&s.project, &s.blobs, &v1).unwrap(), mix_edit(&s.project, &s.blobs, &doc).unwrap());
    assert_eq!(m1.len(), m2.len());
    assert!(m1.iter().any(|x| x.abs() > 1e-4), "a referência tem som");
    assert!(m1 == m2, "a mixagem migrada difere do v1");
    // gravar de novo (v2) e ler: igual
    let cmd = edit_command(&s.project, &mut s.ids, "v2", &doc, None).unwrap();
    let id = s.apply(cmd).unwrap().created[0].clone();
    assert_eq!(load_edit(&s.project, &id).unwrap(), doc);
}

/// Projeto mínimo (sem cortes) para itens de cor sólida.
fn color_project() -> CoreSession {
    let mut s = CoreSession::for_tests("edit-color");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 40, "height": 20}}) }).unwrap();
    s
}

fn color(id: u32, track: u32, start: u32, dur: u32, rgba: [f32; 4]) -> EditItem {
    EditItem::new(id, track, start, dur, ItemSource::Color { rgba })
}

#[test]
fn transitions_render_correctly_at_midpoints() {
    let s = color_project();
    let red = [1.0, 0.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    let at = |kind: TransitionKind, align: TransitionAlign, f: u32| {
        let mut d = EditDoc::with_default_tracks();
        let v = d.tracks_of(TrackKind::Video)[0].id;
        d.items.push(color(1, v, 0, 20, red));
        let mut b = color(2, v, 20, 20, blue);
        b.transition_in = Some(Transition { kind, frames: 10, align });
        d.items.push(b);
        d.validate(&s.project).unwrap();
        render_edit_frame(&s.project, &s.blobs, &d, f, false).unwrap()
    };
    let near = |p: [f32; 4], q: [f32; 3], tol: f32| (0..3).all(|c| (p[c] - q[c]).abs() <= tol);
    // dissolve centrado: 15..25, meio em 20 (k = 0,55)
    let m = at(TransitionKind::Dissolve, TransitionAlign::Center, 20);
    assert!(near(m.get(20, 10), [0.45, 0.0, 0.55], 0.02), "{:?}", m.get(20, 10));
    // fora do trecho: puro
    assert!(near(at(TransitionKind::Dissolve, TransitionAlign::Center, 14).get(5, 5), [1.0, 0.0, 0.0], 1e-6));
    assert!(near(at(TransitionKind::Dissolve, TransitionAlign::Center, 25).get(5, 5), [0.0, 0.0, 1.0], 1e-6));
    // começa no corte: 20..30, meio em 25 (k = 0,55)
    assert!(near(at(TransitionKind::Dissolve, TransitionAlign::Start, 25).get(5, 5), [0.45, 0.0, 0.55], 0.02));
    assert!(near(at(TransitionKind::Dissolve, TransitionAlign::Start, 19).get(5, 5), [1.0, 0.0, 0.0], 1e-6));
    // cortina para a esquerda no meio (k = 0,55): o novo cobre a direita
    let w = at(TransitionKind::WipeLeft, TransitionAlign::Center, 20);
    assert!(near(w.get(2, 10), [1.0, 0.0, 0.0], 1e-6) && near(w.get(37, 10), [0.0, 0.0, 1.0], 1e-6));
    let w = at(TransitionKind::WipeUp, TransitionAlign::Center, 20);
    assert!(near(w.get(10, 1), [1.0, 0.0, 0.0], 1e-6) && near(w.get(10, 18), [0.0, 0.0, 1.0], 1e-6));
    // mergulho no preto: escuro no meio, o anterior escurece na primeira metade
    let dip = at(TransitionKind::DipBlack, TransitionAlign::Center, 20);
    assert!(dip.get(5, 5)[0] < 0.01 && dip.get(5, 5)[2] < 0.15, "{:?}", dip.get(5, 5));
    let early = at(TransitionKind::DipBlack, TransitionAlign::Center, 16);
    assert!(early.get(5, 5)[0] > 0.5 && early.get(5, 5)[2] < 1e-6, "primeira metade: o anterior escurece");
    let dipw = at(TransitionKind::DipWhite, TransitionAlign::Center, 20);
    assert!(near(dipw.get(5, 5), [1.0, 0.9, 1.0], 0.12), "{:?}", dipw.get(5, 5));
    // deslizar: o novo entra pela direita por cima do anterior; empurrar para a direita: o novo vem da esquerda
    let sl = at(TransitionKind::SlideLeft, TransitionAlign::Center, 20);
    assert!(near(sl.get(2, 10), [1.0, 0.0, 0.0], 1e-6) && near(sl.get(38, 10), [0.0, 0.0, 1.0], 1e-6));
    let pu = at(TransitionKind::PushRight, TransitionAlign::Center, 20);
    assert!(near(pu.get(2, 10), [0.0, 0.0, 1.0], 1e-6) && near(pu.get(38, 10), [1.0, 0.0, 0.0], 1e-6));
    // íris: centro já é o novo, cantos ainda o anterior
    let ir = at(TransitionKind::Iris, TransitionAlign::Center, 20);
    assert!(near(ir.get(20, 10), [0.0, 0.0, 1.0], 1e-6) && near(ir.get(0, 0), [1.0, 0.0, 0.0], 1e-6));
    // fade do preto do v1: age só no novo, a partir do corte (25 = metade)
    let fb = at(TransitionKind::FadeBlack, TransitionAlign::Center, 25);
    assert!(near(fb.get(5, 5), [0.0, 0.0, 0.5], 0.01), "{:?}", fb.get(5, 5));
}

#[test]
fn opacity_and_upper_video_tracks_composite() {
    let s = color_project();
    let mut d = EditDoc::with_default_tracks();
    let vt: Vec<u32> = d.tracks_of(TrackKind::Video).iter().map(|t| t.id).collect();
    d.items.push(color(1, vt[0], 0, 10, [1.0, 0.0, 0.0, 1.0]));
    let mut top = color(2, vt[1], 0, 10, [0.0, 1.0, 0.0, 1.0]);
    top.opacity = 0.25;
    d.items.push(top);
    let im = render_edit_frame(&s.project, &s.blobs, &d, 3, false).unwrap();
    let p = im.get(4, 4);
    assert!((p[0] - 0.75).abs() < 1e-4 && (p[1] - 0.25).abs() < 1e-4, "{p:?}");
    // trilha escondida não aparece
    d.track_mut(vt[1]).unwrap().hidden = true;
    assert_eq!(render_edit_frame(&s.project, &s.blobs, &d, 3, false).unwrap().get(4, 4), [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn razor_ripple_insert_overwrite_slip_slide_roll() {
    let mut d = EditDoc::with_default_tracks();
    let v = d.tracks_of(TrackKind::Video)[0].id;
    let a1 = d.tracks_of(TrackKind::Audio)[0].id;
    let s1 = d.tracks_of(TrackKind::Subtitle)[0].id;
    let red = [1.0, 0.0, 0.0, 1.0];
    d.items.push(color(1, v, 0, 20, red));
    d.items.push(color(2, v, 20, 10, red));
    // som vinculado ao item 1
    let mut snd = EditItem::new(3, a1, 0, 20, ItemSource::Audio { asset: Id::from("x") });
    snd.link = Some(1);
    d.items[0].link = Some(1);
    d.items.push(snd);
    // legenda presa ao item 2
    let mut cap = EditItem::new(4, s1, 22, 5, ItemSource::Caption { text: "oi".into(), speaker: None, style: None });
    cap.anchor = Some(2);
    d.items.push(cap);

    // navalha no item 1 em 8: divide a imagem e o som vinculado; as partes direitas continuam vinculadas entre si
    let new = d.razor(8, Some(&[1]));
    assert_eq!(new.len(), 2);
    let (l, r) = (d.item(1).unwrap().clone(), d.item(new[0]).unwrap().clone());
    assert_eq!((l.start, l.dur, r.start, r.dur, r.src_in), (0, 8, 8, 12, 8));
    let rs = d.item(new[1]).unwrap();
    assert_eq!((rs.start, rs.dur, rs.src_in), (8, 12, 8));
    assert!(r.link.is_some() && r.link == rs.link && r.link != l.link);

    // apagar com ripple a parte direita (e o som dela): o item 2 e a legenda presa puxam 12 quadros
    let right = d.with_linked(&[new[0]]);
    d.delete_items(&right, true);
    assert_eq!(d.item(2).unwrap().start, 8);
    assert_eq!(d.item(4).unwrap().start, 10, "a legenda anda junto (ripple em todas as trilhas)");

    // inserir 5 quadros em 4: divide o item 1, empurra tudo
    let ins = d.insert_items(vec![color(0, v, 4, 5, [0.0, 0.0, 1.0, 1.0])], 4);
    assert_eq!(ins.len(), 1);
    let pics: Vec<(u32, u32)> = d.items_on(v).iter().map(|i| (i.start, i.dur)).collect();
    assert_eq!(pics, vec![(0, 4), (4, 5), (9, 4), (13, 10)]);
    assert_eq!(d.item(4).unwrap().start, 15);

    // sobrescrever 3 quadros em 10: apara o pedaço depois do inserido e não muda o total
    let before_end = d.content_end();
    d.overwrite_items(vec![color(0, v, 10, 3, [0.0, 1.0, 0.0, 1.0])]);
    let pics: Vec<(u32, u32)> = d.items_on(v).iter().map(|i| (i.start, i.dur)).collect();
    assert_eq!(pics, vec![(0, 4), (4, 5), (9, 1), (10, 3), (13, 10)]);
    assert_eq!(d.content_end(), before_end);

    // aparar a saída com ripple: o que vem depois anda
    let last = d.items_on(v).last().unwrap().id;
    let mid = d.items_on(v)[3].id;
    assert_eq!(d.trim(&[mid], Edge::Out, 2, true, &no_media_limit), 2);
    assert_eq!(d.item(last).unwrap().start, 15);
    // sem ripple, a saída para no vizinho
    assert_eq!(d.trim(&[mid], Edge::Out, 5, false, &no_media_limit), 0);

    // roll entre `mid` e `last`: duração total igual; para trás não dá (o seguinte não tem alça antes da entrada)
    let total = d.content_end();
    assert_eq!(d.roll(mid, -2, &no_media_limit), 0);
    assert_eq!(d.roll(mid, 2, &no_media_limit), 2);
    assert_eq!((d.item(mid).unwrap().dur, d.item(last).unwrap().start, d.item(last).unwrap().dur, d.item(last).unwrap().src_in), (7, 17, 8, 2));
    assert_eq!(d.content_end(), total);

    // slip: muda só a entrada na mídia (limitada pela mídia)
    let media = |s: &ItemSource| matches!(s, ItemSource::Color { .. }).then_some(30);
    let src0 = d.item(last).unwrap().src_in;
    assert_eq!(d.slip(&[last], 4, &media), 4);
    assert_eq!((d.item(last).unwrap().start, d.item(last).unwrap().src_in), (17, src0 + 4));
    assert_eq!(d.slip(&[last], 100, &media), 30 - (src0 + 4 + 8) as i64, "para no fim da mídia");

    // slide: o item anda entre os vizinhos; o total fica igual
    let total = d.content_end();
    let order: Vec<u32> = d.items_on(v).iter().map(|i| i.id).collect();
    let mid = order[3];
    assert_eq!(d.slide(mid, 1, &no_media_limit), 1);
    let p = |id: u32| (d.item(id).unwrap().start, d.item(id).unwrap().dur);
    assert_eq!((p(order[2]), p(mid), p(order[4])), ((9, 2), (11, 7), (18, 7)));
    assert_eq!(d.content_end(), total);

    // mover para V2 (sobrescreve) e trilha travada recusada
    let v2 = d.tracks_of(TrackKind::Video)[1].id;
    d.move_items(&[mid], TrackKind::Video, 0, 1).unwrap();
    assert_eq!(d.item(mid).unwrap().track, v2);
    d.track_mut(v).unwrap().locked = true;
    assert!(d.move_items(&[order[0]], TrackKind::Video, 3, 0).is_err());
    assert!(d.razor(2, None).iter().all(|id| d.item(*id).unwrap().track != v), "navalha não corta trilha travada");
}

#[test]
fn srt_and_vtt_round_trip() {
    let mut d = EditDoc::with_default_tracks();
    let s1 = d.tracks_of(TrackKind::Subtitle)[0].id;
    let fps = 24000.0 / 1001.0;
    let caps = [(12u32, 36u32, "Olá, mundo!"), (48, 30, "Segunda legenda\ncom duas linhas"), (100, 12, "Ação — çà ü 日本語")];
    for (k, (s, dur, t)) in caps.iter().enumerate() {
        d.items.push(EditItem::new(k as u32 + 1, s1, *s, *dur, ItemSource::Caption { text: (*t).into(), speaker: None, style: None }));
    }
    let subs = captions::track_subtitles(&d, s1, fps);
    let srt = aiai_media::write_srt(&subs);
    assert!(srt.starts_with("1\n00:00:00,500 --> 00:00:02,002\nOlá, mundo!"), "{srt}");
    let back = captions::parse_captions(&srt).unwrap();
    assert_eq!(back.len(), 3);
    let mut e = EditDoc::with_default_tracks();
    let t = e.tracks_of(TrackKind::Subtitle)[0].id;
    captions::import_into(&mut e, t, &back, fps, true).unwrap();
    let got: Vec<(u32, u32, String)> = e.items_on(t).iter().map(|i| (i.start, i.dur, i.caption_text().unwrap().to_owned())).collect();
    let want: Vec<(u32, u32, String)> = caps.iter().map(|(s, d, t)| (*s, *d, (*t).to_owned())).collect();
    assert_eq!(got, want, "SRT ida e volta mantém tempo (em quadros) e texto");
    // VTT
    let vtt = captions::write_vtt(&subs);
    assert!(vtt.starts_with("WEBVTT\n\n00:00:00.500 --> 00:00:02.002"));
    assert_eq!(captions::parse_captions(&vtt).unwrap(), back);
    // tags e CRLF
    let odd = "1\r\n00:00:01,000 --> 00:00:02,500\r\n<i>itálico</i> {\\an8}topo\r\n\r\n";
    assert_eq!(captions::parse_captions(odd).unwrap()[0].text, "itálico topo");
    assert!(captions::parse_captions("lixo sem tempo").is_err());
    // verificação: 11 caracteres em 1,5 s (ok); 0,5 s é curta demais
    let checks = captions::check_captions(&d, Some(s1), fps, &captions::CaptionLimits::default());
    assert!(checks[0].issues.is_empty(), "{:?}", checks[0]);
    assert!(checks[2].issues.contains(&captions::CaptionIssue::TooShort));
}

#[test]
fn markers_toggle_sort_and_navigate() {
    let mut d = EditDoc::default();
    assert!(d.toggle_marker(48));
    assert!(d.toggle_marker(12));
    assert_eq!(d.markers.iter().map(|m| m.frame).collect::<Vec<_>>(), vec![12, 48], "em ordem de quadro");
    assert_eq!(d.marker_near(20, true), Some(48));
    assert_eq!(d.marker_near(20, false), Some(12));
    assert_eq!(d.marker_near(48, true), None);
    assert!(!d.toggle_marker(12), "no mesmo quadro tira");
    let back: EditDoc = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(back.markers, d.markers);
    assert!(!serde_json::to_string(&EditDoc::default()).unwrap().contains("markers"), "sem marcadores não sai no arquivo");
}
