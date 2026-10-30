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
    use aiai_tools::copilot::{AcceptWhich, Suggestion, accept_command, covered_strokes};
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
    let mut got = covered_strokes(&cel_strokes, &[&clean], rect);
    got.sort();
    assert_eq!(got, vec![Id::from("r1"), Id::from("r2")]);
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
    let sg = Suggestion { id: 1, cut: cut.clone(), frame: 0, layer: lay.clone(), rect, improve: vec![clean.clone()], complete: vec![], ms: 0, gen_ms: 0, vec_ms: 0, window: [0, 0, 384, 384], gen_size: [384, 384] };
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
