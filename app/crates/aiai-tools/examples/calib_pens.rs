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
            Some(n) => factory(n),
            None => serde_json::from_value(job["preset"].clone()).expect("preset"),
        };
        // "patch": campos por cima do preset (ajuste rápido sem recompilar; objetos são mesclados, o resto trocado)
        if let Some(patch) = job.get("patch").filter(|p| p.is_object()) {
            let mut base = serde_json::to_value(&preset).expect("preset em JSON");
            merge(&mut base, patch);
            preset = serde_json::from_value(base).expect("preset com patch");
        }
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
            // traço = lista de pontos, ou {"pts": [...], "color": [r, g, b] (0..255), "preset_name": "...", "size": n}
            // (folhas de mistura: cor por traço; base dos misturadores: outro pincel antes)
            let (pts_v, color, stroke_preset) = match st.as_object() {
                Some(o) => {
                    let color = o.get("color").and_then(|c| c.as_array()).map(|c| {
                        let g = |i: usize| c.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32 / 255.0;
                        [g(0), g(1), g(2), 1.0]
                    });
                    let sp = o.get("preset_name").and_then(|n| n.as_str()).map(|n| {
                        let mut p = factory(n);
                        if let Some(sz) = o.get("size").and_then(|x| x.as_f64()) {
                            p.size = sz as f32;
                        }
                        p
                    });
                    (o.get("pts").expect("pts").clone(), color.unwrap_or([0.0, 0.0, 0.0, 1.0]), sp)
                }
                None => (st.clone(), [0.0, 0.0, 0.0, 1.0], None),
            };
            let preset = stroke_preset.as_ref().unwrap_or(&preset);
            let raw: Vec<[f32; 4]> = pts_v.as_array().unwrap().iter().map(|q| {
                let a = q.as_array().unwrap();
                let g = |i: usize, d: f32| a.get(i).and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(d);
                [g(0, 0.0), g(1, 0.0), g(2, 1.0), g(3, f32::NAN)]
            }).collect();
            let samples = simulate(preset, &raw, frame);
            let spec = PaintStrokeSpec { points: brush_points_from_samples(&samples), smooth: true };
            for (path, times) in brush_paths_timed(std::slice::from_ref(&spec)).expect("caminho") {
                let mut sb = StrokeBuffer::new(preset, color, seed.wrapping_add(k as u64 * 7919), w, h, [0, 0]);
                // a camada antes do traço: os pincéis que misturam pegam a cor dela (como no app)
                let layer = img.clone();
                match &times {
                    Some(ts) => sb.add_timed(Some(&layer), &path, ts),
                    None => sb.add(Some(&layer), &path),
                }
                sb.finish(Some(&layer));
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

/// Mescla `patch` em `base` (objetos campo a campo; `null` apaga o campo).
fn merge(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base.as_object_mut(), patch.as_object()) {
        (Some(b), Some(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    b.remove(k);
                } else if v.is_object() && b.get(k).is_some_and(|x| x.is_object()) {
                    merge(b.get_mut(k).unwrap(), v);
                } else {
                    b.insert(k.clone(), v.clone());
                }
            }
        }
        _ => *base = patch.clone(),
    }
}

/// Preset de fábrica pelo nome ("Borracha" = a ponta da ferramenta Borracha).
fn factory(n: &str) -> BrushPreset {
    match n {
        "Borracha" => BrushPreset::eraser(),
        _ => BrushPreset::defaults().into_iter().find(|p| p.name == n).unwrap_or_else(|| panic!("preset de fábrica {n:?} não existe")),
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
