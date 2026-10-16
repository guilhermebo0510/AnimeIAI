//! Suite unica de integracao do aiai-metrics (um binario so; ver licoes de implementacao, item 3).

use aiai_metrics::*;

// ------------------------------------------------------------------ imagens sinteticas
fn solid(w: u32, h: u32, c: [u8; 3]) -> Rgb8 {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for _ in 0..w * h {
        data.extend_from_slice(&c);
    }
    Rgb8 { width: w, height: h, data }
}

fn set(img: &mut Rgb8, x: i32, y: i32, c: [u8; 3]) {
    if x >= 0 && y >= 0 && (x as u32) < img.width && (y as u32) < img.height {
        let i = ((y as u32 * img.width + x as u32) * 3) as usize;
        img.data[i..i + 3].copy_from_slice(&c);
    }
}

fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Textura suave e deterministica (ruido pseudo-aleatorio borrado) com `dx,dy` de translacao do conteudo.
fn texture(w: u32, h: u32, dx: i32, dy: i32) -> Rgb8 {
    // gera ruido numa grade maior para que a translacao entre conteudo real (sem enrolar)
    let pad = 32i32;
    let (bw, bh) = (w as usize + 2 * pad as usize, h as usize + 2 * pad as usize);
    let noise: Vec<f32> = (0..bw * bh).map(|i| hash((i % bw) as i32, (i / bw) as i32) * 255.0).collect();
    let blurred = gaussian_blur(&noise, bw, bh, 2.5);
    let mut img = solid(w, h, [0, 0, 0]);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let sx = (x - dx + pad).clamp(0, bw as i32 - 1) as usize;
            let sy = (y - dy + pad).clamp(0, bh as i32 - 1) as usize;
            let v = blurred[sy * bw + sx];
            let v = ((v - 127.5) * 3.5 + 127.5).clamp(0.0, 255.0) as u8;
            set(&mut img, x, y, [v, v, v]);
        }
    }
    img
}

/// Cena "anime": fundo claro, area chapada colorida e contornos escuros.
fn scene(w: u32, h: u32, shift_x: i32) -> Rgb8 {
    let mut img = solid(w, h, [245, 240, 230]);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let (cx, cy) = (w as i32 / 2 + shift_x, h as i32 / 2);
            let d = (((x - cx).pow(2) + (y - cy).pow(2)) as f32).sqrt();
            if d < 45.0 {
                set(&mut img, x, y, [200, 90, 70]);
            }
            if (d - 45.0).abs() < 1.6 {
                set(&mut img, x, y, [40, 25, 25]);
            }
            if x > cx + 70 && x < cx + 120 && y > cy - 40 && y < cy + 40 {
                set(&mut img, x, y, [70, 120, 200]);
            }
        }
    }
    img
}

/// Anel (linha curva) de espessura `t` pixels, preto sobre branco.
fn ring(w: u32, h: u32, r_in: f32, t: f32) -> Rgb8 {
    let mut img = solid(w, h, [255, 255, 255]);
    let (cx, cy) = (w as f32 / 2.0 - 0.5, h as f32 / 2.0 - 0.5);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if d >= r_in && d < r_in + t {
                set(&mut img, x, y, [10, 10, 10]);
            }
        }
    }
    img
}

// ------------------------------------------------------------------ DeltaE2000 (Sharma, Wu, Dalal 2005)
#[rustfmt::skip]
const SHARMA: [([f64; 3], [f64; 3], f64); 34] = [
    ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
    ([50.0, 3.1571, -77.2803], [50.0, 0.0, -82.7485], 2.8615),
    ([50.0, 2.8361, -74.0200], [50.0, 0.0, -82.7485], 3.4412),
    ([50.0, -1.3802, -84.2814], [50.0, 0.0, -82.7485], 1.0000),
    ([50.0, -1.1848, -84.8006], [50.0, 0.0, -82.7485], 1.0000),
    ([50.0, -0.9009, -85.5211], [50.0, 0.0, -82.7485], 1.0000),
    ([50.0, 0.0, 0.0], [50.0, -1.0, 2.0], 2.3669),
    ([50.0, -1.0, 2.0], [50.0, 0.0, 0.0], 2.3669),
    ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0009], 7.1792),
    ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0010], 7.1792),
    ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0011], 7.2195),
    ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0012], 7.2195),
    ([50.0, -0.001, 2.49], [50.0, 0.0009, -2.49], 4.8045),
    ([50.0, -0.001, 2.49], [50.0, 0.0010, -2.49], 4.8045),
    ([50.0, -0.001, 2.49], [50.0, 0.0011, -2.49], 4.7461),
    ([50.0, 2.5, 0.0], [50.0, 0.0, -2.5], 4.3065),
    ([50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
    ([50.0, 2.5, 0.0], [61.0, -5.0, 29.0], 22.8977),
    ([50.0, 2.5, 0.0], [56.0, -27.0, -3.0], 31.9030),
    ([50.0, 2.5, 0.0], [58.0, 24.0, 15.0], 19.4535),
    ([50.0, 2.5, 0.0], [50.0, 3.1736, 0.5854], 1.0000),
    ([50.0, 2.5, 0.0], [50.0, 3.2972, 0.0], 1.0000),
    ([50.0, 2.5, 0.0], [50.0, 1.8634, 0.5757], 1.0000),
    ([50.0, 2.5, 0.0], [50.0, 3.2592, 0.3350], 1.0000),
    ([60.2574, -34.0099, 36.2677], [60.4626, -34.1751, 39.4387], 1.2644),
    ([63.0109, -31.0961, -5.8663], [62.8187, -29.7946, -4.0864], 1.2630),
    ([61.2901, 3.7196, -5.3901], [61.4292, 2.2480, -4.9620], 1.8731),
    ([35.0831, -44.1164, 3.7933], [35.0232, -40.0716, 1.5901], 1.8645),
    ([22.7233, 20.0904, -46.6940], [23.0331, 14.9730, -42.5619], 2.0373),
    ([36.4612, 47.8580, 18.3852], [36.2715, 50.5065, 21.2231], 1.4146),
    ([90.8027, -2.0831, 1.4410], [91.1528, -1.6435, 0.0447], 1.4441),
    ([90.9257, -0.5406, -0.9208], [88.6381, -0.8985, -0.7239], 1.5381),
    ([6.7747, -0.2908, -2.4247], [5.8714, -0.0985, -2.2286], 0.6377),
    ([2.0776, 0.0795, -1.1350], [0.9033, -0.0636, -0.5514], 0.9082),
];

#[test]
fn delta_e2000_sharma_table() {
    for (i, (a, b, want)) in SHARMA.iter().enumerate() {
        let got = delta_e2000(*a, *b);
        assert!((got - want).abs() < 1e-4, "par {}: obtido {got}, esperado {want}", i + 1);
        // simetria
        let rev = delta_e2000(*b, *a);
        assert!((rev - got).abs() < 1e-9, "par {} nao e simetrico", i + 1);
    }
}

#[test]
fn srgb_lab_known_values() {
    let white = srgb_to_lab([1.0, 1.0, 1.0]);
    assert!((white[0] - 100.0).abs() < 0.01 && white[1].abs() < 0.01 && white[2].abs() < 0.01, "{white:?}");
    let black = srgb_to_lab([0.0, 0.0, 0.0]);
    assert!(black[0].abs() < 1e-9);
    let red = srgb_to_lab([1.0, 0.0, 0.0]); // valores de referencia D65: 53.24, 80.09, 67.20
    assert!((red[0] - 53.24).abs() < 0.05 && (red[1] - 80.09).abs() < 0.1 && (red[2] - 67.20).abs() < 0.1, "{red:?}");
    // ida e volta
    let c = [0.31, 0.62, 0.87];
    let back = lab_to_srgb(srgb_to_lab(c));
    for k in 0..3 {
        assert!((back[k] - c[k]).abs() < 1e-6);
    }
}

#[test]
fn known_color_change_is_measured() {
    // cinza medio; teste = mesma cor deslocada em L* por +4 (dE2000 de duas cores iguais em a,b e dL = 4 e' ~ 4/SL)
    let base = [118u8, 142, 168];
    let lab = rgb8_to_lab(base);
    let shifted_lab = [lab[0] + 4.0, lab[1], lab[2]];
    let s = lab_to_srgb(shifted_lab);
    let test_rgb = [(s[0] * 255.0).round() as u8, (s[1] * 255.0).round() as u8, (s[2] * 255.0).round() as u8];
    let expected = delta_e2000(lab, rgb8_to_lab(test_rgb));
    assert!(expected > 3.0 && expected < 5.0, "sanidade do par: {expected}");
    let a = solid(32, 32, base);
    let b = solid(32, 32, test_rgb);
    let got = mean_delta_e(&a, &b, None).unwrap();
    assert!((got - expected).abs() < 1e-4, "{got} vs {expected}");
    // mascara
    let mut mask = vec![false; 32 * 32];
    mask[0] = true;
    let got_m = mean_delta_e(&a, &b, Some(&mask)).unwrap();
    assert!((got_m - expected).abs() < 1e-4);
    // um corte limpo: estimativa tipica de ΔE ~ 4
    assert!(mean_delta_e(&a, &a, None).unwrap() == 0.0);
}

// ------------------------------------------------------------------ identica => perfeito
#[test]
fn identical_images_are_perfect() {
    let a = scene(256, 160, 0);
    let b = a.clone();
    let f = flip(&a, &b).unwrap();
    assert_eq!(f.mean, 0.0);
    assert_eq!(f.max, 0.0);
    assert!((ssim(&a, &b).unwrap() - 1.0).abs() < 1e-9);
    assert_eq!(mean_delta_e(&a, &b, None).unwrap(), 0.0);
    let (ea, eb) = (edge_map(&a), edge_map(&b));
    assert_eq!(ea, eb);
    assert!(ea.iter().any(|v| *v));
    let s = edge_f1(&ea, &eb, 256, 160, 2.0);
    assert_eq!(s.f1, 1.0);
    let rep = compare(&a, &b, &CompareOptions::default()).unwrap();
    assert!(rep.verdict.pass, "{rep:?}");
    assert!(rep.camera.dx.abs() < 0.05 && rep.camera.dy.abs() < 0.05);
    assert!(rep.line_width.median_error.abs() < 1e-9);
    // relatorio serializavel
    let json = serde_json::to_string(&rep).unwrap();
    assert!(json.contains("\"flip\"") && json.contains("\"verdict\""));
}

#[test]
fn one_pixel_shift_is_flagged_and_ordered() {
    let a = scene(256, 160, 0);
    let s1 = scene(256, 160, 1);
    let s6 = scene(256, 160, 6);
    let (f1, f6) = (flip(&a, &s1).unwrap(), flip(&a, &s6).unwrap());
    assert!(f1.mean > 0.0, "1 px deve ser detectado");
    assert!(f6.mean > f1.mean, "deslocamento maior, erro maior: {} vs {}", f1.mean, f6.mean);
    let (q1, q6) = (ssim(&a, &s1).unwrap(), ssim(&a, &s6).unwrap());
    assert!(q1 < 1.0 && q6 < q1, "{q1} {q6}");
    // bordas: 1 px dentro da tolerancia de 2 px; 6 px fora
    let params = EdgeParams::auto(&a);
    let (ea, e1, e6) = (edge_map_with(&a, &params), edge_map_with(&s1, &params), edge_map_with(&s6, &params));
    let f_1 = edge_f1(&ea, &e1, 256, 160, 2.0).f1;
    let f_6 = edge_f1(&ea, &e6, 256, 160, 2.0).f1;
    assert!(f_1 > 0.95, "1 px com tolerancia 2: {f_1}");
    assert!(f_6 < 0.9 && f_6 < f_1, "6 px: {f_6}");
    // cor mudou: FLIP e dE detectam
    let mut tinted = a.clone();
    for p in tinted.data.chunks_exact_mut(3) {
        p[0] = p[0].saturating_add(18);
        p[2] = p[2].saturating_sub(18);
    }
    let ft = flip(&a, &tinted).unwrap();
    assert!(ft.mean > 0.02, "mudanca de cor detectada: {}", ft.mean);
    assert!(mean_delta_e(&a, &tinted, None).unwrap() > 3.0);
}

#[test]
fn flat_mask_excludes_edges_and_texture() {
    let a = scene(256, 160, 0);
    let m = flat_region_mask(&a);
    let w = 256usize;
    // centro do disco (chapado) esta na mascara; ponto sobre o contorno nao
    assert!(m[80 * w + 128]);
    assert!(!m[80 * w + 128 + 45]);
    // ruido forte nao e chapado
    let tex = texture(64, 64, 0, 0);
    let mt = flat_region_mask(&tex);
    let frac = mt.iter().filter(|b| **b).count() as f32 / mt.len() as f32;
    assert!(frac < 0.3, "textura quase toda fora da mascara: {frac}");
}

// ------------------------------------------------------------------ espessura de linha
#[test]
fn ten_percent_thicker_line_is_detected() {
    // Aneis de espessura 20 e 22 px (22/20 = +10%): a razao medida precisa acusar o aumento.
    let thin = ring(320, 320, 90.0, 20.0);
    let thick = ring(320, 320, 90.0, 22.0);
    let (a, b) = (line_width_stats(&thin), line_width_stats(&thick));
    assert!(a.samples > 100 && b.samples > 100);
    assert!((a.median - 20.0).abs() <= 2.0, "mediana ~20: {}", a.median);
    let ratio = b.median / a.median;
    assert!((1.05..=1.16).contains(&ratio), "razao de espessura {ratio} (a={}, b={})", a.median, b.median);
    // identica -> mesma mediana; p10 <= mediana <= p90
    let c = line_width_stats(&ring(320, 320, 90.0, 20.0));
    assert_eq!(c.median, a.median);
    assert!(a.p10 <= a.median && a.median <= a.p90);
    // linhas finas (10 vs 11 px) tambem sao distinguidas
    let (t10, t11) = (line_width_stats(&ring(320, 320, 90.0, 10.0)), line_width_stats(&ring(320, 320, 90.0, 11.0)));
    assert!(t11.median > t10.median, "{} vs {}", t10.median, t11.median);
    // o relatorio acusa fora de +-5%
    let rep = compare(&thin, &thick, &CompareOptions::default()).unwrap();
    assert!(!rep.verdict.line_width_ok, "erro {}", rep.line_width.median_error);
}

// ------------------------------------------------------------------ correlacao de fase
#[test]
fn phase_correlation_recovers_shift() {
    let a = texture(320, 240, 0, 0);
    let b = texture(320, 240, 7, -3);
    let (dx, dy, peak) = phase_correlation(&a, &b).unwrap();
    assert!((dx - 7.0).abs() < 0.3 && (dy + 3.0).abs() < 0.3, "obtido ({dx}, {dy}) pico {peak}");
    assert!(peak > 0.1, "{peak}");
    let (dx0, dy0, _) = phase_correlation(&a, &a).unwrap();
    assert!(dx0.abs() < 0.05 && dy0.abs() < 0.05);
    // sentido oposto
    let (rx, ry, _) = phase_correlation(&b, &a).unwrap();
    assert!((rx + 7.0).abs() < 0.3 && (ry - 3.0).abs() < 0.3, "({rx}, {ry})");
}

// ------------------------------------------------------------------ paleta, regioes, bordas
#[test]
fn palette_is_deterministic_and_matches() {
    let mut img = solid(120, 60, [250, 250, 250]);
    for y in 0..60 {
        for x in 0..120 {
            if x < 40 {
                set(&mut img, x, y, [200, 40, 40]);
            } else if x < 70 {
                set(&mut img, x, y, [40, 60, 200]);
            }
        }
    }
    let p1 = palette(&img, 3, None);
    let p2 = palette(&img, 3, None);
    assert_eq!(p1, p2, "deterministica");
    assert_eq!(p1.len(), 3);
    assert!((p1[0].1 - 50.0 / 120.0).abs() < 0.01, "{p1:?}"); // fundo: x em [70,120) = 50/120
    let sum: f32 = p1.iter().map(|c| c.1).sum();
    assert!((sum - 1.0).abs() < 1e-4);
    let m = match_palette(&p1, &p1);
    assert!(m.iter().all(|x| x.delta_e < 1e-6));
    // cor deslocada acusa ΔE > 0
    let mut img2 = img.clone();
    for p in img2.data.chunks_exact_mut(3) {
        if p[0] == 200 {
            p[0] = 215;
        }
    }
    let p3 = palette(&img2, 3, None);
    let m2 = match_palette(&p1, &p3);
    assert!(m2.iter().any(|x| x.delta_e > 2.0), "{m2:?}");
}

#[test]
fn palette_shift_measures_per_cluster_color_change() {
    let mut img = solid(120, 60, [250, 250, 250]);
    for y in 0..60 {
        for x in 0..120 {
            if x < 40 {
                set(&mut img, x, y, [200, 40, 40]);
            } else if x < 70 {
                set(&mut img, x, y, [40, 60, 200]);
            }
        }
    }
    let same = palette_shift(&img, &img, 3, None).unwrap();
    assert_eq!(same.len(), 3);
    assert!(same.iter().all(|m| m.delta_e < 1e-9));
    // so o vermelho muda: exatamente um grupo acusa, os outros ficam em 0
    let mut t = img.clone();
    for p in t.data.chunks_exact_mut(3) {
        if p[0] == 200 {
            p[0] = 215;
        }
    }
    let m = palette_shift(&img, &t, 3, None).unwrap();
    let moved: Vec<_> = m.iter().filter(|x| x.delta_e > 1.0).collect();
    assert_eq!(moved.len(), 1, "{m:?}");
    assert_eq!(moved[0].a, [200, 40, 40]);
    assert!(m.iter().filter(|x| x.delta_e < 1e-9).count() == 2);
}

#[test]
fn worst_regions_finds_hotspot_and_overlay_colors() {
    let (w, h) = (400u32, 300u32);
    let mut map = vec![0.01f32; (w * h) as usize];
    for y in 200..230 {
        for x in 300..330 {
            map[(y * w + x) as usize] = 0.9;
        }
    }
    let r = worst_regions(&map, w, h, 2, 64);
    assert_eq!(r.len(), 2);
    let [x, y, rw, rh] = r[0];
    assert!(x <= 300 && x + rw >= 330 && y <= 200 && y + rh >= 230, "{:?}", r[0]);
    // nao se sobrepoem
    assert!(r[0][0] + r[0][2] <= r[1][0] || r[1][0] + r[1][2] <= r[0][0] || r[0][1] + r[0][3] <= r[1][1] || r[1][1] + r[1][3] <= r[0][1]);

    // overlay: verde so ref, vermelho so teste, amarelo ambos
    let (ow, oh) = (40usize, 10usize);
    let mut er = vec![false; ow * oh];
    let mut et = vec![false; ow * oh];
    er[5 * ow + 3] = true; // so ref (longe do teste)
    et[5 * ow + 30] = true; // so teste
    er[2 * ow + 20] = true;
    et[2 * ow + 20] = true; // ambos
    let bg = solid(ow as u32, oh as u32, [255, 255, 255]);
    let img = edge_overlay(&bg, &er, &et, 2.0);
    let px = |x: usize, y: usize| img.get(x as u32, y as u32);
    assert!(px(3, 5)[1] > 0.7 && px(3, 5)[0] < 0.1, "verde {:?}", px(3, 5));
    assert!(px(30, 5)[0] > 0.8 && px(30, 5)[1] < 0.1, "vermelho {:?}", px(30, 5));
    assert!(px(20, 2)[0] > 0.9 && px(20, 2)[1] > 0.8 && px(20, 2)[2] < 0.1, "amarelo {:?}", px(20, 2));
    // heatmap FLIP
    let f = flip(&scene(64, 64, 0), &scene(64, 64, 5)).unwrap();
    let hm = diff_visualization(&f);
    assert_eq!((hm.width, hm.height), (64, 64));
}

#[test]
fn size_mismatch_is_an_error() {
    let a = solid(16, 16, [0, 0, 0]);
    let b = solid(16, 17, [0, 0, 0]);
    assert!(flip(&a, &b).is_err());
    assert!(ssim(&a, &b).is_err());
    assert!(compare(&a, &b, &CompareOptions::default()).is_err());
}

// ------------------------------------------------------------------ gancho manual (nao roda no `cargo test`)
/// Compara PNGs reais e grava relatorio + imagens. Uso:
/// `AIAI_METRICS_REF=ref.png AIAI_METRICS_TESTS=a.png;b.png AIAI_METRICS_OUT=pasta cargo test -p aiai-metrics --test suite -- --ignored harness`
#[test]
#[ignore]
fn harness_compare_files() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("defina {k}"));
    let reference = aiai_paint::Image::load_png(std::path::Path::new(&var("AIAI_METRICS_REF"))).unwrap();
    let out = std::path::PathBuf::from(var("AIAI_METRICS_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let mut all = serde_json::Map::new();
    for t in var("AIAI_METRICS_TESTS").split(';').filter(|s| !s.is_empty()) {
        let path = std::path::Path::new(t);
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let test = aiai_paint::Image::load_png(path).unwrap();
        let t0 = std::time::Instant::now();
        let (rep, art) = compare_with_artifacts(&reference, &test, &CompareOptions::default()).unwrap();
        eprintln!("{name}: {:.2}s", t0.elapsed().as_secs_f32());
        diff_visualization(&art.flip).save_png(&out.join(format!("{name}_flip.png"))).unwrap();
        edge_overlay(&reference, &art.ref_edges, &art.test_edges, 2.0).save_png(&out.join(format!("{name}_edges.png"))).unwrap();
        let v = serde_json::to_value(&rep).unwrap();
        std::fs::write(out.join(format!("{name}_report.json")), serde_json::to_string_pretty(&v).unwrap()).unwrap();
        all.insert(name, v);
    }
    std::fs::write(out.join("all_reports.json"), serde_json::to_string_pretty(&all).unwrap()).unwrap();
}
