//! Calibrador das canetas (S25): desenha a folha de teste com um preset, simulando o caminho da mão no app (amostras a
//! cada 8 ms entregues por quadro, estabilizador do pincel, os mesmos pontos do commit e a mesma spline), e grava o canvas
//! 900x700 em papel branco. O resultado é comparado com a referência do Clip Studio por `tools/csp/calib_compare.py`;
//! a conferência final é feita no app de verdade, pela caneta injetada da bridge.
//!
//! Uso: `cargo run --release -p aiai-tools --example calib_pens -- trabalhos.json`
//! `trabalhos.json`: `{"jobs": [{"preset": {...BrushPreset...}, "strokes": [[[x, y, pressão, t], ...], ...],
//! "out": "arquivo.png", "frame_ms": 16.667, "seed": 1}]}`.

use aiai_paint::Image;
use aiai_paint::brush::{BrushPreset, StrokeBuffer};
use aiai_paint::brush_dyn::Stabilizer;
use aiai_tools::paint::{PaintStrokeSpec, brush_paths_timed, brush_points_from_samples};

fn main() {
    let path = std::env::args().nth(1).expect("uso: calib_pens trabalhos.json");
    let text = std::fs::read_to_string(&path).expect("ler o arquivo de trabalhos");
    let v: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let jobs = v["jobs"].as_array().expect("jobs");
    // tabela do antisserrilhado em teste: [[w0, s0], [w1, s1], [w2, s2], [w3, s3], [menor semieixo, 0]]
    if let Some(t) = v.get("calib").and_then(|c| serde_json::from_value::<[[f32; 2]; 5]>(c.clone()).ok()) {
        aiai_paint::brush_dyn::calibration_override(t);
    }
    for job in jobs {
        // "preset_name" = um preset de fábrica pelo nome (com "size" por cima); senão o "preset" inteiro
        let mut preset: BrushPreset = match job["preset_name"].as_str() {
            Some("Borracha") => BrushPreset::eraser(),
            Some(n) => BrushPreset::defaults().into_iter().find(|p| p.name == n).unwrap_or_else(|| panic!("preset de fábrica {n:?} não existe")),
            None => serde_json::from_value(job["preset"].clone()).expect("preset"),
        };
        if let Some(sz) = job["size"].as_f64() {
            preset.size = sz as f32;
        }
        preset.validate().expect("preset válido");
        let frame = job["frame_ms"].as_f64().unwrap_or(16.667) as f32 / 1000.0;
        let seed = job["seed"].as_u64().unwrap_or(1);
        let (w, h) = (job["w"].as_u64().unwrap_or(900) as u32, job["h"].as_u64().unwrap_or(700) as u32);
        // camada do traço: vazia, ou preta cheia (folha das borrachas, `"base": "black"`); no fim, achatada no papel branco
        let mut img = if job["base"].as_str() == Some("black") { Image::filled(w, h, [0.0, 0.0, 0.0, 1.0]) } else { Image::new(w, h) };
        for (k, st) in job["strokes"].as_array().expect("strokes").iter().enumerate() {
            let raw: Vec<[f32; 4]> = st.as_array().unwrap().iter().map(|q| {
                let a = q.as_array().unwrap();
                let g = |i: usize, d: f32| a.get(i).and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(d);
                [g(0, 0.0), g(1, 0.0), g(2, 1.0), g(3, f32::NAN)]
            }).collect();
            let samples = simulate(&preset, &raw, frame);
            let spec = PaintStrokeSpec { points: brush_points_from_samples(&samples), smooth: true };
            for (path, times) in brush_paths_timed(std::slice::from_ref(&spec)).expect("caminho") {
                let mut sb = StrokeBuffer::new(&preset, [0.0, 0.0, 0.0, 1.0], seed.wrapping_add(k as u64 * 7919), w, h, [0, 0]);
                match &times {
                    Some(ts) => sb.add_timed(None, &path, ts),
                    None => sb.add(None, &path),
                }
                sb.finish(None);
                sb.composite(&mut img, [0, 0]);
            }
        }
        let mut paper = Image::filled(w, h, [1.0, 1.0, 1.0, 1.0]);
        for (d, s) in paper.data.iter_mut().zip(&img.data) {
            for c in 0..4 {
                d[c] = s[c] + d[c] * (1.0 - s[3]);
            }
        }
        let out = job["out"].as_str().expect("out");
        paper.save_png(std::path::Path::new(out)).expect("gravar png");
    }
}

/// Como `StrokeGesture::push` + `finish_stroke` do app com zoom 100%: tempo de cada amostra = quadro em que ela é
/// processada, estabilizador do pincel (se houver), descarte de amostras a menos de 0,4 px.
fn simulate(preset: &BrushPreset, raw: &[[f32; 4]], frame: f32) -> Vec<[f32; 4]> {
    let mut stab = preset.stabilization.filter(|s| s.level > 0).map(Stabilizer::new);
    let mut out: Vec<[f32; 4]> = Vec::new();
    let push = |out: &mut Vec<[f32; 4]>, q: [f32; 4]| {
        if out.last().is_none_or(|l| (l[0] - q[0]).hypot(l[1] - q[1]) >= 0.4) {
            out.push(q);
        }
    };
    for (i, q) in raw.iter().enumerate() {
        let t_in = if q[3].is_finite() { q[3] } else { i as f32 * 0.008 };
        let t = if frame > 0.0 { (t_in / frame).ceil() * frame } else { t_in };
        let (x, y) = match stab.as_mut() {
            Some(s) => s.feed(q[0], q[1], t),
            None => (q[0], q[1]),
        };
        push(&mut out, [x, y, q[2], t]);
    }
    if let Some(s) = stab.as_mut() {
        let pr = out.last().map(|l| l[2]).unwrap_or(1.0);
        for (x, y, t) in s.finish() {
            push(&mut out, [x, y, pr, t]);
        }
    }
    out
}
