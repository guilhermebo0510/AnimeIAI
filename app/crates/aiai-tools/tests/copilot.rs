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
    let sig = |s: &aiai_core::CoreSession| source_signature(&s.project, &cut, &lay, 0);
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
