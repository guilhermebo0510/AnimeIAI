//! Vetorizador (Copiloto de traço, passo 1): traços conhecidos são desenhados pelo próprio motor do app, a imagem
//! volta ao vetorizador e o resultado é medido contra a verdade (posição da linha central, largura, número de traços,
//! cruzamentos).

use aiai_core::{ColorRef, Id, Stroke, StrokeGeometry, StrokePoint, VectorTip, WidthProfile};
use aiai_paint::{StrokeSample, rasterize_stroke, sample_stroke};
use aiai_tools::vectorize::{InkPath, VectorizeOptions, path_to_stroke, vectorize};

const W: u32 = 400;
const H: u32 = 300;

fn stroke(pts: &[(f32, f32)], tip: VectorTip, width: f32, closed: bool) -> Stroke {
    Stroke { taper: None,
        id: Id::from("t"),
        points: pts.iter().map(|&(x, y)| StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
        geometry: StrokeGeometry::Spline,
        tension: 0.0,
        tip,
        width,
        profile: WidthProfile::default(),
        color: ColorRef::default(),
        opacity: 1.0,
        closed,
        aliased: false,
        color_trace: false,
        group: String::new(),
    }
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32, n: usize) -> Vec<(f32, f32)> {
    (0..n).map(|i| {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        (cx + rx * a.cos(), cy + ry * a.sin())
    }).collect()
}

/// Desenha os traços com o motor do app (cobertura com antisserrilhado) e devolve a tinta e as amostras verdadeiras.
fn draw(strokes: &[Stroke]) -> (Vec<f32>, Vec<Vec<StrokeSample>>) {
    let mut ink = vec![0f32; (W * H) as usize];
    let mut truth = Vec::new();
    for s in strokes {
        let smp = sample_stroke(s);
        let c = rasterize_stroke(&smp, false, W, H).expect("cobertura");
        for yy in 0..c.height {
            for xx in 0..c.width {
                let (x, y) = (c.x0 + xx as i32, c.y0 + yy as i32);
                let i = (y as u32 * W + x as u32) as usize;
                ink[i] = ink[i].max(c.cov[(yy * c.width + xx) as usize]);
            }
        }
        truth.push(smp);
    }
    (ink, truth)
}

/// Pontos densos (passo ≤ 0,5 px) de um caminho achado, com a largura interpolada.
fn dense(p: &InkPath) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::new();
    let n = p.points.len();
    let segs = if p.closed { n } else { n - 1 };
    for k in 0..segs {
        let (a, b) = (p.points[k], p.points[(k + 1) % n]);
        let (wa, wb) = (p.widths[k], p.widths[(k + 1) % n]);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        let m = (l / 0.5).ceil().max(1.0) as usize;
        for s in 0..m {
            let t = s as f32 / m as f32;
            out.push((a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, wa + (wb - wa) * t));
        }
    }
    let last = p.points[n - 1];
    out.push((last[0], last[1], p.widths[n - 1]));
    out
}

struct Score {
    paths: usize,
    /// distância da linha central verdadeira até a achada (média e 95%)
    pos_mean: f32,
    pos_p95: f32,
    /// o contrário: algo achado longe de qualquer traço (linha inventada)
    extra_p95: f32,
    /// erro de largura (média do erro relativo), longe de cruzamentos e das pontas
    width_rel: f32,
}

fn score(truth: &[Vec<StrokeSample>], found: &[InkPath]) -> Score {
    let fd: Vec<(f32, f32, f32)> = found.iter().flat_map(dense).collect();
    let all_t: Vec<(f32, f32, f32, usize)> = truth.iter().enumerate().flat_map(|(si, v)| v.iter().map(move |q| (q.x, q.y, q.r, si))).collect();
    let nearest = |x: f32, y: f32| fd.iter().map(|&(a, b, w)| ((a - x).hypot(b - y), w)).min_by(|a, b| a.0.total_cmp(&b.0)).unwrap_or((f32::MAX, 0.0));
    let mut pos = Vec::new();
    let mut wrel = Vec::new();
    for (si, v) in truth.iter().enumerate() {
        let n = v.len();
        for (k, q) in v.iter().enumerate() {
            let (d, wv) = nearest(q.x, q.y);
            pos.push(d);
            // largura: só longe das pontas (afinamento) e de outro traço
            let near_end = k < n / 10 || k > n - 1 - n / 10;
            let crowded = all_t.iter().any(|&(x, y, r, sj)| sj != si && (x - q.x).hypot(y - q.y) < q.r + r + 3.0);
            if !near_end && !crowded {
                wrel.push((wv - 2.0 * q.r).abs() / (2.0 * q.r));
            }
        }
    }
    let extra: Vec<f32> = fd.iter().map(|&(x, y, _)| all_t.iter().map(|&(a, b, _, _)| (a - x).hypot(b - y)).fold(f32::MAX, f32::min)).collect();
    let p95 = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.get(((v.len() as f32) * 0.95) as usize).copied().unwrap_or(0.0)
    };
    Score {
        paths: found.len(),
        pos_mean: pos.iter().sum::<f32>() / pos.len().max(1) as f32,
        pos_p95: p95(pos),
        extra_p95: p95(extra),
        width_rel: if wrel.is_empty() { 0.0 } else { wrel.iter().sum::<f32>() / wrel.len() as f32 },
    }
}

fn run(strokes: &[Stroke]) -> (Score, Vec<InkPath>) {
    let (ink, truth) = draw(strokes);
    let found = vectorize(W, H, &ink, &VectorizeOptions::default());
    (score(&truth, &found), found)
}

fn report(name: &str, s: &Score) {
    println!("{name}: traços {} · posição média {:.2} px, 95% {:.2} px · linha inventada 95% {:.2} px · largura {:.1}%", s.paths, s.pos_mean, s.pos_p95, s.extra_p95, s.width_rel * 100.0);
}

#[test]
fn straight_line_of_constant_width_comes_back_as_one_centred_stroke() {
    let (s, _) = run(&[stroke(&[(40.0, 150.0), (360.0, 150.0)], VectorTip::Marker, 4.0, false)]);
    report("reta 4 px", &s);
    assert_eq!(s.paths, 1);
    assert!(s.pos_mean < 0.4 && s.pos_p95 < 0.8, "posição");
    assert!(s.extra_p95 < 0.8, "nada inventado");
    assert!(s.width_rel < 0.08, "largura");
}

#[test]
fn widths_from_thin_to_thick_are_measured() {
    for w in [1.5f32, 3.0, 6.0, 12.0] {
        let (s, _) = run(&[stroke(&[(40.0, 80.0), (200.0, 140.0), (360.0, 90.0)], VectorTip::Marker, w, false)]);
        report(&format!("curva {w} px"), &s);
        assert_eq!(s.paths, 1, "{w}");
        assert!(s.pos_p95 < 1.0, "{w}: posição");
        assert!(s.width_rel < 0.12, "{w}: largura");
    }
}

#[test]
fn gpen_taper_is_kept_and_the_ends_are_reached() {
    let st = stroke(&[(50.0, 200.0), (150.0, 80.0), (300.0, 120.0), (350.0, 240.0)], VectorTip::GPen, 8.0, false);
    let (s, found) = run(&[st.clone()]);
    report("G-Pen 8 px", &s);
    assert_eq!(s.paths, 1);
    assert!(s.pos_p95 < 1.2, "posição");
    let p = &found[0];
    let (a, b) = (p.points[0], p.points[p.points.len() - 1]);
    println!("pontas achadas {a:?} {b:?}; larguras {:?}", p.widths);
    let ends = [(50.0f32, 200.0f32), (350.0, 240.0)];
    let gap = |q: [f32; 2]| ends.iter().map(|e| (e.0 - q[0]).hypot(e.1 - q[1])).fold(f32::MAX, f32::min);
    assert!(gap(a) < 2.5 && gap(b) < 2.5, "as pontas chegam perto das de verdade: {} {}", gap(a), gap(b));
    // afina: as pontas medidas bem mais finas que o meio
    let mid = p.widths[p.widths.len() / 2];
    assert!(p.widths[1] < mid * 0.75 && p.widths[p.widths.len() - 2] < mid * 0.75, "{:?}", p.widths);
}

#[test]
fn x_crossing_keeps_two_straight_strokes() {
    let a = stroke(&[(60.0, 60.0), (340.0, 240.0)], VectorTip::Marker, 4.0, false);
    let b = stroke(&[(60.0, 240.0), (340.0, 60.0)], VectorTip::Marker, 4.0, false);
    let (s, found) = run(&[a, b]);
    report("X", &s);
    assert_eq!(s.paths, 2, "dois traços, não quatro pedaços");
    assert!(s.pos_p95 < 1.5, "posição");
    for p in &found {
        // cada um atravessa de lado a lado
        assert!(p.length() > 300.0, "{}", p.length());
    }
}

#[test]
fn t_junction_gives_a_bar_and_a_stem() {
    let bar = stroke(&[(60.0, 100.0), (340.0, 100.0)], VectorTip::Marker, 4.0, false);
    let stem = stroke(&[(200.0, 100.0), (200.0, 260.0)], VectorTip::Marker, 4.0, false);
    let (s, found) = run(&[bar, stem]);
    report("T", &s);
    assert_eq!(s.paths, 2);
    assert!(found.iter().any(|p| p.length() > 270.0), "a barra inteira");
    assert!(s.pos_p95 < 1.5);
}

#[test]
fn closed_head_and_face_strokes() {
    let head = stroke(&ellipse(200.0, 150.0, 90.0, 110.0, 24), VectorTip::GPen, 5.0, true);
    let eye_l = stroke(&[(160.0, 130.0), (170.0, 120.0), (180.0, 130.0)], VectorTip::GPen, 3.0, false);
    let eye_r = stroke(&[(220.0, 130.0), (230.0, 120.0), (240.0, 130.0)], VectorTip::GPen, 3.0, false);
    let mouth = stroke(&[(180.0, 200.0), (200.0, 210.0), (220.0, 200.0)], VectorTip::GPen, 3.0, false);
    let (s, found) = run(&[head, eye_l, eye_r, mouth]);
    report("rosto", &s);
    assert_eq!(s.paths, 4);
    assert_eq!(found.iter().filter(|p| p.closed).count(), 1, "a cabeça fecha");
    assert!(s.pos_p95 < 1.2 && s.extra_p95 < 1.2);
}

#[test]
fn black_on_white_and_black_on_transparent_give_the_same_paths() {
    let st = stroke(&[(40.0, 80.0), (200.0, 140.0), (360.0, 90.0)], VectorTip::GPen, 6.0, false);
    let (ink, _) = draw(&[st]);
    let mut white = image::RgbaImage::new(W, H);
    let mut clear = image::RgbaImage::new(W, H);
    for (i, v) in ink.iter().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        let g = ((1.0 - v) * 255.0).round() as u8;
        white.put_pixel(x, y, image::Rgba([g, g, g, 255]));
        clear.put_pixel(x, y, image::Rgba([0, 0, 0, (v * 255.0).round() as u8]));
    }
    let o = VectorizeOptions::default();
    let a = vectorize(W, H, &aiai_tools::vectorize::ink_from_rgba(&white), &o);
    let b = vectorize(W, H, &aiai_tools::vectorize::ink_from_rgba(&clear), &o);
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0].points.len(), b[0].points.len());
    for (p, q) in a[0].points.iter().zip(&b[0].points) {
        assert!((p[0] - q[0]).abs() < 0.05 && (p[1] - q[1]).abs() < 0.05, "{p:?} {q:?}");
    }
}

#[test]
fn found_path_becomes_an_app_stroke_with_the_measured_widths() {
    let st = stroke(&[(50.0, 200.0), (150.0, 80.0), (300.0, 120.0), (350.0, 240.0)], VectorTip::GPen, 8.0, false);
    let (_, found) = run(&[st]);
    let s = path_to_stroke(&found[0], VectorTip::Marker, ColorRef::default(), 2.0, [10.0, 20.0]);
    assert_eq!(s.points.len(), found[0].points.len());
    assert!((s.points[0].x - (found[0].points[0][0] * 2.0 + 10.0)).abs() < 1e-4);
    // largura de volta: base × multiplicador = medida × escala
    for (q, wv) in s.points.iter().zip(&found[0].widths) {
        assert!((s.width * q.width - wv * 2.0).abs() < 1e-3);
    }
    // e a ponta não afina de novo por conta própria (G-Pen): o raio desenhado segue a largura medida também nas pontas
    let g = path_to_stroke(&found[0], VectorTip::GPen, ColorRef::default(), 1.0, [0.0, 0.0]);
    assert_eq!(g.taper, Some(aiai_core::StrokeTaper::NONE));
    let smp = sample_stroke(&g);
    let q = &found[0];
    let k = q.points.len() / 4; // um ponto perto da ponta, ainda dentro do trecho que a G-Pen afinaria
    let near = smp.iter().map(|s| ((s.x - q.points[k][0]).hypot(s.y - q.points[k][1]), s.r)).min_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
    assert!((2.0 * near.1 - q.widths[k]).abs() < 0.35 * q.widths[k], "raio {} × largura {}", near.1, q.widths[k]);
}

/// Bancada com desenhos de verdade (rodar à mão): `AIAI_VEC_BENCH=<project.json>` e opcional `AIAI_VEC_OUT=<pasta>`
/// para as comparações lado a lado. Só LÊ o arquivo: não abre o projeto nem trava nada.
/// `cargo test -p aiai-tools --test vectorize bench_real_cels -- --ignored --nocapture`
#[test]
#[ignore]
fn bench_real_cels() {
    let Some(path) = std::env::var_os("AIAI_VEC_BENCH") else { return };
    let out_dir = std::env::var_os("AIAI_VEC_OUT").map(std::path::PathBuf::from);
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let nodes: Vec<&serde_json::Value> = match &v["nodes"] {
        serde_json::Value::Object(m) => m.values().collect(),
        serde_json::Value::Array(a) => a.iter().collect(),
        _ => panic!("nodes"),
    };
    let mut rows = Vec::new();
    let mut saved = 0;
    for n in nodes {
        let Some(st) = n["data"]["strokes"].as_array() else { continue };
        let strokes: Vec<Stroke> = st.iter().filter_map(|s| serde_json::from_value::<Stroke>(s.clone()).ok()).filter(|s| !s.color_trace && s.points.len() >= 2).collect();
        if strokes.is_empty() {
            continue;
        }
        // janela: a caixa dos traços com margem, levada para a origem
        let smp: Vec<Vec<StrokeSample>> = strokes.iter().map(sample_stroke).collect();
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for q in smp.iter().flatten() {
            x0 = x0.min(q.x - q.r);
            y0 = y0.min(q.y - q.r);
            x1 = x1.max(q.x + q.r);
            y1 = y1.max(q.y + q.r);
        }
        let (ox, oy) = (x0 - 8.0, y0 - 8.0);
        let (w, h) = (((x1 - ox) + 8.0).ceil() as u32, ((y1 - oy) + 8.0).ceil() as u32);
        if w > 2400 || h > 2400 || w < 8 || h < 8 {
            continue;
        }
        let truth: Vec<Vec<StrokeSample>> = smp.iter().map(|v| v.iter().map(|q| StrokeSample { x: q.x - ox, y: q.y - oy, r: q.r }).collect()).collect();
        let mut ink = vec![0f32; (w * h) as usize];
        for t in &truth {
            if let Some(c) = rasterize_stroke(t, false, w, h) {
                for yy in 0..c.height {
                    for xx in 0..c.width {
                        let i = ((c.y0 + yy as i32) as u32 * w + (c.x0 + xx as i32) as u32) as usize;
                        ink[i] = ink[i].max(c.cov[(yy * c.width + xx) as usize]);
                    }
                }
            }
        }
        let t0 = std::time::Instant::now();
        let found = vectorize(w, h, &ink, &VectorizeOptions::default());
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        let s = score(&truth, &found);
        let name = n["name"].as_str().unwrap_or("?").to_string();
        if let (Some(probe), true) = (std::env::var_os("AIAI_VEC_PROBE"), saved == 0 && strokes.len() >= 20) {
            let v: Vec<f32> = probe.to_string_lossy().split(',').map(|x| x.parse().unwrap()).collect();
            for (i, p) in found.iter().enumerate() {
                if p.points.iter().any(|q| (q[0] - v[0]).hypot(q[1] - v[1]) < 8.0) {
                    println!("caminho {i}: {} pontos, comprimento {:.1}, fechado {}, pontos {:?} larguras {:?}", p.points.len(), p.length(), p.closed, &p.points[..p.points.len().min(8)], &p.widths[..p.widths.len().min(8)]);
                }
            }
        }
        rows.push((name.clone(), strokes.len(), s, ms, w, h));
        if let Some(dir) = &out_dir {
            if saved < 6 && strokes.len() >= 20 {
                saved += 1;
                // esquerda: o desenho; direita: os traços achados desenhados de novo (vermelho) sobre o desenho claro
                let mut img = image::RgbaImage::from_pixel(w * 2, h, image::Rgba([255, 255, 255, 255]));
                let mut redraw = vec![0f32; (w * h) as usize];
                for p in &found {
                    let st = path_to_stroke(p, VectorTip::Marker, ColorRef::default(), 1.0, [0.0, 0.0]);
                    if let Some(c) = rasterize_stroke(&sample_stroke(&st), false, w, h) {
                        for yy in 0..c.height {
                            for xx in 0..c.width {
                                let i = ((c.y0 + yy as i32) as u32 * w + (c.x0 + xx as i32) as u32) as usize;
                                redraw[i] = redraw[i].max(c.cov[(yy * c.width + xx) as usize]);
                            }
                        }
                    }
                }
                for y in 0..h {
                    for x in 0..w {
                        let a = ink[(y * w + x) as usize];
                        let g = (255.0 * (1.0 - a)) as u8;
                        img.put_pixel(x, y, image::Rgba([g, g, g, 255]));
                        let r = redraw[(y * w + x) as usize];
                        let base = 255.0 - 60.0 * a;
                        img.put_pixel(w + x, y, image::Rgba([(base * (1.0 - r) + 220.0 * r) as u8, (base * (1.0 - r) + 30.0 * r) as u8, (base * (1.0 - r) + 30.0 * r) as u8, 255]));
                    }
                }
                std::fs::create_dir_all(dir).unwrap();
                let safe: String = name.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                img.save(dir.join(format!("{saved:02}-{safe}.png"))).unwrap();
            }
        }
    }
    let n = rows.len() as f32;
    let mean = |f: &dyn Fn(&Score) -> f32| rows.iter().map(|r| f(&r.2)).sum::<f32>() / n;
    let mut worst: Vec<_> = rows.iter().map(|r| (r.2.pos_p95, r.0.clone(), r.1)).collect();
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("cels: {} · traços originais {} · caminhos achados {}", rows.len(), rows.iter().map(|r| r.1).sum::<usize>(), rows.iter().map(|r| r.2.paths).sum::<usize>());
    println!("posição média {:.2} px · 95% {:.2} px · linha inventada 95% {:.2} px · largura {:.1}%", mean(&|s| s.pos_mean), mean(&|s| s.pos_p95), mean(&|s| s.extra_p95), mean(&|s| s.width_rel) * 100.0);
    let mut times: Vec<f32> = rows.iter().map(|r| r.3).collect();
    times.sort_by(f32::total_cmp);
    println!("tempo por cel: mediana {:.0} ms, máx {:.0} ms (maior janela {:?})", times[times.len() / 2], times[times.len() - 1], rows.iter().max_by_key(|r| r.4 * r.5).map(|r| (r.4, r.5)));
    println!("piores (95% de posição): {:?}", &worst[..worst.len().min(5)]);
}

/// O comando: imagem de arquivo → traços no cel exposto da camada vetorial, um passo de desfazer; fonte vetorial e
/// camada de destino raster são recusadas.
#[test]
fn vectorize_command_puts_the_found_strokes_in_the_vector_cel() {
    use aiai_core::command::CellEdit;
    use aiai_core::*;
    let mut s = CoreSession::for_tests("vet");
    s.apply(Command::SetProjectProps { patch: serde_json::json!({"settings": {"width": W, "height": H}}) }).unwrap();
    let mk = |s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind| -> Id {
        let id = s.ids.next();
        s.apply(Command::CreateNode { id: Some(id.clone()), parent: parent.cloned(), index: None, name: name.into(), data }).unwrap();
        id
    };
    let ep = mk(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = mk(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = mk(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(4)));
    let vec_l = mk(&mut s, Some(&cut), "V", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    let ras_l = mk(&mut s, Some(&cut), "R", NodeKind::Layer(LayerData::new(LayerType::Raster)));
    mk(&mut s, Some(&vec_l), "V1", NodeKind::Cel(CelData::new(1)));
    mk(&mut s, Some(&ras_l), "R1", NodeKind::Cel(CelData::new(1)));
    for (t, l) in [("tv", &vec_l), ("tr", &ras_l)] {
        s.apply(Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(Id::from(t), t, TrackKind::Drawing, Some(l.clone())) }).unwrap();
        s.apply(Command::SetCells { cut: cut.clone(), track: Id::from(t), cells: vec![CellEdit { frame: 0, cell: Some(CellRef::Cel { number: 1 }) }] }).unwrap();
    }
    // a imagem: o rosto do teste acima, preto sobre branco
    let head = stroke(&ellipse(200.0, 150.0, 90.0, 110.0, 24), VectorTip::GPen, 5.0, true);
    let eye = stroke(&[(160.0, 130.0), (170.0, 120.0), (180.0, 130.0)], VectorTip::GPen, 3.0, false);
    let (ink, _) = draw(&[head, eye]);
    let mut img = image::RgbaImage::new(W, H);
    for (i, v) in ink.iter().enumerate() {
        let g = ((1.0 - v) * 255.0).round() as u8;
        img.put_pixel(i as u32 % W, i as u32 / W, image::Rgba([g, g, g, 255]));
    }
    let path = std::env::temp_dir().join(format!("aiai-vet-{}.png", std::process::id()));
    img.save(&path).unwrap();
    let req = |layer: &Id, source: serde_json::Value| -> aiai_tools::vectorize::VectorizeRequest {
        serde_json::from_value(serde_json::json!({"cut": cut, "frame": 0, "layer": layer, "source": source})).unwrap()
    };
    let src = serde_json::json!({"image": {"path": path.to_string_lossy(), "fit": "actual"}});
    let (cmd, report) = aiai_tools::vectorize::vectorize_command(&s.project, &s.blobs, &req(&vec_l, src.clone())).unwrap();
    assert_eq!(report["strokes"], 2, "{report}");
    assert_eq!(report["closed"], 1);
    let undo0 = s.history().0.len();
    s.apply(cmd).unwrap();
    assert_eq!(s.history().0.len(), undo0 + 1, "um passo de desfazer");
    let cel = s.project.cel_by_number(&vec_l, 1).unwrap();
    let NodeKind::Cel(cd) = &cel.data else { unreachable!() };
    assert_eq!(cd.strokes.len(), 2);
    // recusas
    let e = aiai_tools::vectorize::vectorize_command(&s.project, &s.blobs, &req(&ras_l, src)).unwrap_err().to_string();
    assert!(e.contains("vetorial"), "{e}");
    let e = aiai_tools::vectorize::vectorize_command(&s.project, &s.blobs, &req(&vec_l, serde_json::json!({"layer": {"layer": vec_l}}))).unwrap_err().to_string();
    assert!(e.contains("raster"), "{e}");
    let _ = std::fs::remove_file(path);
}
