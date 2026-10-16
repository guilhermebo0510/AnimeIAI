//! S16 (pré-produção) + S19 (produção e revisão): um projeto pequeno do começo ao fim — bíblia, personagens, roteiro
//! JA+PT, Fountain, beats, storyboard de 4 painéis desenhados com as ferramentas de desenho, folha de e-conte, animatic
//! MP4, cortes gerados do storyboard, tabela de cortes, estados automáticos, correções, retake com comparação e
//! relatório de produção. As evidências vão para `sprints/registro/evidencias/S16` e `S19`.

use aiai_core::*;
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::draw::{CurveSpec, DrawRequest, draw_command};
use aiai_tools::fill::{FillRequest, FillSeed, fill_command};
use aiai_tools::preprod::*;
use aiai_tools::production::*;
use serde_json::{Value, json};
use std::path::PathBuf;

// ---------------------------------------------------------------- utilidades

macro_rules! ap {
    ($s:ident, $c:expr) => {{
        let c = $c;
        apply(&mut $s, c)
    }};
}

fn apply(s: &mut CoreSession, c: Command) -> Outcome {
    s.apply(c).unwrap_or_else(|e| panic!("comando falhou: {e}"))
}

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    apply(s, Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).created[0].clone()
}

fn evidence(sub: &str) -> PathBuf {
    let root = std::env::var_os("AIAI_EVIDENCE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../sprints/registro/evidencias"));
    let dir = root.join(sub);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn mini_project() -> (CoreSession, Id, Id) {
    let mut s = CoreSession::for_tests("Hoshizora no Asa");
    s.author = "agent:claude".into();
    apply(
        &mut s,
        Command::SetProjectProps {
            patch: json!({
                "settings": {"width": 960, "height": 540, "fps_num": 24, "fps_den": 1},
                "bible": {
                    "logline": "Duas irmãs descobrem que as estrelas do telhado respondem ao que elas dizem.",
                    "synopsis": "Manhã comum, entardecer incomum.",
                    "world": "Vila rural no Japão atual.",
                    "themes": ["família", "silêncio"],
                    "tone": "contemplativo, leve"
                }
            }),
        },
    );
    let haru = create(&mut s, None, "Haru", NodeKind::Character(CharacterData::default()));
    let rin = create(&mut s, None, "Rin", NodeKind::Character(CharacterData::default()));
    (s, haru, rin)
}

fn curve(v: Value) -> CurveSpec {
    serde_json::from_value(v).unwrap()
}

fn poly(pts: &[[f32; 2]], closed: bool) -> CurveSpec {
    curve(json!({"points": pts, "width": 3.0, "geometry": "polyline", "closed": closed, "tip": "marker"}))
}

fn spline(pts: &[[f32; 2]], closed: bool, w: f32) -> CurveSpec {
    curve(json!({"points": pts, "width": w, "geometry": "spline", "closed": closed}))
}

fn circle(cx: f32, cy: f32, r: f32) -> CurveSpec {
    let pts: Vec<[f32; 2]> = (0..16).map(|i| {
        let a = i as f32 / 16.0 * std::f32::consts::TAU;
        [cx + r * a.cos(), cy + r * a.sin()]
    }).collect();
    spline(&pts, true, 3.0)
}

fn colored(mut c: CurveSpec, hex: &str) -> CurveSpec {
    c.color = ColorRef::Direct { rgba: Rgba::from_hex(hex).unwrap() };
    c
}

fn draw(s: &mut CoreSession, cut: &Id, layer: &Id, frame: u32, curves: Vec<CurveSpec>) {
    let c = draw_command(&s.project, &DrawRequest { cut: cut.clone(), frame, layer: layer.clone(), curves, mirror_x: None }).unwrap();
    apply(s, c);
}

fn cut_of_node(s: &CoreSession, id: &Id) -> CutData {
    match &s.project.node(id).unwrap().data {
        NodeKind::Cut(c) => c.clone(),
        _ => panic!("não é corte"),
    }
}

fn script_request(haru: &Id, rin: &Id) -> ScriptCreateRequest {
    ScriptCreateRequest {
        name: "Roteiro EP01".into(),
        title: "Hoshizora no Asa".into(),
        language: Some("ja".into()),
        translation_language: Some("pt-BR".into()),
        scenes: vec![
            SceneSpec {
                heading: "INT. COZINHA — MANHÃ".into(),
                elements: vec![
                    ElementSpec::action("朝の光が台所に差し込む。", "A luz da manhã entra na cozinha."),
                    ElementSpec::dialogue(haru, "おはよう、リン。", "Bom dia, Rin.", Some("alegre"), Some("sorri para a irmã")),
                    ElementSpec::dialogue(rin, "…まだ眠い。", "…Ainda estou com sono.", Some("sonolenta"), Some("boceja")),
                ],
            },
            SceneSpec {
                heading: "EXT. TELHADO — ENTARDECER".into(),
                elements: vec![
                    ElementSpec::action("二人は屋上で空を見上げる。", "As duas olham o céu do telhado."),
                    ElementSpec::dialogue(haru, "見て、星だ！", "Olha, uma estrela!", Some("maravilhada"), None),
                    ElementSpec { kind: ElementKind::Transition, text: "CORTE PARA:".into(), ..Default::default() },
                ],
            },
        ],
    }
}

// ---------------------------------------------------------------- roteiro

#[test]
fn script_edit_timing_and_fountain_roundtrip() {
    let (mut s, haru, rin) = mini_project();
    let mut ids = IdGen::sequential("t-");
    let created = script_create(&s.project, &mut ids, script_request(&haru, &rin)).unwrap();
    ap!(s, created.command);
    let doc = created.id;
    let script = script_read(&s.project, &doc).unwrap();
    assert_eq!(script.scenes.len(), 2);
    assert_eq!(script.scenes.iter().map(|x| x.elements.len()).sum::<usize>(), 6);
    assert_eq!(script.language, "ja");

    // edição por id: um ReplaceData só
    let s1 = script.scenes[0].id.clone();
    let rin_line = script.scenes[0].elements[2].id.clone();
    let haru_line = script.scenes[0].elements[1].id.clone();
    let ed = script_edit(
        &s.project,
        &mut ids,
        &doc,
        vec![
            ScriptOp::InsertElement {
                scene: s1.clone(),
                index: Some(1),
                element: ElementSpec { kind: ElementKind::Parenthetical, character: Some(rin.clone()), text: "bocejando".into(), ..Default::default() },
            },
            ScriptOp::InsertElement { scene: s1.clone(), index: None, element: ElementSpec { kind: ElementKind::Note, text: "manter o plano longo".into(), ..Default::default() } },
            ScriptOp::UpdateElement { element: rin_line.clone(), patch: ElementPatch { text: Some("…まだ、眠い。".into()), est_seconds: Some(2.0), ..Default::default() } },
            ScriptOp::MoveElement { element: haru_line.clone(), scene: s1.clone(), index: Some(0) },
            ScriptOp::InsertScene { index: Some(1), heading: "INT. CORREDOR — MANHÃ".into() },
        ],
    )
    .unwrap();
    assert!(matches!(ed.command, Command::ReplaceData { .. }), "edição = um único ReplaceData");
    assert_eq!(ed.created.len(), 3);
    ap!(s, ed.command);
    let script = script_read(&s.project, &doc).unwrap();
    assert_eq!(script.scenes.len(), 3);
    assert_eq!(script.scenes[0].elements[0].id, haru_line, "elemento movido para o início");
    assert_eq!(script.scenes[0].elements[2].kind, ElementKind::Parenthetical);
    assert_eq!(script.element(&rin_line).unwrap().est_seconds, Some(2.0));
    // apaga a cena vazia e uma nota
    let empty_scene = script.scenes[1].id.clone();
    let note = script.scenes[0].elements.last().unwrap().id.clone();
    let ed = script_edit(&s.project, &mut ids, &doc, vec![ScriptOp::DeleteScene { scene: empty_scene }, ScriptOp::UpdateElement { element: note.clone(), patch: ElementPatch { text: Some("manter o plano longo (PG)".into()), ..Default::default() } }]).unwrap();
    ap!(s, ed.command);
    let script = script_read(&s.project, &doc).unwrap();
    assert_eq!(script.scenes.len(), 2);
    assert!(script_edit(&s.project, &mut ids, &doc, vec![ScriptOp::DeleteElement { element: Id("nao-existe".into()) }]).is_err());

    // ida e volta Fountain (sem ids): preserva cenas, tipos, personagens, JA, tradução, emoção, direção, segundos
    let fountain = script_to_fountain(&s.project, &script);
    assert!(fountain.contains("@Haru"), "nome com minúscula usa @:\n{fountain}");
    assert!(fountain.contains("[[tr: Bom dia, Rin.]]"));
    assert!(fountain.contains("INT. COZINHA — MANHÃ"));
    assert!(fountain.contains("> CORTE PARA:"));
    let mut ids2 = IdGen::sequential("u-");
    let back = fountain_to_script(&s.project, &mut ids2, &fountain);
    assert_eq!(back.without_ids(), script.without_ids(), "fountain:\n{fountain}");
    let again = script_to_fountain(&s.project, &back);
    assert_eq!(again, fountain, "exportar de novo dá o mesmo texto");
    std::fs::write(evidence("S16").join("roteiro.fountain"), &fountain).unwrap();

    // Fountain externo, com nome desconhecido e ação em caixa alta
    let ext = "Title: Teste\n\nINT. SALA - DIA\n\nUm gato dorme.\n\nNARRADOR\n(baixo)\nTudo começou numa terça.\n\nHARU\nBom dia!\n\n> FADE OUT.\n";
    let imp = fountain_to_script(&s.project, &mut ids2, ext);
    assert_eq!(imp.title, "Teste");
    let els = &imp.scenes[0].elements;
    assert_eq!(els.len(), 5);
    assert_eq!(els[1].kind, ElementKind::Parenthetical);
    assert_eq!(els[2].speaker.as_deref(), Some("NARRADOR"));
    assert_eq!(els[2].character, None);
    assert_eq!(els[3].character, Some(haru.clone()), "HARU casa com o personagem Haru sem diferenciar maiúsculas");
    assert_eq!(els[4].kind, ElementKind::Transition);

    // tempo
    assert_eq!(count_mora("おはよう、リン。"), 6);
    assert_eq!(count_mora("きょう"), 2, "kana pequeno funde");
    assert_eq!(count_mora("ありがとう"), 5);
    assert_eq!(count_mora("朝"), 2);
    let rep = estimate_timing(&script);
    let d = script.scenes[0].elements.iter().find(|e| e.text == "おはよう、リン。").unwrap();
    let sec = rep.scenes[0].elements.iter().find(|e| e.element == d.id).unwrap().seconds;
    assert!((sec - 6.0 / 7.5).abs() < 1e-4, "{sec}");
    let act = &script.scenes[0].elements.iter().find(|e| e.kind == ElementKind::Action).unwrap().id;
    assert_eq!(rep.scenes[0].elements.iter().find(|e| &e.element == act).unwrap().seconds, 2.0, "ação sem estimativa = 2 s");
    let explicit = rep.scenes[0].elements.iter().find(|e| e.element == rin_line).unwrap().seconds;
    assert_eq!(explicit, 2.0);
    assert!((rep.total_seconds - rep.scenes.iter().map(|x| x.seconds).sum::<f32>()).abs() < 1e-5);
    let mut en = d.clone();
    en.text = "Good morning to you my friend".into();
    assert!((element_seconds("en", &en) - 6.0 / 2.5).abs() < 1e-5);
    // grava estimativas ausentes
    ap!(s, script_apply_timing(&s.project, &doc).unwrap());
    let script = script_read(&s.project, &doc).unwrap();
    assert!(script.scenes.iter().flat_map(|x| &x.elements).filter(|e| matches!(e.kind, ElementKind::Action | ElementKind::Dialogue)).all(|e| e.est_seconds.is_some()));
}

#[test]
fn beats_board_edit_and_move() {
    let (mut s, _, _) = mini_project();
    let mut ids = IdGen::sequential("b-");
    let board = BeatBoard {
        acts: vec![
            Act { name: "Abertura".into(), beats: vec![Beat { id: ids.next(), title: "Manhã".into(), summary: "A casa acorda".into(), target_seconds: 20.0 }] },
            Act { name: "Conflito".into(), beats: vec![] },
            Act { name: "Fim".into(), beats: vec![] },
        ],
        ..Default::default()
    };
    let c = beats_create(&mut ids, "Beats EP01", board).unwrap();
    ap!(s, c.command);
    let doc = c.id;
    let (cmd, b) = beats_edit(
        &s.project,
        &mut ids,
        &doc,
        vec![
            BeatOp::AddBeat { act: 1, title: "A estrela responde".into(), summary: "Rin ouve".into(), target_seconds: 45.0 },
            BeatOp::AddBeat { act: 2, title: "Silêncio".into(), summary: "".into(), target_seconds: 15.0 },
        ],
    )
    .unwrap();
    ap!(s, cmd);
    assert_eq!(b.total_target_seconds(), 80.0);
    let star = b.acts[1].beats[0].id.clone();
    let (cmd, b) = beats_edit(&s.project, &mut ids, &doc, vec![BeatOp::MoveBeat { beat: star.clone(), act: 0, index: Some(0) }, BeatOp::UpdateBeat { beat: star.clone(), title: None, summary: None, target_seconds: Some(30.0) }]).unwrap();
    ap!(s, cmd);
    assert_eq!(b.acts[0].beats[0].id, star);
    assert!(b.acts[1].beats.is_empty());
    assert_eq!(beats_read(&s.project, &doc).unwrap().total_target_seconds(), 65.0);
    // um documento de roteiro não é quadro de beats
    let sc = script_create(&s.project, &mut ids, ScriptCreateRequest { name: "x".into(), title: "".into(), language: None, translation_language: None, scenes: vec![] }).unwrap();
    ap!(s, sc.command);
    assert!(beats_read(&s.project, &sc.id).is_err());
}

// ---------------------------------------------------------------- tudo junto

fn add_panel(s: &mut CoreSession, ids: &mut IdGen, sb: &Id, spec: PanelSpec, drawing: Vec<CurveSpec>) -> Id {
    let a = storyboard_panel_add(&s.project, ids, &PanelAddRequest { storyboard: sb.clone(), index: None, spec }).unwrap();
    apply(s, a.command);
    assert!(s.project.node(&a.drawing.cut).is_some());
    draw(s, &a.drawing.cut, &a.drawing.layer, 0, drawing);
    a.panel
}

fn wf(cx: f32, cy: f32, zoom: f32) -> CameraFrame {
    CameraFrame { center: [cx, cy], zoom }
}

#[test]
fn preproduction_to_review_end_to_end() {
    let (mut s, haru, rin) = mini_project();
    let mut ids = IdGen::sequential("t-");
    let ev16 = evidence("S16");
    let ev19 = evidence("S19");

    // ---- roteiro
    let created = script_create(&s.project, &mut ids, script_request(&haru, &rin)).unwrap();
    ap!(s, created.command);
    let script_id = created.id;
    let script = created.script;
    let (s1, s2) = (script.scenes[0].id.clone(), script.scenes[1].id.clone());
    let line_haru1 = script.scenes[0].elements[1].id.clone();
    let line_rin = script.scenes[0].elements[2].id.clone();
    let line_haru2 = script.scenes[1].elements[1].id.clone();

    // ---- storyboard
    let sbc = storyboard_create(&mut ids, "Storyboard EP01", "Hoshizora no Asa — EP01", None).unwrap();
    ap!(s, sbc.command);
    let sb = sbc.id;
    assert_eq!(storyboard_read(&s.project, &sb).unwrap().canvas, [480, 270]);

    // painel provisório: testa mover/atualizar/apagar/renumerar e some sem deixar corte órfão
    let scratch = add_panel(&mut s, &mut ids, &sb, PanelSpec { action: "provisório".into(), ..Default::default() }, vec![]);
    let scratch_cut = storyboard_read(&s.project, &sb).unwrap().panel(&scratch).unwrap().drawing_cut.clone().unwrap();
    let before_nodes = s.project.nodes.len();
    ap!(s, storyboard_panel_update(&s.project, &sb, &scratch, &PanelPatch { seconds: Some(1.5), shot: Some(Shot::Detail), clear_camera: true, ..Default::default() }).unwrap());
    assert_eq!(storyboard_read(&s.project, &sb).unwrap().panels[0].seconds, 1.5);
    assert!(storyboard_panel_update(&s.project, &sb, &scratch, &PanelPatch { seconds: Some(-1.0), ..Default::default() }).is_err());
    ap!(s, storyboard_panel_delete(&s.project, &sb, &scratch).unwrap());
    assert!(s.project.node(&scratch_cut).is_none(), "corte de desenho apagado junto");
    assert!(s.project.nodes.len() < before_nodes);
    assert!(storyboard_read(&s.project, &sb).unwrap().panels.is_empty());

    let p1 = add_panel(
        &mut s,
        &mut ids,
        &sb,
        PanelSpec {
            shot: Shot::Wide,
            action: "Casa no campo, manhã. Fumaça sobe da chaminé; o sol ainda baixo.".into(),
            sfx: "ちゅんちゅん (pássaros)".into(),
            seconds: 3.0,
            camera_move: Some(CameraMoveSpec { kind: CameraMoveKind::ZoomIn, from: Some(wf(0.5, 0.5, 1.0)), to: Some(wf(0.45, 0.55, 1.7)) }),
            scene: Some(s1.clone()),
            ..Default::default()
        },
        vec![
            poly(&[[0.0, 190.0], [480.0, 190.0]], false),
            spline(&[[0.0, 205.0], [100.0, 188.0], [200.0, 198.0], [320.0, 190.0], [480.0, 204.0]], false, 2.5),
            poly(&[[150.0, 190.0], [150.0, 130.0], [260.0, 130.0], [260.0, 190.0]], true),
            colored(poly(&[[140.0, 130.0], [205.0, 85.0], [270.0, 130.0]], true), "#8a3a2a"),
            poly(&[[195.0, 190.0], [195.0, 155.0], [220.0, 155.0], [220.0, 190.0]], true),
            poly(&[[235.0, 110.0], [235.0, 82.0], [250.0, 82.0], [250.0, 118.0]], false),
            spline(&[[242.0, 78.0], [254.0, 64.0], [240.0, 50.0], [254.0, 34.0]], false, 2.0),
            colored(circle(405.0, 60.0, 26.0), "#e8892a"),
            poly(&[[60.0, 50.0], [68.0, 58.0], [76.0, 50.0]], false),
            poly(&[[90.0, 38.0], [98.0, 46.0], [106.0, 38.0]], false),
        ],
    );
    let p2 = add_panel(
        &mut s,
        &mut ids,
        &sb,
        PanelSpec {
            shot: Shot::Medium,
            action: "Haru serve chá na cozinha; Rin entra arrastando os pés. Luz da janela no rosto de Haru.".into(),
            dialogue_refs: vec![line_haru1.clone()],
            dialogue_text: "おはよう、リン。\nBom dia, Rin.".into(),
            seconds: 2.5,
            camera_move: Some(CameraMoveSpec { kind: CameraMoveKind::Pan, from: Some(wf(0.38, 0.5, 1.4)), to: Some(wf(0.62, 0.5, 1.4)) }),
            scene: Some(s1.clone()),
            ..Default::default()
        },
        vec![
            circle(150.0, 92.0, 32.0),
            spline(&[[118.0, 80.0], [135.0, 58.0], [168.0, 60.0], [184.0, 82.0]], false, 4.0),
            poly(&[[105.0, 235.0], [118.0, 160.0], [182.0, 160.0], [195.0, 235.0]], true),
            poly(&[[0.0, 215.0], [480.0, 215.0]], false),
            poly(&[[262.0, 215.0], [266.0, 188.0], [292.0, 188.0], [296.0, 215.0]], true),
            poly(&[[330.0, 40.0], [450.0, 40.0], [450.0, 150.0], [330.0, 150.0]], true),
            poly(&[[390.0, 40.0], [390.0, 150.0]], false),
            poly(&[[330.0, 95.0], [450.0, 95.0]], false),
            spline(&[[138.0, 98.0], [150.0, 106.0], [162.0, 98.0]], false, 2.0),
        ],
    );
    let p3 = add_panel(
        &mut s,
        &mut ids,
        &sb,
        PanelSpec {
            shot: Shot::Close,
            angle: Angle::Low,
            action: "Rin, ainda sonolenta, boceja; olhos meio fechados.".into(),
            dialogue_refs: vec![line_rin.clone()],
            dialogue_text: "…まだ眠い。\n…Ainda estou com sono.".into(),
            seconds: 2.0,
            scene: Some(s1.clone()),
            ..Default::default()
        },
        vec![
            circle(240.0, 140.0, 96.0),
            spline(&[[184.0, 122.0], [205.0, 132.0], [226.0, 122.0]], false, 3.5),
            spline(&[[256.0, 122.0], [277.0, 132.0], [298.0, 122.0]], false, 3.5),
            spline(&[[222.0, 182.0], [240.0, 196.0], [258.0, 182.0], [240.0, 170.0]], true, 2.5),
            poly(&[[150.0, 90.0], [190.0, 60.0], [220.0, 84.0], [260.0, 56.0], [300.0, 84.0], [330.0, 62.0]], false),
            poly(&[[372.0, 60.0], [392.0, 60.0], [372.0, 82.0], [392.0, 82.0]], false),
        ],
    );
    let star = |cx: f32, cy: f32, r: f32| -> CurveSpec {
        let pts: Vec<[f32; 2]> = (0..10).map(|i| {
            let a = i as f32 / 10.0 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            [cx + rr * a.cos(), cy + rr * a.sin()]
        }).collect();
        poly(&pts, true)
    };
    let p4 = add_panel(
        &mut s,
        &mut ids,
        &sb,
        PanelSpec {
            shot: Shot::Detail,
            angle: Angle::Bird,
            action: "As duas no telhado, vistas de cima; uma estrela cadente risca o céu.".into(),
            dialogue_refs: vec![line_haru2.clone()],
            dialogue_text: "見て、星だ！\nOlha, uma estrela!".into(),
            sfx: "シュッ".into(),
            seconds: 3.5,
            camera_move: Some(CameraMoveSpec { kind: CameraMoveKind::Tilt, from: Some(wf(0.5, 0.75, 1.5)), to: Some(wf(0.5, 0.28, 1.5)) }),
            scene: Some(s2.clone()),
            ..Default::default()
        },
        vec![
            poly(&[[0.0, 210.0], [480.0, 170.0]], false),
            circle(190.0, 205.0, 15.0),
            circle(250.0, 196.0, 15.0),
            poly(&[[175.0, 222.0], [190.0, 240.0], [205.0, 222.0]], false),
            poly(&[[235.0, 214.0], [250.0, 232.0], [265.0, 214.0]], false),
            star(80.0, 50.0, 14.0),
            star(190.0, 90.0, 10.0),
            star(330.0, 40.0, 12.0),
            colored(poly(&[[420.0, 30.0], [300.0, 120.0]], false), "#d89a1a"),
            colored(star(420.0, 30.0, 16.0), "#d89a1a"),
        ],
    );
    let sbdoc = storyboard_read(&s.project, &sb).unwrap();
    assert_eq!(sbdoc.panels.len(), 4);
    assert_eq!(sbdoc.panels.iter().map(|x| x.cut_number).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    ap!(s, storyboard_renumber(&s.project, &sb, 10).unwrap());
    assert_eq!(storyboard_read(&s.project, &sb).unwrap().panels.iter().map(|x| x.cut_number).collect::<Vec<_>>(), vec![10, 20, 30, 40]);
    ap!(s, storyboard_renumber(&s.project, &sb, 1).unwrap());
    let sbdoc = storyboard_read(&s.project, &sb).unwrap();
    assert_eq!(sbdoc.panels.iter().map(|x| x.cut_number).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    // mover painel: p4 para o início e volta
    ap!(s, storyboard_panel_move(&s.project, &sb, &p4, 0).unwrap());
    assert_eq!(storyboard_read(&s.project, &sb).unwrap().panels[0].id, p4);
    ap!(s, storyboard_panel_move(&s.project, &sb, &p4, 3).unwrap());
    assert_eq!(storyboard_read(&s.project, &sb).unwrap().panels[3].id, p4);
    // desenho foi parar no cel do painel
    let d1 = panel_drawing(&s.project, &sb, &p1).unwrap();
    match &s.project.node(&d1.cel).unwrap().data {
        NodeKind::Cel(c) => assert_eq!(c.strokes.len(), 10),
        _ => panic!(),
    }

    // ---- folha de e-conte
    let sheet = storyboard_export_sheet(&s.project, &s.blobs, &sb, &SheetOptions::default()).unwrap();
    assert!(sheet.width <= 1600 && sheet.width == 1400);
    assert!(sheet.height > 1000);
    assert!(!sheet.is_blank());
    sheet.save_png(&ev16.join("econte.png")).unwrap();
    let narrow = storyboard_export_sheet(&s.project, &s.blobs, &sb, &SheetOptions { width: 700 }).unwrap();
    assert_eq!(narrow.width, 700);
    assert!({ let empty = sbc_empty(&mut s, &mut ids); storyboard_export_sheet(&s.project, &s.blobs, &empty, &SheetOptions::default()).is_err() });

    // ---- animatic
    let plan = animatic_frames(&s.project, &s.blobs, &sb, 24.0).unwrap();
    assert_eq!(plan.iter().map(|x| x.2).collect::<Vec<_>>(), vec![72, 60, 48, 84]);
    assert_eq!(plan.iter().map(|x| x.1).collect::<Vec<_>>(), vec![0, 72, 132, 180]);
    let mp4 = ev16.join("animatic.mp4");
    let rep = render_animatic_opts(&s.project, &s.blobs, &sb, &mp4, 640, 360, &AnimaticOptions { fps: None, captions: true, cut_numbers: true }).unwrap();
    assert_eq!(rep.frames, 264);
    let probe = aiai_media::probe(&mp4).unwrap();
    assert!((probe.duration - 11.0).abs() < 0.1, "duração do animatic {} ≈ soma dos painéis (11 s)", probe.duration);
    assert_eq!((probe.width, probe.height), (Some(640), Some(360)));
    assert_eq!(probe.frames, Some(264));
    let tmp = std::env::temp_dir().join("aiai_preprod_plain.mp4");
    let rep2 = render_animatic(&s.project, &s.blobs, &sb, &tmp, 320, 180).unwrap();
    assert_eq!(rep2.frames, 264);
    let _ = std::fs::remove_file(&tmp);
    // o quadro do meio do primeiro painel tem a casa (não é só branco)
    let f = aiai_media::extract_frame(&mp4, 1.0).unwrap();
    assert!(f.data.iter().filter(|p| p[0] < 0.4 && p[1] < 0.4 && p[2] < 0.4).count() > 300, "traços visíveis no animatic");

    // ---- episódio e cortes a partir do storyboard
    let ep = create(&mut s, None, "EP01", NodeKind::Episode(EpisodeData { number: 1 }));
    let nodes_before = s.project.nodes.clone();
    let cf = cuts_from_storyboard(
        &s.project,
        &mut ids,
        &mut s.blobs,
        &CutsFromStoryboardRequest { storyboard: sb.clone(), episode: ep.clone(), scene_per_panel_group: true, script: Some(script_id.clone()), fps: None },
    )
    .unwrap();
    assert!(matches!(cf.command, Command::Batch { .. }));
    ap!(s, cf.command.clone());
    assert_eq!(cf.scenes.len(), 2, "cena 1 (3 painéis) e cena 2 (1 painel)");
    assert_eq!(cf.cuts.len(), 4);
    let scene_names: Vec<String> = cf.scenes.iter().map(|id| s.project.node(id).unwrap().name.clone()).collect();
    assert_eq!(scene_names, vec!["INT. COZINHA — MANHÃ", "EXT. TELHADO — ENTARDECER"]);
    let by_panel = |pid: &Id| cf.cuts.iter().find(|(p, _)| p == pid).unwrap().1.clone();
    let expected: Vec<(&Id, u32, u32, Vec<Id>)> = vec![(&p1, 1, 72, vec![]), (&p2, 2, 60, vec![line_haru1.clone()]), (&p3, 3, 48, vec![line_rin.clone()]), (&p4, 4, 84, vec![line_haru2.clone()])];
    for (panel, number, dur, links) in expected {
        let c = cut_of_node(&s, &by_panel(panel));
        assert_eq!(c.number, number);
        assert_eq!(c.duration, dur, "duração = segundos × fps");
        assert_eq!(c.dialogue_links, links);
        assert_eq!(c.storyboard_panel.as_ref(), Some(panel));
        assert_eq!(c.notes, sbdoc.panel(panel).unwrap().action);
        assert!(c.canvas.is_none());
    }
    let (c1, c2, c3, c4) = (by_panel(&p1), by_panel(&p2), by_panel(&p3), by_panel(&p4));
    assert_eq!(s.project.node(&c4).unwrap().parent.as_ref(), Some(&cf.scenes[1]));
    // câmera: zoom 1 → 1,7 no corte 1; tilt no corte 4
    let cam1 = cut_of_node(&s, &c1).camera;
    assert_eq!(cam1.keys.0["zoom"].first().unwrap().value, 1.0);
    assert_eq!(cam1.keys.0["zoom"].last().unwrap().value, 1.7);
    assert!(cut_of_node(&s, &c4).camera.keys.0.contains_key("center_y"));
    assert!(cut_of_node(&s, &c3).camera.keys.is_empty());
    // camada de referência com o desenho do painel, no tamanho do projeto
    let ref_layer = s.project.node(&c2).unwrap().children.iter().find(|l| matches!(&s.project.node(l).unwrap().data, NodeKind::Layer(ld) if ld.kind == LayerType::Reference)).unwrap().clone();
    let ref_cel = s.project.node(&ref_layer).unwrap().children[0].clone();
    let NodeKind::Cel(rc) = &s.project.node(&ref_cel).unwrap().data else { panic!() };
    let png = s.blobs.get(rc.image.as_ref().unwrap()).unwrap();
    let ref_img = aiai_paint::Image::decode(&png).unwrap();
    assert_eq!((ref_img.width, ref_img.height), (960, 540));
    assert!(!ref_img.is_blank());
    let plain = render_frame(&s.project, &s.blobs, &c2, 0, &RenderOptions::default()).unwrap().image;
    assert!(plain.is_blank(), "referência não renderiza no final");
    let with_ref = render_frame(&s.project, &s.blobs, &c2, 0, &RenderOptions { include_reference: true, ..Default::default() }).unwrap().image;
    assert!(!with_ref.is_blank(), "mesa de luz mostra a referência");
    with_ref.save_png(&ev16.join("corte2_referencia.png")).unwrap();
    // desfazer remove tudo de uma vez
    s.undo().unwrap();
    assert_eq!(s.project.nodes.keys().collect::<Vec<_>>(), nodes_before.keys().collect::<Vec<_>>());
    s.redo().unwrap();
    assert!(s.project.node(&c1).is_some());
    s.project.validate().unwrap();
    // proveniência: criado pelo agente
    let pv = provenance(&s.project, &c1).unwrap();
    assert_eq!(pv.created_by, "agent:claude");
    assert!(pv.revision > 0);
    let hist = provenance_history(&s.project, s.log(), &c1, 10).unwrap();
    assert!(!hist.is_empty() && hist.iter().all(|e| e.author == "agent:claude"));

    // ================================================================ S19
    // conteúdo mínimo para os estados automáticos
    let mk_layer = |s: &mut CoreSession, cut: &Id, name: &str, ld: LayerData| -> (Id, Id) {
        let l = create(s, Some(cut), name, NodeKind::Layer(ld));
        let cel = create(s, Some(&l), &format!("{name}1"), NodeKind::Cel(CelData::new(1)));
        (l, cel)
    };
    // corte 1: layout com traços
    let mut lo = LayerData::new(LayerType::Vector);
    lo.role = LayerRole::Layout;
    let (lo_layer, _) = mk_layer(&mut s, &c1, "LO", lo);
    draw(&mut s, &c1, &lo_layer, 0, vec![poly(&[[100.0, 400.0], [500.0, 300.0], [860.0, 400.0]], false)]);
    // cortes 2 e 3: linha (duas regiões fechadas) + preenchimento
    let setup_shiage = |s: &mut CoreSession, cut: &Id, paint: usize| -> Id {
        let (line, _) = mk_layer(s, cut, "linha", {
            let mut l = LayerData::new(LayerType::Vector);
            l.role = LayerRole::Shiage;
            l
        });
        draw(s, cut, &line, 0, vec![circle(360.0, 270.0, 90.0), circle(600.0, 270.0, 90.0)]);
        let mut fl = LayerData::new(LayerType::Fill);
        fl.line_refs = vec![line.clone()];
        let (fill_layer, _) = mk_layer(s, cut, "cor", fl);
        let seeds: Vec<FillSeed> = [(360.0, 270.0, "#f2c9a0"), (600.0, 270.0, "#3a5aa0")].iter().take(paint).map(|(x, y, h)| FillSeed { x: *x, y: *y, color: ColorRef::Direct { rgba: Rgba::from_hex(h).unwrap() } }).collect();
        if !seeds.is_empty() {
            let c = fill_command(&s.project, &mut s.blobs, &FillRequest { cut: cut.clone(), frame: 0, layer: fill_layer.clone(), seeds, line_layers: vec![], gap: 3.0, expand: 2, line_threshold: 0.35 }).unwrap();
            apply(s, c);
        }
        line
    };
    let line2 = setup_shiage(&mut s, &c2, 2);
    let _line3 = setup_shiage(&mut s, &c3, 1);

    // tabela de cortes
    let rows = cut_table(&s.project);
    assert_eq!(rows.iter().map(|r| r.number).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    assert_eq!(rows[0].scene_name, "INT. COZINHA — MANHÃ");
    assert_eq!(rows[3].scene_name, "EXT. TELHADO — ENTARDECER");
    assert!(rows.iter().all(|r| r.stages.len() == 8 && r.stages.values().all(|st| *st == StageStatus::NotStarted)));
    let with_thumbs = cut_table_with_thumbs(&s.project, &mut s.blobs, 320).unwrap();
    let thumb = aiai_paint::Image::decode(&s.blobs.get(with_thumbs[1].thumbnail.as_ref().unwrap()).unwrap()).unwrap();
    assert_eq!(thumb.width, 320);

    // estados automáticos
    let sug1 = suggest_stages(&s.project, &s.blobs, &c1).unwrap();
    assert_eq!(sug1.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(Stage::Layout, StageStatus::InProgress), (Stage::Satsuei, StageStatus::InProgress)], "{sug1:?}");
    let sug2 = suggest_stages(&s.project, &s.blobs, &c2).unwrap();
    let shiage2 = sug2.iter().find(|x| x.0 == Stage::Shiage).unwrap();
    assert_eq!(shiage2.1, StageStatus::Review, "lint 0 → pronto para revisão: {}", shiage2.2);
    let sug3 = suggest_stages(&s.project, &s.blobs, &c3).unwrap();
    let shiage3 = sug3.iter().find(|x| x.0 == Stage::Shiage).unwrap();
    assert_eq!(shiage3.1, StageStatus::InProgress, "uma região sem pintar: {}", shiage3.2);
    assert!(shiage3.2.contains("1 região"), "{}", shiage3.2);
    let lint3 = lint_fill_layer(&s.project, &s.blobs, &c3, &s.project.node(&c3).unwrap().children[2]).unwrap();
    assert_eq!((lint3.unpainted_regions, lint3.painted_regions), (1, 1));
    assert!(suggest_stages(&s.project, &s.blobs, &c4).unwrap().iter().all(|x| x.0 != Stage::Shiage));
    // aplica as sugestões e marca etapas manualmente
    for (cut, sug) in [(&c1, &sug1), (&c2, &sug2), (&c3, &sug3)] {
        if let Some(cmd) = apply_stage_suggestions(&s.project, cut, sug).unwrap() {
            ap!(s, cmd);
        }
    }
    assert_eq!(cut_of_node(&s, &c2).stages[&Stage::Shiage], StageStatus::Review);
    ap!(s, set_stage(&s.project, &c1, Stage::Layout, StageStatus::Approved).unwrap());
    ap!(s, set_stage(&s.project, &c1, Stage::Genga, StageStatus::InProgress).unwrap());
    ap!(s, set_stage(&s.project, &c4, Stage::Layout, StageStatus::Retake).unwrap());
    assert_eq!(cut_of_node(&s, &c1).stages[&Stage::Layout], StageStatus::Approved);
    // sugestão nunca rebaixa nem mexe em aprovado/retake
    assert!(suggest_stages(&s.project, &s.blobs, &c1).unwrap().iter().all(|x| x.0 != Stage::Layout));
    assert!(set_stage(&s.project, &Id("nao-existe".into()), Stage::Layout, StageStatus::Review).is_err());

    // correções: duas, em cortes diferentes
    let ca = correction_add(
        &s.project,
        &mut ids,
        &CorrectionRequest {
            cut: c2.clone(),
            frame: 0,
            note: "Contorno do círculo esquerdo está deformado; refazer a linha.".into(),
            strokes: vec![spline(&[[270.0, 270.0], [360.0, 175.0], [450.0, 270.0]], false, 4.0), poly(&[[260.0, 340.0], [460.0, 200.0]], false)],
            author: Some("agent:claude".into()),
        },
    )
    .unwrap();
    ap!(s, ca.command);
    let cb = correction_add(&s.project, &mut ids, &CorrectionRequest { cut: c1.clone(), frame: 12, note: "Perspectiva do layout muito achatada.".into(), strokes: vec![circle(500.0, 300.0, 60.0)], author: None }).unwrap();
    ap!(s, cb.command);
    // segunda correção no mesmo corte e quadro: reaproveita camada e cel
    let cc = correction_add(&s.project, &mut ids, &CorrectionRequest { cut: c2.clone(), frame: 0, note: "Faltou sombra no queixo.".into(), strokes: vec![poly(&[[250.0, 340.0], [350.0, 340.0]], false)], author: None }).unwrap();
    ap!(s, cc.command);
    let corr_layers: Vec<&Node> = s.project.node(&c2).unwrap().children.iter().filter_map(|l| s.project.node(l)).filter(|n| matches!(&n.data, NodeKind::Layer(l) if l.role == LayerRole::Correction)).collect();
    assert_eq!(corr_layers.len(), 1, "uma camada de correção por corte");
    let NodeKind::Layer(cl) = &corr_layers[0].data else { panic!() };
    assert_eq!(cl.kind, LayerType::Guide);
    assert!(cl.kind.is_non_rendering());
    assert_eq!(corr_layers[0].name, "correção");
    let NodeKind::Cel(corr_cel) = &s.project.node(&corr_layers[0].children[0]).unwrap().data else { panic!() };
    assert_eq!(corr_cel.strokes.len(), 3);
    assert!(corr_cel.strokes.iter().all(|st| matches!(&st.color, ColorRef::Direct { rgba } if rgba.0[0] > 0.9 && rgba.0[1] < 0.3)), "traços vermelhos");
    // a correção aparece na mesa com guias e não sai no render final
    let final_img = render_frame(&s.project, &s.blobs, &c2, 0, &RenderOptions::default()).unwrap().image;
    let guides = render_frame(&s.project, &s.blobs, &c2, 0, &RenderOptions { include_guides: true, ..Default::default() }).unwrap().image;
    let reds = |im: &aiai_paint::Image| im.data.iter().filter(|p| p[0] > 0.8 && p[1] < 0.35 && p[2] < 0.35 && p[3] > 0.8).count();
    assert_eq!(reds(&final_img), 0);
    assert!(reds(&guides) > 100, "traços vermelhos só com guias: {}", reds(&guides));
    guides.save_png(&ev19.join("correcao_corte2.png")).unwrap();
    // o timesheet expõe a correção só no quadro dela
    let c2d = cut_of_node(&s, &c2);
    let track = c2d.xsheet.track_for_layer(&corr_layers[0].id).unwrap();
    assert_eq!(track.exposed_at(0), Some(CellRef::Cel { number: 1 }));
    assert_eq!(track.exposed_at(1), Some(CellRef::Empty));
    assert!(correction_add(&s.project, &mut ids, &CorrectionRequest { cut: c2.clone(), frame: 9999, note: "x".into(), strokes: vec![], author: None }).is_err());
    let prod_doc = find_production_doc(&s.project).expect("documento Produção criado");
    assert_eq!(s.project.node(&prod_doc).unwrap().name, "Produção");
    assert_eq!(production_read(&s.project).unwrap().corrections.len(), 3);
    let rows = cut_table(&s.project);
    assert_eq!(rows[1].open_corrections, 2);
    assert_eq!(rows[0].open_corrections, 1);

    // retake: snapshot antes do conserto; conserta; compara
    let rt = retake_create(&s.project, &mut ids, &mut s.blobs, &RetakeRequest { cut: c2.clone(), frame: 0, reason: "Linha do círculo esquerdo deformada (correção 1)".into(), author: Some("agent:claude".into()) }).unwrap();
    ap!(s, rt.command);
    let same = retake_compare(&s.project, &s.blobs, &rt.id, None).unwrap();
    assert_eq!(same.changed_pixels, 0, "sem mudança, sem diferença");
    // refaz a linha do círculo esquerdo (mais firme, por cima da antiga: não divide nenhuma região)
    draw(&mut s, &c2, &line2, 0, vec![curve(json!({"points": (0..16).map(|i| { let a = i as f32 / 16.0 * std::f32::consts::TAU; [360.0 + 90.0 * a.cos(), 270.0 + 90.0 * a.sin()] }).collect::<Vec<_>>(), "width": 8.0, "geometry": "spline", "closed": true}))]);
    let cmp = retake_compare(&s.project, &s.blobs, &rt.id, None).unwrap();
    assert!(cmp.changed_pixels > 500, "mudança detectada: {}", cmp.changed_pixels);
    assert!(cmp.changed_fraction > 0.001 && cmp.changed_fraction < 0.2);
    assert!(cmp.max_diff > 0.5);
    assert_eq!((cmp.before.width, cmp.diff.height), (960, 540));
    assert_ne!(cmp.before.content_hash(), cmp.after.content_hash());
    // snapshot sobrevive à coleta de lixo (está num asset)
    let collected = s.collect_garbage();
    let _ = collected;
    assert!(retake_compare(&s.project, &s.blobs, &rt.id, None).is_ok(), "blob do snapshot preservado pelo asset");
    let prod = production_read(&s.project).unwrap();
    let retake_row = prod.retakes[0].clone();
    assert_eq!(retake_row.cut, c2);
    assert!(retake_row.revision > 0 && retake_row.revision < s.project.revision);
    let sheet = retake_compare_sheet(&cmp, &retake_row, 640);
    sheet.save_png(&ev19.join("retake_compare.png")).unwrap();

    // resolve a correção 1 com o retake; a 2 e a 3 continuam abertas
    ap!(s, correction_resolve(&s.project, &ca.id, true, "agent:claude", Some(rt.id.clone())).unwrap());
    ap!(s, retake_resolve(&s.project, &rt.id, true).unwrap());
    let prod = production_read(&s.project).unwrap();
    assert!(prod.corrections.iter().find(|c| c.id == ca.id).unwrap().resolved);
    assert_eq!(prod.corrections.iter().find(|c| c.id == ca.id).unwrap().retake.as_ref(), Some(&rt.id));
    assert_eq!(prod.retakes[0].correction.as_ref(), Some(&ca.id));
    assert_eq!(prod.corrections.iter().filter(|c| !c.resolved).count(), 2);
    assert_eq!(cut_table(&s.project)[1].open_corrections, 1);
    assert_eq!(cut_table(&s.project)[1].retakes, 1);
    assert_eq!(cut_table(&s.project)[1].open_retakes, 0);
    // reabrir
    ap!(s, correction_resolve(&s.project, &ca.id, false, "x", None).unwrap());
    assert_eq!(cut_table(&s.project)[1].open_corrections, 2);
    ap!(s, correction_resolve(&s.project, &ca.id, true, "agent:claude", Some(rt.id.clone())).unwrap());
    assert!(correction_resolve(&s.project, &Id("nao-existe".into()), true, "x", None).is_err());

    // relatório
    let md = production_report(&s.project, &s.blobs).unwrap();
    std::fs::write(ev19.join("relatorio-producao.md"), &md).unwrap();
    assert!(md.contains("# Relatório de produção — Hoshizora no Asa"));
    assert!(md.contains("## Correções abertas (2 de 3)"), "{md}");
    assert!(md.contains("## Retakes (1)"));
    assert!(md.contains("Faltou sombra no queixo."));
    assert!(md.contains("Perspectiva do layout muito achatada."));
    assert!(md.contains("Contorno do círculo esquerdo"), "resolvida aparece em Resolvidas");
    assert!(md.contains("Corte 4 (C004): etapa(s) em retake: LO"), "{md}");
    assert!(md.contains("Corte 3 / cor: 1 região(ões) sem pintar"), "lint no relatório:\n{md}");
    assert!(md.contains("Corte 2 / cor: 0 região(ões) sem pintar"));
    assert!(md.contains("| Shiage | 2 | 1 | 1 | 0 | 0 | 0% | 25% |"), "{md}");
    s.project.validate().unwrap();
}

/// Storyboard vazio (para checar o erro da folha).
fn sbc_empty(s: &mut CoreSession, ids: &mut IdGen) -> Id {
    let c = storyboard_create(ids, "vazio", "", None).unwrap();
    apply(s, c.command);
    c.id
}
