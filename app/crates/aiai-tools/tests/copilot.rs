//! Copiloto de traço: as partes que não dependem do gerador (janela de contexto, corte na caixa).
use aiai_tools::copilot::{clip_to_box, context_window, gen_size, gen_zoom};
use aiai_tools::vectorize::InkPath;

#[test]
fn context_window_follows_the_box_shape_at_least_384_inside_the_canvas_and_never_cuts_the_box() {
    // caixa pequena: a janela cresce até 384 em volta dela
    let w = context_window([100.0, 100.0, 50.0, 50.0], 2.0, (1920, 1080));
    assert_eq!((w[2], w[3]), (384, 384));
    assert!(w[0] as f32 <= 100.0 && (w[0] + w[2]) as f32 >= 150.0);
    // perto da borda: empurrada para dentro
    let w = context_window([1900.0, 1060.0, 10.0, 10.0], 2.0, (1920, 1080));
    assert!(w[0] + w[2] <= 1920 && w[1] + w[3] <= 1080);
    // caixa retangular: a janela tem o formato dela (cada lado dobra; nunca menos de 384)
    let w = context_window([500.0, 300.0, 400.0, 250.0], 2.0, (1920, 1080));
    assert_eq!((w[2], w[3]), (800, 500));
    // caixa grande: cada lado dobra; uma maior que o canvas fica limitada a ele
    let w = context_window([500.0, 200.0, 400.0, 300.0], 2.0, (1920, 1080));
    assert_eq!((w[2], w[3]), (800, 600));
    let w = context_window([100.0, 100.0, 900.0, 800.0], 2.0, (1920, 1080));
    assert_eq!(w[3], 1080);
    // caixa mais larga que a altura do canvas: antes (quadrado de 1080) as pontas ficavam de fora
    let b = [200.0, 400.0, 1500.0, 300.0];
    let w = context_window(b, 2.0, (1920, 1080));
    assert!(w[0] as f32 <= b[0] && (w[0] + w[2]) as f32 >= b[0] + b[2], "{w:?}");
    assert!(w[1] as f32 <= b[1] && (w[1] + w[3]) as f32 >= b[1] + b[3], "{w:?}");
}

#[test]
fn generation_keeps_the_window_shape_in_multiples_of_64() {
    // automático: 1:1 até 512 no lado maior
    assert_eq!(gen_size([0, 0, 384, 384], None), [384, 384]);
    assert_eq!(gen_size([0, 0, 900, 700], None), [512, 384]);
    // pedido 768 no lado maior, o menor na proporção
    assert_eq!(gen_size([0, 0, 900, 700], Some(768)), [768, 576]);
    // caixa pequena em 768: o modelo vê o desenho ampliado 2×
    assert!((gen_zoom([0, 0, 384, 384], gen_size([0, 0, 384, 384], Some(768))) - 2.0).abs() < 1e-3);
    // nunca abaixo de 256 nem acima de 768
    let g = gen_size([0, 0, 1920, 400], Some(768));
    assert!(g[0] == 768 && g[1] >= 256, "{g:?}");
}

#[test]
fn a_stroke_crossing_the_box_is_cut_at_its_edge() {
    // reta horizontal de x = 0 a 200 (imagem), escala 1, deslocada 100 no canvas → 100..300; caixa x 150..250
    let p = InkPath { points: vec![[0.0, 50.0], [200.0, 50.0]], widths: vec![3.0, 3.0], closed: false };
    let out = clip_to_box(&p, 1.0, [100.0, 0.0], [150.0, 0.0, 250.0, 100.0]);
    assert_eq!(out.len(), 1);
    let (a, b) = (out[0].points[0], *out[0].points.last().unwrap());
    assert!((a[0] + 100.0 - 150.0).abs() <= 2.5 && (b[0] + 100.0 - 250.0).abs() <= 2.5, "{a:?} {b:?}");
    // entra e sai duas vezes: dois pedaços
    let z = InkPath { points: vec![[0.0, 10.0], [100.0, 10.0], [100.0, 90.0], [0.0, 90.0]], widths: vec![2.0; 4], closed: false };
    let out = clip_to_box(&z, 1.0, [0.0, 0.0], [50.0, 0.0, 200.0, 200.0]);
    assert_eq!(out.len(), 1, "o U inteiro dentro de x ≥ 50 é um pedaço só");
    let out = clip_to_box(&z, 1.0, [0.0, 0.0], [50.0, 0.0, 200.0, 50.0]);
    assert_eq!(out.len(), 1, "só o braço de cima");
    // tudo dentro: igual, e o fechado continua fechado
    let c = InkPath { points: vec![[10.0, 10.0], [40.0, 10.0], [25.0, 40.0]], widths: vec![2.0; 3], closed: true };
    assert_eq!(clip_to_box(&c, 1.0, [0.0, 0.0], [0.0, 0.0, 100.0, 100.0]), vec![c.clone()]);
    // tudo fora: nada
    assert!(clip_to_box(&c, 1.0, [500.0, 500.0], [0.0, 0.0, 100.0, 100.0]).is_empty());
}

/// Melhorar substitui o rascunho: os traços do cel que a versão limpa cobre saem no mesmo passo; os de fora da caixa e
/// os que não batem ficam; desligado, o rascunho fica.
#[test]
fn accepting_improve_replaces_the_rough_strokes_it_covers() {
    use aiai_core::command::CellEdit;
    use aiai_core::*;
    use aiai_tools::copilot::{AcceptWhich, accept_command, covered_strokes};
    let line = |id: &str, pts: &[(f32, f32)], w: f32| Stroke {
        taper: None,
        id: Id::from(id),
        points: pts.iter().map(|&(x, y)| StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
        geometry: StrokeGeometry::Spline,
        tension: 0.0,
        tip: VectorTip::Marker,
        width: w,
        profile: WidthProfile::default(),
        color: ColorRef::default(),
        opacity: 1.0,
        closed: false,
        aliased: false,
        color_trace: false,
        group: String::new(),
    };
    // rascunho: duas passadas tremidas da mesma reta, outra reta longe e uma fora da caixa
    let rough1 = line("r1", &[(100.0, 102.0), (150.0, 98.0), (200.0, 101.0)], 2.0);
    let rough2 = line("r2", &[(102.0, 99.0), (151.0, 103.0), (198.0, 100.0)], 2.0);
    let other = line("o", &[(100.0, 160.0), (200.0, 160.0)], 2.0);
    let outside = line("x", &[(400.0, 100.0), (500.0, 100.0)], 2.0);
    let clean = line("", &[(100.0, 100.0), (150.0, 100.0), (200.0, 100.0)], 3.0);
    let rect = [80.0, 80.0, 150.0, 100.0];
    let cel_strokes = vec![rough1.clone(), rough2.clone(), other.clone(), outside.clone()];
    let (mut got, outside_pieces) = covered_strokes(&cel_strokes, &[&clean], rect);
    got.sort();
    assert_eq!(got, vec![Id::from("r1"), Id::from("r2")]);
    assert!(outside_pieces.is_empty(), "r1 e r2 estão inteiros dentro da caixa");
    // no documento: um passo de desfazer que tira r1 e r2 e põe o limpo
    let mut s = CoreSession::for_tests("cp");
    let mk = |s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind| -> Id {
        let id = s.ids.next();
        s.apply(Command::CreateNode { id: Some(id.clone()), parent: parent.cloned(), index: None, name: name.into(), data }).unwrap();
        id
    };
    let ep = mk(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = mk(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = mk(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(2)));
    let lay = mk(&mut s, Some(&cut), "V", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    let mut cd = CelData::new(1);
    cd.strokes = cel_strokes;
    mk(&mut s, Some(&lay), "V1", NodeKind::Cel(cd));
    s.apply(Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(Id::from("t"), "t", TrackKind::Drawing, Some(lay.clone())) }).unwrap();
    s.apply(Command::SetCells { cut: cut.clone(), track: Id::from("t"), cells: vec![CellEdit { frame: 0, cell: Some(CellRef::Cel { number: 1 }) }] }).unwrap();
    let sg = sug(&cut, &lay, rect, vec![clean.clone()]);
    let undo0 = s.history().0.len();
    s.apply(accept_command(&s.project, &sg, AcceptWhich::All, None, true).unwrap()).unwrap();
    assert_eq!(s.history().0.len(), undo0 + 1, "um passo só");
    let ids_now = |s: &CoreSession| -> Vec<String> {
        let NodeKind::Cel(c) = &s.project.cel_by_number(&lay, 1).unwrap().data else { unreachable!() };
        c.strokes.iter().map(|x| x.id.0.clone()).collect()
    };
    let now = ids_now(&s);
    assert!(!now.contains(&"r1".to_string()) && !now.contains(&"r2".to_string()) && now.contains(&"o".to_string()) && now.contains(&"x".to_string()), "{now:?}");
    assert_eq!(now.len(), 3);
    s.undo().unwrap();
    assert_eq!(ids_now(&s), vec!["r1", "r2", "o", "x"], "desfazer volta o rascunho");
    // replace = false: o rascunho fica e o limpo entra por cima
    s.apply(accept_command(&s.project, &sg, AcceptWhich::All, None, false).unwrap()).unwrap();
    assert_eq!(ids_now(&s).len(), 5);
}

#[test]
fn more_change_means_less_drawing_control_and_more_prompt_weight() {
    use aiai_tools::copilot::change_params;
    let (low, mid, high) = (change_params(0.1), change_params(0.5), change_params(0.9));
    // Pouca: quase copia (sem peso de prompt, controle até quase o fim); Muita: o desenho só guia o começo
    assert!(low.cfg <= 1.0 && low.strength >= 0.8 && low.control_end >= 0.85, "{low:?}");
    assert!(high.cfg > 2.0 && high.strength < 0.5 && high.control_end < 0.5, "{high:?}");
    assert!(low.strength > mid.strength && mid.strength > high.strength);
    assert!(low.control_end > mid.control_end && mid.control_end > high.control_end);
    assert!(low.cfg < mid.cfg && mid.cfg < high.cfg);
    // fora da faixa e NaN não quebram
    assert_eq!(change_params(-1.0), change_params(0.0));
    assert_eq!(change_params(5.0), change_params(1.0));
    assert!(change_params(f32::NAN).steps >= 1);
}

fn sug(cut: &aiai_core::Id, lay: &aiai_core::Id, rect: [f32; 4], improve: Vec<aiai_core::Stroke>) -> aiai_tools::copilot::Suggestion {
    aiai_tools::copilot::Suggestion {
        id: 1,
        cut: cut.clone(),
        frame: 0,
        layer: lay.clone(),
        rect,
        improve,
        complete: vec![],
        ms: 0,
        gen_ms: 0,
        vec_ms: 0,
        window: [0, 0, 384, 384],
        gen_size: [384, 384],
        cel_rect: rect,
        placement: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        cel: None,
        cel_sig: 0,
        src_sig: 0,
        src_region: [0.0; 4],
        prompt_tokens: 0,
        prompt_truncated: false,
        reduced: false,
        throttled: false,
    }
}

fn stroke(id: &str, pts: &[(f32, f32)], w: f32) -> aiai_core::Stroke {
    use aiai_core::*;
    Stroke {
        taper: None,
        id: Id::from(id),
        points: pts.iter().map(|&(x, y)| StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
        geometry: StrokeGeometry::Spline,
        tension: 0.0,
        tip: VectorTip::Marker,
        width: w,
        profile: WidthProfile::default(),
        color: ColorRef::default(),
        opacity: 1.0,
        closed: false,
        aliased: false,
        color_trace: false,
        group: String::new(),
    }
}

/// Projeto mínimo: EP/S/corte (2 quadros)/camada vetorial com um cel exposto no quadro 0.
fn doc(cel_strokes: Vec<aiai_core::Stroke>) -> (aiai_core::CoreSession, aiai_core::Id, aiai_core::Id) {
    use aiai_core::command::CellEdit;
    use aiai_core::*;
    let mut s = CoreSession::for_tests("cp");
    let mk = |s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind| -> Id {
        let id = s.ids.next();
        s.apply(Command::CreateNode { id: Some(id.clone()), parent: parent.cloned(), index: None, name: name.into(), data }).unwrap();
        id
    };
    let ep = mk(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = mk(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = mk(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(2)));
    let lay = mk(&mut s, Some(&cut), "V", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    let mut cd = CelData::new(1);
    cd.strokes = cel_strokes;
    mk(&mut s, Some(&lay), "V1", NodeKind::Cel(cd));
    s.apply(Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(Id::from("t"), "t", TrackKind::Drawing, Some(lay.clone())) }).unwrap();
    s.apply(Command::SetCells { cut: cut.clone(), track: Id::from("t"), cells: vec![CellEdit { frame: 0, cell: Some(CellRef::Cel { number: 1 }) }] }).unwrap();
    (s, cut, lay)
}

fn cel_of(s: &aiai_core::CoreSession, lay: &aiai_core::Id) -> Vec<aiai_core::Stroke> {
    let aiai_core::NodeKind::Cel(c) = &s.project.cel_by_number(lay, 1).unwrap().data else { unreachable!() };
    c.strokes.clone()
}

#[test]
fn replacing_never_erases_the_part_of_a_rough_stroke_outside_the_box() {
    // CP1-05: rascunho de x=0 a 1000; caixa de x=0 a 800; o limpo cobre 0..800. Antes, o traço inteiro saía.
    use aiai_tools::copilot::{AcceptWhich, accept_command};
    let rough = stroke("r", &[(0.0, 50.0), (500.0, 50.0), (1000.0, 50.0)], 3.0);
    let clean = stroke("", &[(0.0, 50.0), (400.0, 50.0), (800.0, 50.0)], 3.0);
    let rect = [-10.0, 0.0, 810.0, 100.0];
    let (mut s, cut, lay) = doc(vec![rough]);
    s.apply(accept_command(&s.project, &sug(&cut, &lay, rect, vec![clean]), AcceptWhich::All, None, true).unwrap()).unwrap();
    let now = cel_of(&s, &lay);
    assert!(!now.iter().any(|x| x.id.0 == "r"), "o rascunho dentro da caixa sai");
    // o que estava fora da caixa (800..1000) continua desenhado, no mesmo lugar
    let outside: Vec<_> = now.iter().flat_map(aiai_paint::sample_stroke).filter(|q| q.x > 802.0).collect();
    assert!(outside.iter().any(|q| q.x > 990.0) && outside.iter().all(|q| (q.y - 50.0).abs() < 1.0), "pedaço de fora perdido: {} amostras", outside.len());
    let span = outside.iter().map(|q| q.x).fold(f32::MIN, f32::max) - outside.iter().map(|q| q.x).fold(f32::MAX, f32::min);
    assert!(span > 180.0, "{span}");
    // um passo de desfazer devolve o rascunho inteiro
    s.undo().unwrap();
    assert_eq!(cel_of(&s, &lay).len(), 1);
}

#[test]
fn split_keeps_the_drawn_shape_and_the_cut_curve_stays_inside_the_box() {
    use aiai_tools::copilot::split_stroke;
    // CP1-35 (contraprova do Codex): controles todos dentro de [0,0,150,100], mas a spline desce até y = -4,8
    let st = stroke("s", &[(0.0, 50.0), (50.0, 0.0), (100.0, 0.0), (150.0, 50.0)], 2.0);
    assert!(aiai_paint::sample_stroke(&st).iter().any(|q| q.y < -1.0), "o caso precisa sair da caixa");
    let (ins, outs) = split_stroke(&st, |x, y| (0.0..=150.0).contains(&x) && (0.0..=100.0).contains(&y));
    assert!(!ins.is_empty() && !outs.is_empty());
    for p in &ins {
        for q in aiai_paint::sample_stroke(p) {
            assert!(q.y > -1.0, "pedaço de dentro saiu da caixa: y = {}", q.y);
        }
    }
    // a forma não muda: cada amostra dos pedaços está a menos de 1 px da curva original
    let orig = aiai_paint::sample_stroke(&st);
    for p in ins.iter().chain(&outs) {
        for q in aiai_paint::sample_stroke(p) {
            let d = orig.iter().map(|o| (o.x - q.x).hypot(o.y - q.y)).fold(f32::MAX, f32::min);
            assert!(d < 1.0, "{d}");
        }
    }
    // traço todo de um lado volta intacto (mesmo id)
    let (ins, outs) = split_stroke(&st, |_, _| true);
    assert_eq!((ins.len(), outs.len(), ins[0].id.0.as_str()), (1, 0, "s"));
}

#[test]
fn a_stale_suggestion_is_refused_and_a_fresh_one_is_accepted() {
    // CP1-21: a sugestão guarda o cel e a assinatura dele; o desenho mudou → recusa
    use aiai_tools::copilot::{cel_signature, check_fresh};
    let (mut s, cut, lay) = doc(vec![stroke("a", &[(0.0, 0.0), (10.0, 10.0)], 2.0)]);
    let cel = s.project.cel_by_number(&lay, 1).unwrap().id.clone();
    let mut g = sug(&cut, &lay, [0.0, 0.0, 100.0, 100.0], vec![]);
    g.cel = Some(cel.clone());
    g.cel_sig = cel_signature(&s.project, &cel);
    assert!(check_fresh(&s.project, &g).is_ok());
    s.apply(aiai_core::Command::RemoveStrokes { cel: cel.clone(), ids: vec![aiai_core::Id::from("a")] }).unwrap();
    assert!(check_fresh(&s.project, &g).unwrap_err().to_string().contains("velha"));
    // e voltar (desfazer) volta a valer
    s.undo().unwrap();
    assert!(check_fresh(&s.project, &g).is_ok());
}

#[test]
fn a_request_with_a_foreign_layer_a_frame_out_of_the_cut_or_a_box_off_canvas_is_refused_before_generating() {
    // CP1-22: nada disso chega à GPU
    use aiai_tools::copilot::{SuggestRequest, prepare};
    let (s, cut, lay) = doc(vec![stroke("a", &[(10.0, 10.0), (90.0, 90.0)], 2.0)]);
    let base = SuggestRequest {
        cut: cut.clone(),
        frame: 0,
        layer: lay.clone(),
        rect: [0.0, 0.0, 100.0, 100.0],
        prompt: String::new(),
        improve: true,
        complete: true,
        steps: None,
        res: None,
        change: None,
        strength: None,
        control_end: None,
        cfg: None,
        init: None,
        seed: None,
        context: None,
        tip: aiai_core::VectorTip::Marker,
        color: aiai_core::ColorRef::default(),
    };
    assert!(prepare(&s.project, &s.blobs, &base).is_ok());
    let err = |r: SuggestRequest| prepare(&s.project, &s.blobs, &r).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(err(SuggestRequest { frame: 5, ..base.clone() }).contains("fora do corte"));
    assert!(err(SuggestRequest { rect: [5000.0, 5000.0, 100.0, 100.0], ..base.clone() }).contains("fora do desenho"));
    assert!(err(SuggestRequest { improve: false, complete: false, ..base.clone() }).contains("nem Melhorar"));
    assert!(!err(SuggestRequest { layer: cut.clone(), ..base.clone() }).is_empty(), "o próprio corte não é camada");
}

#[test]
fn a_box_signature_ignores_a_rename_and_changes_when_its_cel_is_drawn_on_or_moved() {
    // I-08: só as caixas cujo desenho lido mudou voltam para a fila
    use aiai_tools::copilot::source_signature;
    let (mut s, cut, lay) = doc(vec![stroke("a", &[(0.0, 0.0), (10.0, 10.0)], 2.0)]);
    let sig = |s: &aiai_core::CoreSession| source_signature(&s.project, &cut, &lay, 0, None);
    let s0 = sig(&s);
    s.apply(aiai_core::Command::SetProps { id: lay.clone(), name: Some("outro nome".into()), data: serde_json::Value::Null }).unwrap();
    assert_eq!(sig(&s), s0, "renomear a camada não muda o desenho");
    let cel = s.project.cel_by_number(&lay, 1).unwrap().id.clone();
    s.apply(aiai_core::Command::RemoveStrokes { cel: cel.clone(), ids: vec![aiai_core::Id::from("a")] }).unwrap();
    let s1 = sig(&s);
    assert_ne!(s1, s0, "apagar um traço muda");
    s.apply(aiai_core::Command::SetProps { id: cel, name: None, data: serde_json::json!({"transform": [50.0, 0.0, 1.0, 0.0]}) }).unwrap();
    assert_ne!(sig(&s), s1, "mover o cel muda");
}

/// Sugestão "de verdade" para a guarda: cel, assinaturas e a janela lida, como o `prepare` deixa.
fn fresh_sug(s: &aiai_core::CoreSession, cut: &aiai_core::Id, lay: &aiai_core::Id, rect: [f32; 4], region: [f32; 4], placement: [f32; 6]) -> aiai_tools::copilot::Suggestion {
    use aiai_tools::copilot::{cel_signature, source_signature};
    let mut g = sug(cut, lay, rect, vec![stroke("", &[(rect[0] + 10.0, rect[1] + 10.0), (rect[0] + 40.0, rect[1] + 30.0)], 2.0)]);
    let cel = s.project.cel_by_number(lay, 1).unwrap().id.clone();
    g.cel_sig = cel_signature(&s.project, &cel);
    g.cel = Some(cel);
    g.placement = placement;
    g.src_region = region;
    g.src_sig = source_signature(&s.project, cut, lay, 0, Some(region));
    g
}

#[test]
fn a_stroke_far_from_the_box_keeps_the_suggestion_fresh_and_one_inside_the_window_makes_it_stale() {
    // CP2-01/CP2-04: a guarda olha o que a sugestão leu (a janela), não o cel inteiro
    use aiai_tools::copilot::check_fresh;
    let (mut s, cut, lay) = doc(vec![stroke("a", &[(100.0, 100.0), (150.0, 140.0)], 2.0)]);
    let g = fresh_sug(&s, &cut, &lay, [80.0, 80.0, 100.0, 100.0], [0.0, 0.0, 400.0, 400.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    assert!(check_fresh(&s.project, &g).is_ok());
    let cel = s.project.cel_by_number(&lay, 1).unwrap().id.clone();
    let ins = |st| aiai_core::Command::InsertStrokes { cel: cel.clone(), items: vec![aiai_core::command::IndexedStroke { index: 1, stroke: st }] };
    s.apply(ins(stroke("longe", &[(1500.0, 900.0), (1600.0, 950.0)], 2.0))).unwrap();
    assert!(check_fresh(&s.project, &g).is_ok(), "traço longe da janela não deixa a sugestão velha");
    s.apply(ins(stroke("perto", &[(200.0, 200.0), (260.0, 230.0)], 2.0))).unwrap();
    assert!(check_fresh(&s.project, &g).unwrap_err().to_string().contains("velha"), "traço na janela deixa");
}

#[test]
fn moving_the_layer_or_changing_a_palette_makes_the_suggestion_stale() {
    // CP2-04: deslocar a camada (sem mexer no cel) muda o que o modelo leu; CP2-09: a cor de paleta também
    use aiai_tools::copilot::check_fresh;
    let (mut s, cut, lay) = doc(vec![stroke("a", &[(100.0, 100.0), (150.0, 140.0)], 2.0)]);
    let g = fresh_sug(&s, &cut, &lay, [80.0, 80.0, 100.0, 100.0], [0.0, 0.0, 400.0, 400.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    s.apply(aiai_core::Command::SetProps { id: lay.clone(), name: None, data: serde_json::json!({"offset": [40.0, 0.0]}) }).unwrap();
    assert!(check_fresh(&s.project, &g).is_err(), "camada deslocada");
    s.undo().unwrap();
    assert!(check_fresh(&s.project, &g).is_ok());
    let pal = s.ids.next();
    s.apply(aiai_core::Command::CreateNode { id: Some(pal), parent: None, index: None, name: "P".into(), data: aiai_core::NodeKind::Palette(Default::default()) }).unwrap();
    assert!(check_fresh(&s.project, &g).is_err(), "paleta nova/alterada");
}

#[test]
fn with_a_rotated_layer_the_box_is_its_exact_shape_not_the_bounding_rectangle() {
    // CP2-03 (contraprova do Codex): caixa [450,450,100,100] no canvas, camada girada 45 graus em volta de (500,500);
    // o ponto do cel (568,568) está no retângulo que envolve, mas no canvas fica 46 px fora da caixa
    let a = std::f32::consts::FRAC_PI_4;
    let (c, sn) = (a.cos(), a.sin());
    let placement = [c, -sn, 500.0 - c * 500.0 + sn * 500.0, sn, c, 500.0 - sn * 500.0 - c * 500.0];
    let mut g = sug(&aiai_core::Id::from("c"), &aiai_core::Id::from("l"), [450.0, 450.0, 100.0, 100.0], vec![]);
    g.placement = placement;
    let (cx, cy) = g.to_canvas(568.0, 568.0);
    assert!(cy > 550.0 + 40.0, "o ponto cai fora da caixa no canvas: ({cx}, {cy})");
    assert!(!g.inside(568.0, 568.0), "fora da forma exata");
    assert!(g.inside(500.0, 500.0), "o centro está dentro");
}

#[test]
fn a_box_entirely_off_the_canvas_is_refused_even_when_the_layer_is_moved() {
    // CP2-03: o domínio é conferido no canvas (antes, a caixa levada ao cel por um deslocamento passava)
    use aiai_tools::copilot::{SuggestRequest, prepare};
    let (mut s, cut, lay) = doc(vec![stroke("a", &[(10.0, 10.0), (90.0, 90.0)], 2.0)]);
    let cel = s.project.cel_by_number(&lay, 1).unwrap().id.clone();
    s.apply(aiai_core::Command::SetProps { id: cel, name: None, data: serde_json::json!({"transform": [200.0, 0.0, 1.0, 0.0]}) }).unwrap();
    let w = s.project.settings.width as f32;
    let req = SuggestRequest {
        cut: cut.clone(),
        frame: 0,
        layer: lay.clone(),
        rect: [w + 30.0, 100.0, 100.0, 100.0],
        prompt: String::new(),
        improve: true,
        complete: true,
        steps: None,
        res: None,
        change: None,
        strength: None,
        control_end: None,
        cfg: None,
        init: None,
        seed: None,
        context: None,
        tip: aiai_core::VectorTip::Marker,
        color: aiai_core::ColorRef::default(),
    };
    assert!(prepare(&s.project, &s.blobs, &req).err().map(|e| e.to_string()).unwrap_or_default().contains("fora do desenho"));
}

#[test]
fn accepting_into_the_clean_layer_keeps_the_strokes_where_the_ghost_was() {
    // CP2-02: o rascunho com o cel deslocado 200 px; a sugestão está no espaço do cel dele; a camada limpa nasce sem
    // movimento, então os traços entram 200 px à direita (onde o fantasma aparecia no canvas)
    use aiai_tools::copilot::{AcceptTarget, AcceptWhich, accept_command_to};
    let (mut s, cut, lay) = doc(vec![stroke("a", &[(100.0, 100.0), (150.0, 140.0)], 2.0)]);
    let cel = s.project.cel_by_number(&lay, 1).unwrap().id.clone();
    s.apply(aiai_core::Command::SetProps { id: cel, name: None, data: serde_json::json!({"transform": [200.0, 0.0, 1.0, 0.0]}) }).unwrap();
    let placement = aiai_tools::copilot::placement_of(&s.project, aiai_tools::context::cut_data(&s.project, &cut).unwrap(), &lay, 0).unwrap();
    assert!((placement.apply(0.0, 0.0).0 - 200.0).abs() < 1e-3);
    let mut g = sug(&cut, &lay, [280.0, 80.0, 100.0, 100.0], vec![stroke("", &[(100.0, 100.0), (150.0, 140.0)], 2.0)]);
    g.placement = placement.0;
    let cmd = {
        let ss = &mut s;
        accept_command_to(&ss.project, &g, AcceptWhich::All, None, AcceptTarget::Clean, false, &mut ss.ids).unwrap()
    };
    s.apply(cmd).unwrap();
    let clean = aiai_tools::copilot::clean_layer_of(&s.project, &lay).unwrap();
    let now = cel_of(&s, &clean);
    assert_eq!(now.len(), 1);
    assert!((now[0].points[0].x - 300.0).abs() < 1e-3 && (now[0].points[0].y - 100.0).abs() < 1e-3, "{:?}", (now[0].points[0].x, now[0].points[0].y));
}

#[test]
fn a_partial_accept_removes_only_what_was_accepted() {
    // CP2-05: índices na ordem de `which` (All = Melhorar e depois Completar)
    use aiai_tools::copilot::{AcceptWhich, strip_accepted};
    let st = |id: &str| stroke(id, &[(0.0, 0.0), (10.0, 10.0)], 2.0);
    let base = || {
        let mut g = sug(&aiai_core::Id::from("c"), &aiai_core::Id::from("l"), [0.0; 4], vec![st("i0"), st("i1")]);
        g.complete = vec![st("c0"), st("c1")];
        g
    };
    let ids = |g: &aiai_tools::copilot::Suggestion| (g.improve.iter().map(|x| x.id.0.clone()).collect::<Vec<_>>(), g.complete.iter().map(|x| x.id.0.clone()).collect::<Vec<_>>());
    let mut g = base();
    assert!(!strip_accepted(&mut g, AcceptWhich::Improve, None));
    assert_eq!(ids(&g), (vec![], vec!["c0".to_string(), "c1".to_string()]));
    let mut g = base();
    assert!(!strip_accepted(&mut g, AcceptWhich::Complete, Some(&[1])));
    assert_eq!(ids(&g), (vec!["i0".to_string(), "i1".to_string()], vec!["c0".to_string()]));
    let mut g = base();
    assert!(!strip_accepted(&mut g, AcceptWhich::All, Some(&[1, 2])));
    assert_eq!(ids(&g), (vec!["i0".to_string()], vec!["c1".to_string()]));
    let mut g = base();
    assert!(strip_accepted(&mut g, AcceptWhich::All, None));
}

#[test]
fn a_line_drawn_at_the_edge_of_the_canvas_on_a_moved_cel_is_read_as_context() {
    // CP-CX03 (parcial): cel deslocado +300 px; o traço desenhado na beira esquerda do canvas (x 100..200) mora em
    // x −200..−100 do cel. Antes, o contexto só lia [0, w] do cel e a caixa ali via papel em branco.
    use aiai_tools::copilot::{SuggestRequest, prepare};
    let (mut s, cut, lay) = doc(vec![stroke("a", &[(-200.0, 120.0), (-100.0, 180.0)], 4.0)]);
    let cel = s.project.cel_by_number(&lay, 1).unwrap().id.clone();
    s.apply(aiai_core::Command::SetProps { id: cel, name: None, data: serde_json::json!({"transform": [300.0, 0.0, 1.0, 0.0]}) }).unwrap();
    let req = SuggestRequest {
        cut: cut.clone(),
        frame: 0,
        layer: lay.clone(),
        rect: [60.0, 60.0, 200.0, 200.0],
        prompt: String::new(),
        improve: true,
        complete: true,
        steps: None,
        res: None,
        change: None,
        strength: None,
        control_end: None,
        cfg: None,
        init: None,
        seed: None,
        context: None,
        tip: aiai_core::VectorTip::Marker,
        color: aiai_core::ColorRef::default(),
    };
    let prep = prepare(&s.project, &s.blobs, &req).unwrap();
    let (img, win) = prep.context();
    assert!(win[0] < 0, "a janela do cel começa em x negativo: {win:?}");
    let ink = img.pixels().filter(|q| q.0[0] < 128).count();
    assert!(ink > 50, "o traço aparece no contexto: {ink} px de tinta, janela {win:?}");
}


#[test]
fn the_clean_line_welds_to_the_rough_piece_left_outside_the_box() {
    // continuidade na borda (régua do CP-CX02 §8): o rascunho vai de x 0 a 1000 em y 50; a caixa cobre x até 810; a
    // linha limpa sai 3 px acima e termina na borda. Depois de aceitar, a ponta dela está na ponta do pedaço de fora.
    use aiai_tools::copilot::{AcceptWhich, accept_command};
    let rough = stroke("r", &[(0.0, 50.0), (500.0, 50.0), (1000.0, 50.0)], 3.0);
    let clean = stroke("", &[(2.0, 47.0), (400.0, 47.0), (808.0, 47.0)], 3.0);
    let rect = [-10.0, 0.0, 820.0, 100.0];
    let (mut s, cut, lay) = doc(vec![rough]);
    s.apply(accept_command(&s.project, &sug(&cut, &lay, rect, vec![clean]), AcceptWhich::All, None, true).unwrap()).unwrap();
    let now = cel_of(&s, &lay);
    // o pedaço de fora começa na borda (x ~810); a linha limpa termina exatamente lá
    let outside = now.iter().find(|st| st.points.iter().all(|q| q.x > 805.0)).expect("pedaço de fora");
    let start = outside.points[0];
    let accepted = now.iter().find(|st| st.points.iter().any(|q| q.x < 100.0)).expect("linha limpa");
    let end = accepted.points[accepted.points.len() - 1];
    assert!((end.x - start.x).hypot(end.y - start.y) < 0.5, "solda: fim da limpa ({}, {}) × início do pedaço ({}, {})", end.x, end.y, start.x, start.y);
}

#[test]
fn clean_width_has_no_knots() {
    let mut st = stroke("k", &[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (30.0, 0.0), (40.0, 0.0)], 3.0);
    st.points[2].width = 3.0; // encontro de linhas: o vetorizador mediu o triplo
    let st = aiai_tools::copilot::even_width(st);
    assert!(st.points.iter().all(|q| q.width <= 1.35 && q.width >= 0.6), "{:?}", st.points.iter().map(|q| q.width).collect::<Vec<_>>());
}

#[test]
fn curve_with_anchors_outside_and_belly_inside_touches_the_box() {
    // CP3-01: âncoras em x 1000, alças de ±800: o meio da curva passa em x ~400, dentro da janela [0, 0, 512, 512]
    let mut st = stroke("c", &[(1000.0, 100.0), (1000.0, 400.0)], 3.0);
    st.points[0].handle = Some([-800.0, 0.0]);
    st.points[1].handle = Some([800.0, 0.0]);
    let b = aiai_paint::stroke_bounds(&st).unwrap();
    assert!(b[0] < 512.0, "{b:?}");
    // a curva inteiramente longe não toca
    let far = stroke("f", &[(1000.0, 100.0), (1000.0, 400.0)], 3.0);
    assert!(aiai_paint::stroke_bounds(&far).unwrap()[0] > 512.0);
}

#[test]
fn a_cel_moved_1200_px_reads_the_same_ink_as_the_identity_control() {
    // CP3-02 (fixture do CP-CX03): linha local (-1000, 200)→(-900, 200), largura 8, cel +1200 em x: no canvas,
    // (200, 200)→(300, 200). Caixa [100, 100, 400, 400]. O contexto lido tem a mesma tinta que o controle com a linha
    // já em (200, 200)→(300, 200) e transformação identidade.
    use aiai_tools::copilot::{SuggestRequest, prepare};
    let req = |cut: &aiai_core::Id, lay: &aiai_core::Id| SuggestRequest {
        cut: cut.clone(),
        frame: 0,
        layer: lay.clone(),
        rect: [100.0, 100.0, 400.0, 400.0],
        prompt: String::new(),
        improve: true,
        complete: true,
        steps: None,
        res: None,
        change: None,
        strength: None,
        control_end: None,
        cfg: None,
        init: None,
        seed: None,
        context: None,
        tip: aiai_core::VectorTip::Marker,
        color: aiai_core::ColorRef::default(),
    };
    let (mut s, cut, lay) = doc(vec![stroke("a", &[(-1000.0, 200.0), (-900.0, 200.0)], 8.0)]);
    let cel = s.project.cel_by_number(&lay, 1).unwrap().id.clone();
    s.apply(aiai_core::Command::SetProps { id: cel, name: None, data: serde_json::json!({"transform": [1200.0, 0.0, 1.0, 0.0]}) }).unwrap();
    let moved = prepare(&s.project, &s.blobs, &req(&cut, &lay)).unwrap();
    let (c, cut2, lay2) = doc(vec![stroke("a", &[(200.0, 200.0), (300.0, 200.0)], 8.0)]);
    let ident = prepare(&c.project, &c.blobs, &req(&cut2, &lay2)).unwrap();
    let ink = |p: &aiai_tools::copilot::Prepared| {
        let (img, win) = p.context();
        let v: Vec<(u32, u32)> = img.enumerate_pixels().filter(|q| q.2.0[0] < 128).map(|q| (q.0, q.1)).collect();
        (v, win)
    };
    let ((a, wa), (b, wb)) = (ink(&moved), ink(&ident));
    assert!(b.len() > 400, "controle tem a linha: {} px", b.len());
    assert_eq!(a.len(), b.len(), "mesma tinta: janela deslocada {wa:?} × identidade {wb:?}");
    assert_eq!(a, b, "mesma posição na imagem de entrada");
    assert_eq!(wa[2..], wb[2..], "mesmo tamanho de janela");
}

#[test]
fn a_layer_scaled_to_one_percent_is_refused_before_allocating_the_context() {
    // CP4-03: escala 0,01 leva a janela de ~800 px do canvas a ~80 000 px do cel (dezenas de GB): erro, não alocação
    use aiai_tools::copilot::{SuggestRequest, prepare};
    let (mut s, cut, lay) = doc(vec![stroke("a", &[(0.0, 0.0), (100.0, 0.0)], 4.0)]);
    let cel = s.project.cel_by_number(&lay, 1).unwrap().id.clone();
    s.apply(aiai_core::Command::SetProps { id: cel, name: None, data: serde_json::json!({"transform": [0.0, 0.0, 0.01, 0.0]}) }).unwrap();
    let req = SuggestRequest {
        cut,
        frame: 0,
        layer: lay,
        rect: [700.0, 300.0, 392.0, 401.0],
        prompt: String::new(),
        improve: true,
        complete: true,
        steps: None,
        res: None,
        change: None,
        strength: None,
        control_end: None,
        cfg: None,
        init: None,
        seed: None,
        context: None,
        tip: aiai_core::VectorTip::Marker,
        color: aiai_core::ColorRef::default(),
    };
    let e = prepare(&s.project, &s.blobs, &req).err().expect("recusa");
    assert!(e.to_string().contains("reduzida demais"), "{e}");
}
