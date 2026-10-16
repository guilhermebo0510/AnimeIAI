//! Suíte única de aiai-fx (S13 efeitos + S15 cor). Testes pequenos (rápidos mesmo no perfil de teste); a prova de
//! uso (folha de contato, tempos em 1080p, .cube, scopes) é gerada quando `FX_EVIDENCE=1` (ver `evidence`).

use aiai_core::{BlendMode, BlobHash, BlobStore, Ease, Key, KeyTracks};
use aiai_fx::lut::LutKind;
use aiai_fx::scopes;
use aiai_fx::{Effect, EffectInstance, FxContext, Lut, apply_stack, bake_lut, blur};
use aiai_paint::Image;
use aiai_paint::adjust::Adjustment;
use aiai_paint::noise::Rng;
use std::path::PathBuf;
use std::time::Instant;

// ───────────────────────────── utilidades ─────────────────────────────

fn ctx<'a>(blobs: &'a BlobStore, below: Option<&'a Image>, frame: u32) -> FxContext<'a> {
    FxContext { frame, below, blobs, seed: 0xA11CE }
}

fn run(e: &Effect, img: &Image, below: Option<&Image>, frame: u32) -> Image {
    let blobs = BlobStore::default();
    let mut out = img.clone();
    e.apply(&mut out, &ctx(&blobs, below, frame)).unwrap_or_else(|m| panic!("{}: {m}", e.name()));
    out
}

fn solid(w: u32, h: u32, c: [f32; 4]) -> Image {
    Image::filled(w, h, c)
}

fn px(img: &Image, x: u32, y: u32) -> [f32; 4] {
    img.get(x, y)
}

fn mass(img: &Image) -> [f64; 4] {
    let mut m = [0.0f64; 4];
    for p in &img.data {
        for k in 0..4 {
            m[k] += p[k] as f64;
        }
    }
    m
}

fn max_diff(a: &Image, b: &Image) -> f32 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    a.data.iter().zip(&b.data).map(|(p, q)| (0..4).map(|k| (p[k] - q[k]).abs()).fold(0.0, f32::max)).fold(0.0, f32::max)
}

fn assert_valid(img: &Image, what: &str) {
    for (i, p) in img.data.iter().enumerate() {
        for k in 0..4 {
            assert!(p[k].is_finite(), "{what}: valor não finito no pixel {i}");
        }
        assert!(p[3] >= -1e-4 && p[3] <= 1.0 + 1e-4, "{what}: alfa fora de 0..1 no pixel {i}: {}", p[3]);
        for k in 0..3 {
            assert!(p[k] >= -1e-4 && p[k] <= p[3] + 1e-3, "{what}: cor {} > alfa {} no pixel {i}", p[k], p[3]);
        }
    }
}

fn cov(sd: f32) -> f32 {
    (0.5 - sd).max(0.0).min(1.0)
}

fn sd_circle(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> f32 {
    ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r
}

fn sd_box(x: f32, y: f32, cx: f32, cy: f32, hx: f32, hy: f32, rad: f32) -> f32 {
    let qx = (x - cx).abs() - hx + rad;
    let qy = (y - cy).abs() - hy + rad;
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rad
}

/// Pinta cor reta com cobertura `c` sobre um pixel pré-multiplicado.
fn paint(dst: &mut [f32; 4], col: [f32; 3], c: f32) {
    for k in 0..3 {
        dst[k] = col[k] * c + dst[k] * (1.0 - c);
    }
    dst[3] = c + dst[3] * (1.0 - c);
}

fn disk(w: u32, h: u32, cx: f32, cy: f32, r: f32, col: [f32; 3]) -> Image {
    let mut img = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let c = cov(sd_circle(x as f32 + 0.5, y as f32 + 0.5, cx, cy, r));
            let i = img.idx(x, y);
            paint(&mut img.data[i], col, c);
        }
    }
    img
}

/// Cena de teste procedural: céu em degradê + chão escuro com pontos brilhantes (fundo), janela branca com
/// caixilhos (camada de luz) e silhueta cel de personagem com contorno escuro (camada de personagem).
struct Scene {
    bg: Image,
    window: Image,
    ch: Image,
}

fn make_scene(w: u32, h: u32) -> Scene {
    let (wf, hf) = (w as f32, h as f32);
    let mut bg = Image::new(w, h);
    let dots: [(f32, f32); 10] =
        [(0.06, 0.76), (0.14, 0.84), (0.2, 0.74), (0.5, 0.80), (0.58, 0.92), (0.66, 0.76), (0.74, 0.86), (0.85, 0.78), (0.93, 0.90), (0.45, 0.20)];
    for y in 0..h {
        for x in 0..w {
            let (px_, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let horizon = 0.70 * hf;
            let mut c = if py < horizon {
                let t = (py / horizon).powf(1.3);
                [0.12 + (0.80 - 0.12) * t, 0.30 + (0.90 - 0.30) * t, 0.70 + (1.0 - 0.70) * t]
            } else {
                let t = (py - horizon) / (hf - horizon);
                [0.07 - 0.03 * t, 0.10 - 0.04 * t, 0.16 - 0.06 * t]
            };
            for d in dots {
                let r = (0.0075 * hf).max(1.5);
                let k = cov(sd_circle(px_, py, d.0 * wf, d.1 * hf, r));
                if k > 0.0 {
                    for ch in 0..3 {
                        c[ch] = c[ch] * (1.0 - k) + [1.0, 0.95, 0.7][ch] * k;
                    }
                }
            }
            let i = bg.idx(x, y);
            bg.data[i] = [c[0], c[1], c[2], 1.0];
        }
    }
    let mut window = Image::new(w, h);
    let bar = (0.012 * hf).max(1.0);
    let (wcx, wcy, whx, why) = (0.77 * wf, 0.31 * hf, 0.15 * wf, 0.21 * hf);
    for y in 0..h {
        for x in 0..w {
            let (px_, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let rect = cov(sd_box(px_, py, wcx, wcy, whx, why, 0.0));
            let bars = cov(sd_box(px_, py, wcx, wcy, bar * 0.5, why * 2.0, 0.0)).max(cov(sd_box(px_, py, wcx, wcy, whx * 2.0, bar * 0.5, 0.0)));
            let c = rect * (1.0 - bars);
            let i = window.idx(x, y);
            paint(&mut window.data[i], [1.0, 1.0, 0.95], c);
        }
    }
    let mut ch = Image::new(w, h);
    let ow = (0.008 * hf).max(1.5);
    let (hx, hy, hr) = (0.33 * wf, 0.40 * hf, 0.12 * hf);
    for y in 0..h {
        for x in 0..w {
            let (px_, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let sd_head = sd_circle(px_, py, hx, hy, hr);
            let sd_body = sd_box(px_, py, hx, 0.73 * hf, 0.10 * hf, 0.22 * hf, 0.05 * hf);
            let sd_hair = sd_circle(px_, py, hx, hy - 0.03 * hf, hr * 1.12).max(py - 0.42 * hf);
            let sil = sd_head.min(sd_body).min(sd_hair);
            let i = ch.idx(x, y);
            let p = &mut ch.data[i];
            paint(p, [0.08, 0.05, 0.10], cov(sil - ow));
            paint(p, [0.80, 0.20, 0.25], cov(sd_body));
            paint(p, [0.98, 0.82, 0.72], cov(sd_head));
            paint(p, [0.25, 0.15, 0.42], cov(sd_hair));
            for ex in [0.30, 0.36] {
                paint(p, [0.1, 0.05, 0.12], cov(sd_circle(px_, py, ex * wf, 0.43 * hf, 0.012 * hf)));
            }
        }
    }
    Scene { bg, window, ch }
}

fn comp(bg: &Image, layers: &[&Image]) -> Image {
    let mut out = bg.clone();
    for l in layers {
        for (d, s) in out.data.iter_mut().zip(&l.data) {
            Image::over(d, *s, 1.0);
        }
    }
    out
}

fn add_noise(img: &Image, amp: f32) -> Image {
    let mut out = img.clone();
    let mut r = Rng::new(7);
    for p in &mut out.data {
        let n = r.signed() * amp * p[3];
        for k in 0..3 {
            p[k] = (p[k] + n).max(0.0).min(p[3]);
        }
    }
    out
}

// ───────────────────────────── catálogo (todos os efeitos) ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Target {
    Comp,
    CompNoisy,
    Window,
    Char,
}

#[derive(Clone)]
struct Item {
    label: String,
    effect: Effect,
    target: Target,
}

const ALL_NAMES: [&str; 25] = [
    "gaussian_blur", "directional_blur", "radial_blur", "color_aware_blur", "glow", "diffusion", "halation", "transmitted_light", "light_wrap",
    "rim_light", "para", "chromatic_aberration", "vignette", "grain", "deband", "lens_flare", "god_rays", "drop_shadow", "outline", "color_overlay",
    "invert", "two_tone", "adjust", "lut", "grade",
];

fn grade_sunset() -> Effect {
    Effect::Grade {
        lift: [0.03, 0.0, -0.02],
        gamma: [1.05, 1.0, 0.92],
        gain: [1.08, 0.97, 0.88],
        saturation: 1.15,
        temperature: 0.18,
        tint: 0.05,
        contrast: 1.08,
        pivot: 0.5,
    }
}

fn grade_night() -> Effect {
    Effect::Grade {
        lift: [-0.01, 0.0, 0.05],
        gamma: [0.95, 1.0, 1.1],
        gain: [0.72, 0.8, 1.0],
        saturation: 0.85,
        temperature: -0.2,
        tint: 0.0,
        contrast: 1.05,
        pivot: 0.4,
    }
}

fn grade_day() -> Effect {
    Effect::Grade {
        lift: [0.0; 3],
        gamma: [1.0, 1.0, 1.0],
        gain: [1.02, 1.02, 1.0],
        saturation: 1.1,
        temperature: 0.04,
        tint: 0.0,
        contrast: 1.06,
        pivot: 0.5,
    }
}

fn catalog(w: u32, h: u32, lut_blob: &BlobHash) -> Vec<Item> {
    let f = w as f32;
    let hh = h as f32;
    let it = |label: &str, effect: Effect, target: Target| Item { label: label.to_string(), effect, target };
    vec![
        it("gaussian_blur", Effect::GaussianBlur { radius: 0.025 * f }, Target::Comp),
        it("directional_blur", Effect::DirectionalBlur { angle: 20.0, length: 0.06 * f }, Target::Comp),
        it("radial_blur", Effect::RadialBlur { center: [0.5 * f, 0.5 * hh], amount: 0.15 }, Target::Comp),
        it("color_aware_blur (noisy)", Effect::ColorAwareBlur { radius: 0.015 * f, tolerance: 0.12 }, Target::CompNoisy),
        it("glow", Effect::Glow { threshold: 0.9, radius: 0.06 * f, intensity: 1.0, color: None, screen: false }, Target::Comp),
        it("diffusion", Effect::Diffusion { radius: 0.06 * f, amount: 0.7, threshold: 0.4 }, Target::Comp),
        it("halation", Effect::Halation { threshold: 0.8, radius: 0.07 * f, intensity: 1.4 }, Target::Comp),
        it("transmitted_light", Effect::TransmittedLight { radius: 0.08 * f, intensity: 1.2, color: [1.0, 0.85, 0.6] }, Target::Window),
        it("light_wrap", Effect::LightWrap { radius: 0.03 * f, amount: 0.8 }, Target::Char),
        it("rim_light", Effect::RimLight { angle: 315.0, width: 0.012 * f, color: [1.0, 0.9, 0.7], intensity: 1.0 }, Target::Char),
        it(
            "para (multiply)",
            Effect::Para { from: [0.33 * f, 0.25 * hh], to: [0.33 * f, 0.95 * hh], color_from: [1.0, 1.0, 1.0, 0.0], color_to: [0.1, 0.1, 0.45, 0.7], mode: BlendMode::Multiply },
            Target::Char,
        ),
        it("chromatic_aberration", Effect::ChromaticAberration { amount: 0.012 * f, center: None }, Target::Comp),
        it("vignette", Effect::Vignette { amount: 0.7, softness: 0.6, color: None }, Target::Comp),
        it("grain", Effect::Grain { amount: 0.06, size: 1.5 }, Target::Comp),
        it("deband", Effect::Deband { amount: 1.0 }, Target::Comp),
        it("lens_flare", Effect::LensFlare { position: [0.8 * f, 0.25 * hh], intensity: 1.0, color: [1.0, 0.9, 0.7] }, Target::Comp),
        it("god_rays", Effect::GodRays { center: [0.76 * f, 0.31 * hh], threshold: 0.8, length: 0.6, intensity: 0.8 }, Target::Comp),
        it("drop_shadow", Effect::DropShadow { offset: [0.015 * f, 0.02 * hh], blur: 0.01 * f, color: [0.0, 0.0, 0.0, 0.6] }, Target::Char),
        it("outline (white)", Effect::Outline { width: 0.006 * f, color: [1.0, 1.0, 1.0, 1.0] }, Target::Char),
        it("color_overlay", Effect::ColorOverlay { color: [1.0, 0.3, 0.1, 0.5], mode: BlendMode::Normal }, Target::Char),
        it("invert", Effect::Invert, Target::Comp),
        it("two_tone", Effect::TwoTone { threshold: 0.5, dark: [0.05, 0.05, 0.12], light: [1.0, 0.95, 0.85] }, Target::Comp),
        it("adjust (hue/sat)", Effect::Adjust { adjustment: Adjustment::HueSaturation { hue: 40.0, saturation: 0.2, lightness: 0.0 } }, Target::Comp),
        it("lut (sunset .cube)", Effect::Lut { blob: lut_blob.clone(), intensity: 1.0 }, Target::Comp),
        it("grade (teal/orange)", teal_orange(), Target::Comp),
    ]
}

fn teal_orange() -> Effect {
    Effect::Grade {
        lift: [-0.02, 0.01, 0.04],
        gamma: [1.0, 1.0, 1.0],
        gain: [1.12, 1.0, 0.9],
        saturation: 1.2,
        temperature: 0.1,
        tint: -0.05,
        contrast: 1.12,
        pivot: 0.5,
    }
}

fn setup(w: u32, h: u32) -> (Scene, BlobStore, BlobHash) {
    let mut blobs = BlobStore::default();
    let lut = bake_lut(&[grade_sunset()], 17, "AnimeIAI sunset", &blobs).unwrap();
    let hash = blobs.put(lut.to_cube().into_bytes());
    (make_scene(w, h), blobs, hash)
}

/// Aplica o item no alvo certo; devolve (imagem antes, imagem depois, resultado de tela para a folha de contato).
fn render_item(item: &Item, sc: &Scene, blobs: &BlobStore) -> (Image, Image, Image) {
    let c0 = comp(&sc.bg, &[&sc.window, &sc.ch]);
    let run_on = |target: &Image, below: &Image| {
        let mut out = target.clone();
        item.effect.apply(&mut out, &FxContext { frame: 3, below: Some(below), blobs, seed: 99 }).unwrap_or_else(|m| panic!("{}: {m}", item.label));
        out
    };
    match item.target {
        Target::Comp => {
            let out = run_on(&c0, &c0);
            (c0.clone(), out.clone(), out)
        }
        Target::CompNoisy => {
            let n = add_noise(&c0, 0.05);
            let out = run_on(&n, &n);
            (n, out.clone(), out)
        }
        Target::Window => {
            let out = run_on(&sc.window, &sc.bg);
            let tile = comp(&sc.bg, &[&out, &sc.ch]);
            (sc.window.clone(), out, tile)
        }
        Target::Char => {
            let below = comp(&sc.bg, &[&sc.window]);
            let out = run_on(&sc.ch, &below);
            let tile = comp(&below, &[&out]);
            (sc.ch.clone(), out, tile)
        }
    }
}

// ───────────────────────────── contrato geral ─────────────────────────────

#[test]
fn catalog_covers_every_variant_and_roundtrips_serde() {
    let (_sc, _blobs, hash) = setup(64, 36);
    let items = catalog(64, 36, &hash);
    let mut names: Vec<&str> = items.iter().map(|i| i.effect.name()).collect();
    names.sort();
    let mut expect = ALL_NAMES.to_vec();
    expect.sort();
    assert_eq!(names, expect, "o catálogo de teste deve cobrir os 25 efeitos");
    for it in &items {
        let v = serde_json::to_value(&it.effect).unwrap();
        let back: Effect = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(back, it.effect, "{}: ida e volta serde", it.label);
        assert_eq!(v["type"], it.effect.name());
    }
    // Glow antigo (sem `screen`) ainda carrega
    let g: Effect = serde_json::from_value(serde_json::json!({"type":"glow","threshold":0.5,"radius":4,"intensity":1})).unwrap();
    assert!(matches!(g, Effect::Glow { screen: false, color: None, .. }));
}

#[test]
fn every_effect_is_deterministic_valid_and_changes_the_image() {
    let (sc, blobs, hash) = setup(240, 135);
    for item in catalog(240, 135, &hash) {
        let (before, a, _) = render_item(&item, &sc, &blobs);
        let (_, b, _) = render_item(&item, &sc, &blobs);
        assert!(a == b, "{}: duas execuções devem ser idênticas (bits)", item.label);
        assert_valid(&a, &item.label);
        assert!(max_diff(&before, &a) > 0.0, "{}: não alterou a imagem", item.label);
    }
}

#[test]
fn results_do_not_depend_on_the_thread_count() {
    let (sc, blobs, hash) = setup(160, 90);
    let items = catalog(160, 90, &hash);
    let render_all = |threads: usize| -> Vec<Image> {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        pool.install(|| items.iter().map(|it| render_item(it, &sc, &blobs).1).collect())
    };
    let (a, b) = (render_all(1), render_all(5));
    for ((x, y), it) in a.iter().zip(&b).zip(&items) {
        assert!(x == y, "{}: resultado depende do número de threads", it.label);
    }
}

#[test]
fn absurd_parameters_stay_fast_and_valid() {
    let blobs = BlobStore::default();
    let img = disk(96, 54, 48.0, 27.0, 10.0, [1.0, 0.9, 0.8]);
    let below = solid(96, 54, [0.5, 0.5, 0.5, 1.0]);
    for e in [
        Effect::GaussianBlur { radius: 1.0e9 },
        Effect::Glow { threshold: 0.0, radius: 1.0e9, intensity: 50.0, color: None, screen: true },
        Effect::Diffusion { radius: 1.0e7, amount: 9.0, threshold: 0.0 },
        Effect::Halation { threshold: -3.0, radius: 1.0e7, intensity: 9.0 },
        Effect::DirectionalBlur { angle: 1.0e5, length: 1.0e9 },
        Effect::RadialBlur { center: [1.0e7, -1.0e7], amount: 5.0 },
        Effect::ColorAwareBlur { radius: 1.0e9, tolerance: 9.0 },
        Effect::TransmittedLight { radius: 1.0e8, intensity: 1.0e3, color: [9.0, 9.0, 9.0] },
        Effect::LightWrap { radius: 1.0e8, amount: 1.0e3 },
        Effect::RimLight { angle: 1.0e5, width: 1.0e8, color: [9.0; 3], intensity: 1.0e3 },
        Effect::Vignette { amount: 9.0, softness: -4.0, color: Some([5.0; 3]) },
        Effect::Grain { amount: 50.0, size: 1.0e9 },
        Effect::LensFlare { position: [1.0e8, 1.0e8], intensity: 1.0e4, color: [9.0; 3] },
        Effect::GodRays { center: [-1.0e8, 5.0], threshold: 2.0, length: 9.0, intensity: 9.0 },
        Effect::DropShadow { offset: [1.0e9, -1.0e9], blur: 1.0e9, color: [9.0, 9.0, 9.0, 9.0] },
        Effect::Outline { width: 1.0e9, color: [9.0; 4] },
        Effect::Grade { lift: [9.0; 3], gamma: [0.0; 3], gain: [-9.0; 3], saturation: 99.0, temperature: 99.0, tint: -99.0, contrast: 99.0, pivot: 9.0 },
    ] {
        let t = Instant::now();
        let mut out = img.clone();
        e.apply(&mut out, &FxContext { frame: 7, below: Some(&below), blobs: &blobs, seed: 1 }).unwrap_or_else(|m| panic!("{}: {m}", e.name()));
        assert_valid(&out, e.name());
        assert!(t.elapsed().as_secs_f32() < 5.0, "{} demorou {:?}", e.name(), t.elapsed());
    }
}

#[test]
fn every_effect_handles_degenerate_inputs() {
    let (_sc, blobs, hash) = setup(8, 8);
    let empty = Image::new(0, 0);
    let one = solid(1, 1, [0.5, 0.5, 0.5, 1.0]);
    let blank = Image::new(32, 18);
    for item in catalog(32, 18, &hash) {
        for img in [&empty, &one, &blank] {
            let mut out = img.clone();
            let below = img.clone();
            let r = item.effect.apply(&mut out, &FxContext { frame: 0, below: Some(&below), blobs: &blobs, seed: 1 });
            assert!(r.is_ok(), "{}: {:?}", item.label, r);
            assert_valid(&out, &item.label);
        }
    }
    // parâmetros não finitos viram erro, nunca NaN na imagem
    let mut img = solid(8, 8, [0.5, 0.5, 0.5, 1.0]);
    let r = Effect::GaussianBlur { radius: f32::NAN }.apply(&mut img, &ctx(&blobs, None, 0));
    assert!(r.is_err());
    assert_valid(&img, "nan");
    // light wrap sem `below` é erro explícito
    let r = Effect::LightWrap { radius: 4.0, amount: 1.0 }.apply(&mut img, &ctx(&blobs, None, 0));
    assert!(r.unwrap_err().contains("light_wrap"));
}

#[test]
fn stack_keys_and_posterize_time() {
    let blobs = BlobStore::default();
    let img = disk(120, 80, 60.0, 40.0, 14.0, [1.0, 1.0, 1.0]);
    let mut keys = KeyTracks::default();
    keys.0.insert(
        "radius".into(),
        vec![Key { frame: 0, value: 0.0, ease: Ease::Linear }, Key { frame: 10, value: 20.0, ease: Ease::Linear }],
    );
    let inst = EffectInstance { effect: Effect::GaussianBlur { radius: 0.0 }, enabled: true, posterize_time: 2, keys };
    let at = |frame| {
        let mut o = img.clone();
        let w = apply_stack(&mut o, std::slice::from_ref(&inst), &FxContext { frame, below: None, blobs: &blobs, seed: 0 });
        assert!(w.is_empty(), "{w:?}");
        o
    };
    // quadros 4 e 5 seguram o mesmo valor (posterize 2), 6 muda
    assert!(at(4) == at(5));
    assert!(at(4) != at(6));
    // quadro 0: raio 0 = imagem intacta
    assert!(at(0) == img);
    // efeito desligado não faz nada; erro vira aviso
    let off = EffectInstance { enabled: false, ..inst.clone() };
    let mut o = img.clone();
    assert!(apply_stack(&mut o, &[off], &FxContext { frame: 5, below: None, blobs: &blobs, seed: 0 }).is_empty());
    assert!(o == img);
    let lw = EffectInstance { effect: Effect::LightWrap { radius: 5.0, amount: 1.0 }, enabled: true, posterize_time: 1, keys: KeyTracks::default() };
    let warn = apply_stack(&mut o, &[lw], &FxContext { frame: 0, below: None, blobs: &blobs, seed: 0 });
    assert_eq!(warn.len(), 1);
}

// ───────────────────────────── desfoques ─────────────────────────────

#[test]
fn gaussian_blur_preserves_mass_and_matches_box_approximation() {
    let mut img = Image::new(400, 300);
    for y in 100..200 {
        for x in 150..250 {
            let i = img.idx(x, y);
            img.data[i] = [0.8, 0.3, 0.1, 1.0];
        }
    }
    let i = img.idx(200, 150);
    img.data[i] = [0.2, 0.1, 0.05, 0.25]; // pixel semi-transparente dentro do bloco
    let m0 = mass(&img);
    for radius in [3.0f32, 12.0, 30.0, 45.0, 90.0] {
        let b = run(&Effect::GaussianBlur { radius }, &img, None, 0);
        let m1 = mass(&b);
        for k in 0..4 {
            assert!((m1[k] - m0[k]).abs() / m0[k].max(1e-9) < 0.005, "raio {radius} canal {k}: {} vs {}", m1[k], m0[k]);
        }
        assert_valid(&b, "gaussian");
    }
    // o caminho de caixas (σ > 10) concorda com a gaussiana exata em σ = 10 (erro pequeno no perfil de borda)
    let step = {
        let mut s = Image::new(200, 20);
        for y in 0..20 {
            for x in 100..200 {
                let i = s.idx(x, y);
                s.data[i] = [1.0, 1.0, 1.0, 1.0];
            }
        }
        s
    };
    let exact = blur::gaussian(&step, 10.0);
    let boxed = blur::gaussian(&step, 10.01);
    let mut worst = 0.0f32;
    for x in 40..160 {
        worst = worst.max((px(&exact, x, 10)[0] - px(&boxed, x, 10)[0]).abs());
    }
    assert!(worst < 0.02, "caixa×3 vs gaussiana exata: erro máximo {worst}");
    // sem franja escura: borrar branco opaco sobre transparente mantém cor/alfa = 1 onde alfa > 0
    let b = run(&Effect::GaussianBlur { radius: 9.0 }, &disk(100, 100, 50.0, 50.0, 15.0, [1.0, 1.0, 1.0]), None, 0);
    for p in &b.data {
        if p[3] > 0.01 {
            assert!((p[0] / p[3] - 1.0).abs() < 1e-3, "franja: cor reta {}", p[0] / p[3]);
        }
    }
}

#[test]
fn directional_blur_follows_the_angle_and_keeps_mass() {
    let mut img = solid(200, 100, [0.0, 0.0, 0.0, 1.0]);
    for y in 0..100 {
        for x in 98..102 {
            let i = img.idx(x, y);
            img.data[i] = [1.0, 1.0, 1.0, 1.0];
        }
    }
    let h = run(&Effect::DirectionalBlur { angle: 0.0, length: 21.0 }, &img, None, 0);
    // espalha na horizontal: platô de ~4/22 em ±10 px do centro, nada além; linhas idênticas
    assert!(px(&h, 100, 50)[0] > 0.12 && px(&h, 100, 50)[0] < 0.24);
    assert!(px(&h, 125, 50)[0] < 1e-4);
    assert!((px(&h, 100, 20)[0] - px(&h, 100, 80)[0]).abs() < 1e-5);
    let (m0, m1) = (mass(&img)[0], mass(&h)[0]);
    assert!((m1 - m0).abs() / m0 < 0.005);
    // ao longo da barra (90°) a barra vertical não muda no interior
    let v = run(&Effect::DirectionalBlur { angle: 90.0, length: 21.0 }, &img, None, 0);
    for y in 15..85 {
        assert!((px(&v, 99, y)[0] - 1.0).abs() < 1e-4 && px(&v, 110, y)[0] < 1e-4);
    }
    // comprimento 0 não faz nada
    assert!(run(&Effect::DirectionalBlur { angle: 33.0, length: 0.0 }, &img, None, 0) == img);
}

#[test]
fn radial_blur_streaks_along_the_ray() {
    let mut img = solid(200, 200, [0.0, 0.0, 0.0, 1.0]);
    for y in 98..102 {
        for x in 148..152 {
            let i = img.idx(x, y);
            img.data[i] = [1.0, 1.0, 1.0, 1.0];
        }
    }
    let r = run(&Effect::RadialBlur { center: [100.0, 100.0], amount: 0.3 }, &img, None, 0);
    // o ponto em (150,100) vira traço radial: cada pixel média amostras mais perto do centro, então o traço
    // se estende para FORA (x maior) e não se espalha na vertical
    assert!(px(&r, 160, 100)[0] > 0.05, "traço radial ausente: {}", px(&r, 160, 100)[0]);
    assert!(px(&r, 140, 100)[0] < 1e-4, "não deve borrar para dentro");
    assert!(px(&r, 150, 112)[0] < 1e-3, "espalhou na tangente");
    // o centro não muda
    let c = solid(200, 200, [0.3, 0.6, 0.9, 1.0]);
    let rc = run(&Effect::RadialBlur { center: [100.0, 100.0], amount: 0.5 }, &c, None, 0);
    assert!(max_diff(&c, &rc) < 1e-5);
}

#[test]
fn color_aware_blur_keeps_flat_cels_apart() {
    let (w, h) = (100u32, 60u32);
    let mut cel = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = cel.idx(x, y);
            cel.data[i] = if x < 50 { [0.9, 0.1, 0.1, 1.0] } else { [0.1, 0.1, 0.9, 1.0] };
        }
    }
    let cab = run(&Effect::ColorAwareBlur { radius: 8.0, tolerance: 0.1 }, &cel, None, 0);
    assert!(max_diff(&cel, &cab) <= 1.0 / 255.0, "cel plano mudou {}", max_diff(&cel, &cab));
    // controle: a gaussiana comum vaza ≫ 1/255 junto da fronteira
    let g = run(&Effect::GaussianBlur { radius: 8.0 }, &cel, None, 0);
    assert!(px(&g, 49, 30)[0] < 0.8, "controle: a gaussiana deveria misturar");
    // ruído dentro de cada região é suavizado sem vazar entre regiões
    let mut noisy = cel.clone();
    let mut r = Rng::new(3);
    for p in &mut noisy.data {
        let n = r.signed() * 0.03;
        for k in 0..3 {
            p[k] = (p[k] + n).max(0.0).min(1.0);
        }
    }
    let var = |img: &Image| {
        let (mut s, mut s2, mut n) = (0.0f64, 0.0f64, 0.0f64);
        for y in 5..55 {
            for x in 5..45 {
                let v = px(img, x, y)[0] as f64;
                s += v;
                s2 += v * v;
                n += 1.0;
            }
        }
        s2 / n - (s / n) * (s / n)
    };
    let sm = run(&Effect::ColorAwareBlur { radius: 6.0, tolerance: 0.2 }, &noisy, None, 0);
    assert!(var(&sm) < var(&noisy) * 0.35, "ruído não reduziu: {} → {}", var(&noisy), var(&sm));
    for y in 5..55 {
        assert!((px(&sm, 49, y)[0] - 0.9).abs() < 0.05, "vazou a cor da direita: {}", px(&sm, 49, y)[0]);
        assert!((px(&sm, 50, y)[2] - 0.9).abs() < 0.05);
    }
    // tolerância 0 = imagem intacta
    assert!(max_diff(&noisy, &run(&Effect::ColorAwareBlur { radius: 6.0, tolerance: 0.0 }, &noisy, None, 0)) < 1e-6);
}

// ───────────────────────────── luz ─────────────────────────────

fn night_with_dot() -> Image {
    let mut img = solid(160, 90, [0.05, 0.05, 0.05, 1.0]);
    let d = disk(160, 90, 80.0, 45.0, 6.0, [1.0, 1.0, 1.0]);
    for (p, q) in img.data.iter_mut().zip(&d.data) {
        Image::over(p, *q, 1.0);
    }
    img
}

#[test]
fn glow_brightens_near_highlights_and_leaves_darks() {
    let img = night_with_dot();
    for screen in [false, true] {
        let g = run(&Effect::Glow { threshold: 0.7, radius: 30.0, intensity: 1.5, color: None, screen }, &img, None, 0);
        assert!(px(&g, 90, 45)[0] > px(&img, 90, 45)[0] + 0.01, "perto do realce deve clarear");
        assert!(px(&g, 80, 45)[0] >= px(&img, 80, 45)[0] - 1e-6, "o realce não escurece");
        assert!((px(&g, 2, 2)[0] - px(&img, 2, 2)[0]).abs() < 1.0 / 255.0, "longe do realce não muda");
        // cai com a distância
        assert!(px(&g, 88, 45)[0] > px(&g, 100, 45)[0] && px(&g, 100, 45)[0] > px(&g, 120, 45)[0]);
        assert_valid(&g, "glow");
    }
    // colorido: o brilho segue a cor pedida
    let c = run(&Effect::Glow { threshold: 0.7, radius: 30.0, intensity: 2.0, color: Some([1.0, 0.1, 0.1]), screen: false }, &img, None, 0);
    let p = px(&c, 92, 45);
    assert!(p[0] - 0.05 > 3.0 * (p[2] - 0.05).max(1e-4), "glow vermelho: {p:?}");
    // sem nada acima do limiar, não muda
    let dark = solid(64, 36, [0.3, 0.3, 0.3, 1.0]);
    assert!(max_diff(&dark, &run(&Effect::Glow { threshold: 0.9, radius: 20.0, intensity: 3.0, color: None, screen: false }, &dark, None, 0)) < 1e-6);
}

#[test]
fn diffusion_softens_highlights_only_above_threshold() {
    let img = night_with_dot();
    assert!(run(&Effect::Diffusion { radius: 20.0, amount: 0.0, threshold: 0.0 }, &img, None, 0) == img);
    let d = run(&Effect::Diffusion { radius: 20.0, amount: 0.7, threshold: 0.1 }, &img, None, 0);
    assert!(px(&d, 88, 45)[0] > px(&img, 88, 45)[0] + 0.02, "{:?}", px(&d, 88, 45));
    assert!((px(&d, 5, 5)[0] - px(&img, 5, 5)[0]).abs() < 1e-3, "área escura não muda com limiar");
    // sem limiar, a difusão age em tudo (mais clara que o original em cinza médio)
    let gray = solid(64, 36, [0.4, 0.4, 0.4, 1.0]);
    let dg = run(&Effect::Diffusion { radius: 10.0, amount: 0.5, threshold: 0.0 }, &gray, None, 0);
    assert!(px(&dg, 32, 18)[0] > 0.4 + 0.05);
    assert_valid(&dg, "diffusion");
}

#[test]
fn halation_adds_a_red_orange_halo() {
    let img = night_with_dot();
    let h = run(&Effect::Halation { threshold: 0.7, radius: 40.0, intensity: 1.5 }, &img, None, 0);
    let p = px(&h, 80 + 12, 45);
    let (dr, db) = (p[0] - 0.05, p[2] - 0.05);
    assert!(dr > 0.01, "halo ausente");
    assert!(dr > db * 2.5, "halo deve ser avermelhado: dr {dr} db {db}");
    assert!((px(&h, 2, 2)[0] - 0.05).abs() < 1.0 / 255.0);
}

#[test]
fn transmitted_light_uses_the_alpha_shape_as_emitter() {
    let mut win = Image::new(160, 90);
    for y in 25..65 {
        for x in 50..110 {
            let i = win.idx(x, y);
            win.data[i] = [1.0, 1.0, 1.0, 1.0];
        }
    }
    let t = run(&Effect::TransmittedLight { radius: 30.0, intensity: 1.0, color: [1.0, 0.6, 0.2] }, &win, None, 0);
    assert_valid(&t, "transmitted");
    // vaza luz para fora da forma, tingida e decrescente
    let a: Vec<f32> = [115u32, 120, 130, 140].iter().map(|&x| px(&t, x, 45)[3]).collect();
    assert!(a[0] > 0.05, "sem glow fora: {a:?}");
    assert!(a.windows(2).all(|w| w[0] > w[1]), "alfa deveria decrescer: {a:?}");
    let p = px(&t, 118, 45);
    assert!(p[0] > p[1] && p[1] > p[2], "tinta laranja esperada: {p:?}");
    assert!(px(&t, 155, 2)[3] < 1e-3, "longe da forma não deve haver luz");
    // dentro da forma continua branco
    assert!(px(&t, 80, 45)[0] > 0.99);
    // sem alfa não há emissor
    let blank = Image::new(40, 20);
    assert!(run(&Effect::TransmittedLight { radius: 10.0, intensity: 2.0, color: [1.0; 3] }, &blank, None, 0) == blank);
}

#[test]
fn light_wrap_tints_the_inner_edge_with_the_background() {
    let layer = disk(160, 90, 80.0, 45.0, 25.0, [0.4, 0.4, 0.4]);
    let below = solid(160, 90, [0.9, 0.1, 0.1, 1.0]);
    let w = run(&Effect::LightWrap { radius: 16.0, amount: 1.0 }, &layer, Some(&below), 0);
    assert_valid(&w, "light_wrap");
    let edge = (px(&w, 80 + 22, 45), px(&layer, 80 + 22, 45));
    assert!(edge.0[0] - edge.1[0] > 0.1, "a borda interna deve receber o vermelho do fundo");
    assert!(edge.0[0] - edge.1[0] > 3.0 * (edge.0[1] - edge.1[1]).max(1e-4));
    assert!(max_diff(&Image { width: 1, height: 1, data: vec![px(&w, 80, 45)] }, &Image { width: 1, height: 1, data: vec![px(&layer, 80, 45)] }) < 1e-3, "o centro não muda");
    // fora do alfa continua vazio
    assert_eq!(px(&w, 5, 5), [0.0; 4]);
    for (p, q) in w.data.iter().zip(&layer.data) {
        assert!((p[3] - q[3]).abs() < 1e-6, "light wrap não muda o alfa");
    }
    // tamanho de fundo diferente = erro
    let bad = solid(10, 10, [1.0; 4]);
    let blobs = BlobStore::default();
    let mut l2 = layer.clone();
    assert!(Effect::LightWrap { radius: 4.0, amount: 1.0 }.apply(&mut l2, &ctx(&blobs, Some(&bad), 0)).is_err());
}

#[test]
fn rim_light_lights_the_edge_facing_the_light() {
    let layer = disk(160, 90, 80.0, 45.0, 25.0, [0.3, 0.3, 0.3]);
    let r = run(&Effect::RimLight { angle: 0.0, width: 8.0, color: [1.0, 0.9, 0.5], intensity: 1.0 }, &layer, None, 0);
    assert_valid(&r, "rim");
    let facing = px(&r, 80 + 23, 45);
    let away = px(&r, 80 - 23, 45);
    assert!(facing[0] - 0.3 > 0.2, "borda voltada à luz sem brilho: {facing:?}");
    assert!((away[0] - px(&layer, 80 - 23, 45)[0]).abs() < 0.02, "borda oposta não deve acender: {away:?}");
    assert!(facing[0] > facing[2], "cor do rim");
    for (p, q) in r.data.iter().zip(&layer.data) {
        assert!((p[3] - q[3]).abs() < 1e-6, "rim não muda o alfa");
    }
    // a direção gira o rim: luz por cima (270°: y para cima) acende o topo, não a base
    let up = run(&Effect::RimLight { angle: 270.0, width: 8.0, color: [1.0; 3], intensity: 1.0 }, &layer, None, 0);
    assert!(px(&up, 80, 45 - 23)[0] - 0.3 > 0.2 && (px(&up, 80, 45 + 23)[0] - 0.3).abs() < 0.02);
}

#[test]
fn lens_flare_and_god_rays() {
    let dark = solid(200, 120, [0.02, 0.02, 0.02, 1.0]);
    let f = run(&Effect::LensFlare { position: [150.0, 30.0], intensity: 1.0, color: [1.0, 0.9, 0.7] }, &dark, None, 0);
    assert_valid(&f, "flare");
    assert!(px(&f, 150, 30)[0] > 0.8, "núcleo fraco");
    // fantasma em c + v·(−0,35)
    let (cx, cy) = (100.0f32, 60.0f32);
    let (gx, gy) = (cx + (150.0 - cx) * -0.35, cy + (30.0 - cy) * -0.35);
    assert!(px(&f, gx as u32, gy as u32)[0] > 0.02 + 0.05, "fantasma ausente");
    assert!(run(&Effect::LensFlare { position: [150.0, 30.0], intensity: 0.0, color: [1.0; 3] }, &dark, None, 0) == dark);

    let mut img = solid(200, 200, [0.02, 0.02, 0.02, 1.0]);
    let d = disk(200, 200, 100.0, 100.0, 8.0, [1.0, 1.0, 1.0]);
    for (p, q) in img.data.iter_mut().zip(&d.data) {
        Image::over(p, *q, 1.0);
    }
    let g = run(&Effect::GodRays { center: [100.0, 100.0], threshold: 0.8, length: 1.0, intensity: 1.0 }, &img, None, 0);
    assert_valid(&g, "god rays");
    let (a, b) = (px(&g, 150, 100)[0], px(&g, 50, 100)[0]);
    assert!(a > 0.05, "raio ausente: {a}");
    assert!((a - b).abs() < 0.04, "raios simétricos: {a} vs {b}");
    assert!(px(&g, 180, 100)[0] > 0.02 + 0.003, "{}", px(&g, 180, 100)[0]);
    // sem realce acima do limiar, nada
    let flat = solid(64, 64, [0.4, 0.4, 0.4, 1.0]);
    assert!(max_diff(&flat, &run(&Effect::GodRays { center: [32.0, 32.0], threshold: 0.9, length: 0.5, intensity: 2.0 }, &flat, None, 0)) < 1e-6);
}

// ───────────────────────────── estilo ─────────────────────────────

#[test]
fn para_gradient_is_clipped_to_alpha() {
    let mut layer = solid(80, 90, [0.5, 0.5, 0.5, 1.0]);
    for y in 0..90 {
        for x in 60..80 {
            let i = layer.idx(x, y);
            layer.data[i] = [0.0; 4];
        }
    }
    let e = Effect::Para { from: [0.0, 0.0], to: [0.0, 89.0], color_from: [1.0, 0.0, 0.0, 1.0], color_to: [0.0, 0.0, 1.0, 1.0], mode: BlendMode::Normal };
    let p = run(&e, &layer, None, 0);
    assert!(px(&p, 10, 0)[0] > 0.95 && px(&p, 10, 0)[2] < 0.05);
    assert!(px(&p, 10, 89)[2] > 0.95 && px(&p, 10, 89)[0] < 0.05);
    assert_eq!(px(&p, 70, 45), [0.0; 4], "fora do alfa nada é pintado");
    for (a, b) in p.data.iter().zip(&layer.data) {
        assert_eq!(a[3], b[3]);
    }
    // multiplicar com cor sem alfa não muda
    let m = Effect::Para { from: [0.0, 0.0], to: [0.0, 89.0], color_from: [0.2, 0.2, 0.2, 0.0], color_to: [0.2, 0.2, 0.2, 0.0], mode: BlendMode::Multiply };
    assert!(max_diff(&layer, &run(&m, &layer, None, 0)) < 1e-6);
    let mm = Effect::Para { from: [0.0, 0.0], to: [0.0, 89.0], color_from: [0.5, 0.5, 0.5, 1.0], color_to: [0.5, 0.5, 0.5, 1.0], mode: BlendMode::Multiply };
    assert!((px(&run(&mm, &layer, None, 0), 10, 40)[0] - 0.25).abs() < 1e-5);
}

#[test]
fn chromatic_aberration_splits_channels_radially() {
    let mut img = solid(200, 100, [0.0, 0.0, 0.0, 1.0]);
    for y in 0..100 {
        for x in 149..151 {
            let i = img.idx(x, y);
            img.data[i] = [1.0, 1.0, 1.0, 1.0];
        }
    }
    let c = run(&Effect::ChromaticAberration { amount: 8.0, center: None }, &img, None, 0);
    let argmax = |ch: usize| (0..200u32).max_by(|&a, &b| px(&c, a, 50)[ch].partial_cmp(&px(&c, b, 50)[ch]).unwrap()).unwrap();
    let (xr, xg, xb) = (argmax(0), argmax(1), argmax(2));
    assert!(xr < xg && xg < xb, "R deve ficar mais perto do centro e B mais longe: r{xr} g{xg} b{xb}");
    // o centro não muda e o canto desloca ≈ `amount`
    let flat = solid(200, 100, [0.3, 0.6, 0.9, 1.0]);
    assert!(max_diff(&flat, &run(&Effect::ChromaticAberration { amount: 8.0, center: None }, &flat, None, 0)) < 1e-5);
    assert_valid(&c, "ca");
}

#[test]
fn vignette_darkens_corners_only() {
    let img = solid(160, 90, [0.8, 0.8, 0.8, 1.0]);
    let v = run(&Effect::Vignette { amount: 0.8, softness: 0.6, color: None }, &img, None, 0);
    assert!(px(&v, 1, 1)[0] < 0.8 * 0.4);
    assert!((px(&v, 80, 45)[0] - 0.8).abs() < 1e-6);
    assert!(px(&v, 1, 1)[0] < px(&v, 30, 20)[0]);
    assert!(run(&Effect::Vignette { amount: 0.0, softness: 0.5, color: None }, &img, None, 0) == img);
    // com cor: o canto tende à cor (multiplicada)
    let c = run(&Effect::Vignette { amount: 1.0, softness: 0.3, color: Some([0.5, 0.0, 0.0]) }, &img, None, 0);
    assert!(px(&c, 0, 0)[0] > 0.3 && px(&c, 0, 0)[1] < 0.02);
    // alfa intacto
    let t = solid(160, 90, [0.8, 0.8, 0.8, 0.5]);
    let vt = run(&Effect::Vignette { amount: 1.0, softness: 0.5, color: None }, &t, None, 0);
    assert!(vt.data.iter().all(|p| (p[3] - 0.5).abs() < 1e-6));
}

#[test]
fn grain_is_deterministic_per_frame_and_respects_alpha() {
    let mut img = solid(160, 90, [0.5, 0.5, 0.5, 1.0]);
    for y in 0..20 {
        for x in 0..160 {
            let i = img.idx(x, y);
            img.data[i] = [0.0; 4];
        }
    }
    let g0 = run(&Effect::Grain { amount: 0.1, size: 1.0 }, &img, None, 0);
    assert!(g0 == run(&Effect::Grain { amount: 0.1, size: 1.0 }, &img, None, 0));
    assert!(g0 != run(&Effect::Grain { amount: 0.1, size: 1.0 }, &img, None, 1), "a semente varia por quadro");
    assert!(px(&g0, 5, 5) == [0.0; 4], "só onde alfa > 0");
    let (mut s, mut s2, mut n) = (0.0f64, 0.0f64, 0.0f64);
    for y in 20..90 {
        for x in 0..160 {
            let v = px(&g0, x, y)[0] as f64;
            s += v;
            s2 += v * v;
            n += 1.0;
        }
    }
    let (mean, sd) = (s / n, (s2 / n - (s / n).powi(2)).sqrt());
    assert!((mean - 0.5).abs() < 0.005, "média {mean}");
    assert!(sd > 0.02 && sd < 0.1, "desvio {sd}");
    assert!(run(&Effect::Grain { amount: 0.0, size: 2.0 }, &img, None, 0) == img);
    // tamanho maior = ruído correlacionado entre vizinhos
    let corr = |size: f32| {
        let g = run(&Effect::Grain { amount: 0.1, size }, &img, None, 0);
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for y in 20..90 {
            for x in 0..159 {
                let (a, b) = ((px(&g, x, y)[0] - 0.5) as f64, (px(&g, x + 1, y)[0] - 0.5) as f64);
                num += a * b;
                den += a * a;
            }
        }
        num / den
    };
    assert!(corr(6.0) > corr(1.0) + 0.3, "{} vs {}", corr(6.0), corr(1.0));
}

#[test]
fn deband_dither_reduces_banding_error() {
    // rampa lenta (25 níveis em 256 px) quantizada em 8 bits, com e sem dither
    let (w, h) = (256u32, 8u32);
    let mut ramp = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = 0.3 + 0.1 * x as f32 / w as f32;
            let i = ramp.idx(x, y);
            ramp.data[i] = [v, v, v, 1.0];
        }
    }
    let q = |img: &Image| -> Vec<f32> { img.data.iter().map(|p| (p[0] * 255.0 + 0.5).floor() / 255.0).collect() };
    let plain = q(&ramp);
    let deb = q(&run(&Effect::Deband { amount: 1.0 }, &ramp, None, 0));
    let win = |v: &[f32], bx: usize| -> f32 {
        let mut s = 0.0;
        for y in 0..h as usize {
            for x in bx..bx + 16 {
                s += v[y * w as usize + x];
            }
        }
        s / (16 * h) as f32
    };
    let (mut e_plain, mut e_deb) = (0.0f32, 0.0f32);
    for bx in (0..240).step_by(16) {
        let truth = win(&ramp.data.iter().map(|p| p[0]).collect::<Vec<_>>(), bx);
        e_plain += (win(&plain, bx) - truth).abs();
        e_deb += (win(&deb, bx) - truth).abs();
    }
    assert!(e_deb < e_plain * 0.7, "o dither deve reduzir o erro local: {e_deb} vs {e_plain}");
    // amplitude máxima = ±0,5/255 × amount
    let d = run(&Effect::Deband { amount: 1.0 }, &ramp, None, 0);
    assert!(max_diff(&ramp, &d) <= 0.5 / 255.0 + 1e-6);
    // estático (sem cintilar entre quadros) e só onde há alfa
    assert!(d == run(&Effect::Deband { amount: 1.0 }, &ramp, None, 9));
    assert!(run(&Effect::Deband { amount: 1.0 }, &Image::new(8, 8), None, 0) == Image::new(8, 8));
}

#[test]
fn drop_shadow_goes_behind_the_content() {
    let layer = disk(160, 90, 60.0, 45.0, 20.0, [1.0, 1.0, 1.0]);
    let s = run(&Effect::DropShadow { offset: [8.0, 8.0], blur: 6.0, color: [0.0, 0.0, 0.0, 0.8] }, &layer, None, 0);
    assert_valid(&s, "shadow");
    let p = px(&s, 84, 55);
    assert!(p[3] > 0.5 && p[0] < 0.05, "sombra escura embaixo-direita: {p:?}");
    assert!(px(&s, 36, 35)[3] < 0.02, "sem sombra em cima-esquerda");
    assert!(px(&s, 60, 45) == px(&layer, 60, 45), "o conteúdo opaco não muda");
    assert!(s.data.iter().zip(&layer.data).all(|(a, b)| a[3] >= b[3] - 1e-6), "o alfa só cresce");
}

#[test]
fn outline_draws_a_ring_behind() {
    let layer = disk(160, 90, 80.0, 45.0, 20.0, [0.9, 0.9, 0.9]);
    let o = run(&Effect::Outline { width: 4.0, color: [1.0, 0.0, 0.0, 1.0] }, &layer, None, 0);
    assert_valid(&o, "outline");
    let ring = px(&o, 80 + 20 + 2, 45);
    assert!(ring[3] > 0.99 && ring[0] > 0.99 && ring[1] < 0.01, "anel: {ring:?}");
    assert!(px(&o, 80 + 20 + 8, 45)[3] < 0.01, "fora do anel");
    assert!(px(&o, 80, 45) == px(&layer, 80, 45), "o interior não muda");
    assert!(px(&o, 80 + 20 + 3, 45)[3] > 0.0 && px(&o, 80 + 20 + 3, 45)[3] < 1.0, "borda do anel com anti-aliasing");
    assert!(run(&Effect::Outline { width: 0.0, color: [1.0; 4] }, &layer, None, 0) == layer);
}

#[test]
fn color_overlay_two_tone_invert_adjust() {
    let mut layer = solid(40, 20, [0.5, 0.5, 0.5, 1.0]);
    for y in 0..20 {
        for x in 30..40 {
            let i = layer.idx(x, y);
            layer.data[i] = [0.0; 4];
        }
    }
    let o = run(&Effect::ColorOverlay { color: [1.0, 0.0, 0.0, 1.0], mode: BlendMode::Normal }, &layer, None, 0);
    assert_eq!(px(&o, 5, 5), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(px(&o, 35, 5), [0.0; 4]);
    let half = run(&Effect::ColorOverlay { color: [1.0, 0.0, 0.0, 0.5], mode: BlendMode::Normal }, &layer, None, 0);
    assert!((px(&half, 5, 5)[0] - 0.75).abs() < 1e-5 && (px(&half, 5, 5)[1] - 0.25).abs() < 1e-5);
    let mul = run(&Effect::ColorOverlay { color: [0.5, 1.0, 1.0, 1.0], mode: BlendMode::Multiply }, &layer, None, 0);
    assert!((px(&mul, 5, 5)[0] - 0.25).abs() < 1e-5 && (px(&mul, 5, 5)[1] - 0.5).abs() < 1e-5);

    // invert é involutivo (inclui semi-transparente)
    let sc = make_scene(120, 68);
    let c = comp(&sc.bg, &[&sc.window]);
    for img in [&sc.ch, &c] {
        let twice = run(&Effect::Invert, &run(&Effect::Invert, img, None, 0), None, 0);
        assert!(max_diff(img, &twice) < 1e-6);
    }

    // two-tone: só duas cores
    let t = run(&Effect::TwoTone { threshold: 0.5, dark: [0.05, 0.05, 0.12], light: [1.0, 0.95, 0.85] }, &c, None, 0);
    let mut colors: Vec<[u32; 4]> = t.data.iter().map(|p| [p[0].to_bits(), p[1].to_bits(), p[2].to_bits(), p[3].to_bits()]).collect();
    colors.sort();
    colors.dedup();
    assert_eq!(colors.len(), 2, "two_tone deve produzir exatamente duas cores");
    // e preserva o alfa
    let tc = run(&Effect::TwoTone { threshold: 0.5, dark: [0.0; 3], light: [1.0; 3] }, &sc.ch, None, 0);
    assert!(tc.data.iter().zip(&sc.ch.data).all(|(a, b)| a[3] == b[3]));

    // adjust continua igual ao ajuste direto
    let adj = Adjustment::HueSaturation { hue: 30.0, saturation: -0.5, lightness: 0.1 };
    let via_fx = run(&Effect::Adjust { adjustment: adj.clone() }, &c, None, 0);
    let mut direct = c.clone();
    adj.apply_image(&mut direct, 1.0, None);
    assert!(via_fx == direct);
}

// ───────────────────────────── cor: Grade ─────────────────────────────

fn grade(f: impl Fn(&mut [f32; 3], &mut [f32; 3], &mut [f32; 3], &mut f32, &mut f32, &mut f32, &mut f32, &mut f32)) -> Effect {
    let (mut lift, mut gamma, mut gain, mut sat, mut temp, mut tint, mut con, mut piv) = ([0.0; 3], [1.0; 3], [1.0; 3], 1.0, 0.0, 0.0, 1.0, 0.5);
    f(&mut lift, &mut gamma, &mut gain, &mut sat, &mut temp, &mut tint, &mut con, &mut piv);
    Effect::Grade { lift, gamma, gain, saturation: sat, temperature: temp, tint, contrast: con, pivot: piv }
}

#[test]
fn grade_operations() {
    let sc = make_scene(160, 90);
    let c = comp(&sc.bg, &[&sc.window, &sc.ch]);
    let id = grade(|_, _, _, _, _, _, _, _| {});
    assert!(max_diff(&c, &run(&id, &c, None, 0)) < 1e-6, "grade neutro = identidade");
    // lift levanta os pretos e não mexe nos brancos
    let lifted = run(&grade(|l, _, _, _, _, _, _, _| *l = [0.1; 3]), &solid(4, 4, [0.0, 0.0, 0.0, 1.0]), None, 0);
    assert!((px(&lifted, 0, 0)[0] - 0.1).abs() < 1e-5);
    let white = run(&grade(|l, _, _, _, _, _, _, _| *l = [0.1; 3]), &solid(4, 4, [1.0; 4]), None, 0);
    assert!((px(&white, 0, 0)[0] - 1.0).abs() < 1e-5);
    // gain escala; gamma > 1 clareia os meios-tons
    let g = run(&grade(|_, _, g, _, _, _, _, _| *g = [0.5, 1.0, 1.0]), &solid(4, 4, [0.8, 0.8, 0.8, 1.0]), None, 0);
    assert!((px(&g, 0, 0)[0] - 0.4).abs() < 1e-5 && (px(&g, 0, 0)[1] - 0.8).abs() < 1e-5);
    let gm = run(&grade(|_, ga, _, _, _, _, _, _| *ga = [2.0; 3]), &solid(4, 4, [0.25, 0.25, 0.25, 1.0]), None, 0);
    assert!((px(&gm, 0, 0)[0] - 0.5).abs() < 1e-5);
    // saturação 0 = cinza (luma Rec.709)
    let s0 = run(&grade(|_, _, _, s, _, _, _, _| *s = 0.0), &solid(4, 4, [0.9, 0.2, 0.4, 1.0]), None, 0);
    let p = px(&s0, 0, 0);
    assert!((p[0] - p[1]).abs() < 1e-6 && (p[1] - p[2]).abs() < 1e-6);
    assert!((p[0] - (0.2126 * 0.9 + 0.7152 * 0.2 + 0.0722 * 0.4)).abs() < 1e-5);
    // contraste em torno do pivô: pivô fixo, claros sobem, escuros descem
    let k = grade(|_, _, _, _, _, _, c, p| {
        *c = 1.5;
        *p = 0.4;
    });
    assert!((px(&run(&k, &solid(2, 2, [0.4, 0.4, 0.4, 1.0]), None, 0), 0, 0)[0] - 0.4).abs() < 1e-5);
    assert!(px(&run(&k, &solid(2, 2, [0.6, 0.6, 0.6, 1.0]), None, 0), 0, 0)[0] > 0.65);
    assert!(px(&run(&k, &solid(2, 2, [0.2, 0.2, 0.2, 1.0]), None, 0), 0, 0)[0] < 0.2 - 0.1);
    // temperatura positiva esquenta: R sobe, B desce
    let w = run(&grade(|_, _, _, _, t, _, _, _| *t = 0.5), &solid(2, 2, [0.5, 0.5, 0.5, 1.0]), None, 0);
    let p = px(&w, 0, 0);
    assert!(p[0] > 0.5 && p[2] < 0.5);
    // com alfa < 1: a cor reta é graduada e re-pré-multiplicada (alfa intacto)
    let half = run(&grade(|_, _, g, _, _, _, _, _| *g = [0.5; 3]), &solid(2, 2, [0.8, 0.8, 0.8, 0.5]), None, 0);
    let p = px(&half, 0, 0);
    assert!((p[0] / p[3] - 0.4).abs() < 1e-5 && p[3] == 0.5);
    assert_valid(&run(&teal_orange(), &c, None, 0), "grade");
}

// ───────────────────────────── cor: LUT ─────────────────────────────

fn lut_blob(text: &str) -> (BlobStore, BlobHash) {
    let mut b = BlobStore::default();
    let h = b.put(text.as_bytes().to_vec());
    (b, h)
}

fn apply_lut(blobs: &BlobStore, hash: &BlobHash, intensity: f32, img: &Image) -> Image {
    let mut out = img.clone();
    Effect::Lut { blob: hash.clone(), intensity }.apply(&mut out, &FxContext { frame: 0, below: None, blobs, seed: 0 }).unwrap();
    out
}

#[test]
fn lut_identity_cube_is_identity() {
    let sc = make_scene(160, 90);
    let c = comp(&sc.bg, &[&sc.window, &sc.ch]);
    for size in [2usize, 17, 33, 65] {
        let (blobs, h) = lut_blob(&Lut::identity(size).to_cube());
        let out = apply_lut(&blobs, &h, 1.0, &c);
        assert!(max_diff(&c, &out) < 2e-6, "LUT identidade {size}: {}", max_diff(&c, &out));
        let out = apply_lut(&blobs, &h, 1.0, &sc.ch);
        assert!(max_diff(&sc.ch, &out) < 2e-6);
    }
}

#[test]
fn cube_parser_accepts_the_format_and_rejects_garbage() {
    let text = "# criado à mão\r\nTITLE \"Inverter\"\r\n\r\nLUT_3D_SIZE 2\r\nDOMAIN_MIN 0.0 0.0 0.0\r\nDOMAIN_MAX 1.0 1.0 1.0\r\n\
        1 1 1\r\n0 1 1\r\n1 0 1\r\n0 0 1\r\n1 1 0\r\n0 1 0\r\n1 0 0\r\n0 0 0\r\n";
    let l = Lut::parse(text).unwrap();
    assert_eq!((l.title.as_str(), l.size, l.kind), ("Inverter", 2, LutKind::D3));
    let o = l.sample([0.25, 0.5, 1.0]);
    assert!((o[0] - 0.75).abs() < 1e-6 && (o[1] - 0.5).abs() < 1e-6 && o[2].abs() < 1e-6, "{o:?}");
    // BOM
    assert!(Lut::parse(&format!("\u{feff}{text}")).is_ok());
    // domínio: entrada limitada e normalizada
    let d = Lut::parse("LUT_3D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 0.5 0.5 0.5\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").unwrap();
    let o = d.sample([0.25, 0.1, 0.9]);
    assert!((o[0] - 0.5).abs() < 1e-6 && (o[1] - 0.2).abs() < 1e-6 && (o[2] - 1.0).abs() < 1e-6, "{o:?}");
    // 1D
    let one = Lut::parse("LUT_1D_SIZE 3\n0 0 0\n0.8 0.8 0.8\n1 1 1\n").unwrap();
    assert_eq!(one.kind, LutKind::D1);
    assert!((one.sample([0.5, 0.25, 1.0])[0] - 0.8).abs() < 1e-6 && (one.sample([0.5, 0.25, 1.0])[1] - 0.4).abs() < 1e-6);
    // erros
    for bad in [
        "TITLE \"x\"\n0 0 0\n",
        "LUT_3D_SIZE 2\n0 0 0\n",
        "LUT_3D_SIZE 1\n0 0 0\n",
        "LUT_3D_SIZE 66\n",
        "LUT_3D_SIZE 2\nDOMAIN_MIN 1 1 1\nDOMAIN_MAX 0 0 0\n0 0 0\n0 0 0\n0 0 0\n0 0 0\n0 0 0\n0 0 0\n0 0 0\n0 0 0\n",
        "LUT_3D_SIZE 2\nBANANA 1\n",
        "LUT_3D_SIZE 2\n0 0 x\n",
        "LUT_3D_SIZE 2\n0 0 NaN\n",
        "LUT_3D_SIZE 2\nLUT_1D_SIZE 2\n",
    ] {
        assert!(Lut::parse(bad).is_err(), "deveria falhar: {bad:?}");
    }
    // blob que não existe / não é texto
    let blobs = BlobStore::default();
    let mut img = solid(4, 4, [0.5; 4]);
    let r = Effect::Lut { blob: BlobHash::of(b"nada"), intensity: 1.0 }.apply(&mut img, &ctx(&blobs, None, 0));
    assert!(r.is_err());
    let (b2, h2) = lut_blob("\u{0}\u{1}");
    let mut img = solid(4, 4, [0.5; 4]);
    assert!(Effect::Lut { blob: h2, intensity: 1.0 }.apply(&mut img, &ctx(&b2, None, 0)).is_err());
}

#[test]
fn lut_intensity_mixes_and_works_on_premultiplied_alpha() {
    let inv = bake_lut(&[Effect::Invert], 17, "inv", &BlobStore::default()).unwrap();
    let (blobs, h) = lut_blob(&inv.to_cube());
    let img = solid(8, 8, [0.2, 0.4, 0.6, 1.0]);
    let full = apply_lut(&blobs, &h, 1.0, &img);
    assert!((px(&full, 0, 0)[0] - 0.8).abs() < 1.5e-3, "{:?}", px(&full, 0, 0));
    let half = apply_lut(&blobs, &h, 0.5, &img);
    assert!((px(&half, 0, 0)[0] - 0.5).abs() < 1.5e-3);
    assert!(apply_lut(&blobs, &h, 0.0, &img) == img);
    // alfa < 1: cor reta transformada, alfa mantido
    let tr = solid(8, 8, [0.2, 0.4, 0.6, 0.5]);
    let o = apply_lut(&blobs, &h, 1.0, &tr);
    let p = px(&o, 0, 0);
    assert!(p[3] == 0.5 && (p[0] / p[3] - 0.8).abs() < 1.5e-3);
}

#[test]
fn exported_lut_reproduces_the_grade_and_roundtrips() {
    let blobs = BlobStore::default();
    let mut r = Rng::new(11);
    let mut samples = vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.5, 0.5, 0.5]];
    for _ in 0..4000 {
        samples.push([r.f32(), r.f32(), r.f32()]);
    }
    let mut img = Image::new(samples.len() as u32, 1);
    for (i, s) in samples.iter().enumerate() {
        img.data[i] = [s[0], s[1], s[2], 1.0];
    }
    for (name, g) in [("sunset", grade_sunset()), ("night", grade_night()), ("day", grade_day()), ("teal_orange", teal_orange())] {
        let direct = run(&g, &img, None, 0);
        for (size, tol) in [(33usize, 1.0 / 255.0), (65, 0.5 / 255.0)] {
            let lut = bake_lut(std::slice::from_ref(&g), size, name, &blobs).unwrap();
            // reimportado do texto (6 casas) = o mesmo resultado
            let text = lut.to_cube();
            let (b2, h) = lut_blob(&text);
            let via = apply_lut(&b2, &h, 1.0, &img);
            let e = max_diff(&direct, &via);
            assert!(e <= tol, "{name} {size}³: LUT exportada difere do Grade em {e} (> {tol})");
            let back = Lut::parse(&text).unwrap();
            for (a, b) in lut.data.iter().zip(&back.data) {
                for k in 0..3 {
                    assert!((a[k] - b[k]).abs() < 6e-7, "ida e volta do texto");
                }
            }
            assert_eq!(back.size, size);
        }
    }
    // cadeia Grade + Invert cozinhada em uma LUT: dentro de 1/255
    let chain = [grade_day(), Effect::Invert];
    let lut = bake_lut(&chain, 33, "chain", &blobs).unwrap();
    let mut via = img.clone();
    lut.apply_image(&mut via, 1.0);
    let mut direct = img.clone();
    for e in &chain {
        e.apply(&mut direct, &ctx(&blobs, None, 0)).unwrap();
    }
    assert!(max_diff(&direct, &via) <= 1.0 / 255.0, "cadeia: {}", max_diff(&direct, &via));
    // com Adjust (HSL tem quinas): a LUT 33³ erra mais (limite documentado: 5/255); 65³ melhora
    let hsl = [grade_day(), Effect::Adjust { adjustment: Adjustment::HueSaturation { hue: 20.0, saturation: 0.1, lightness: 0.0 } }];
    let mut direct = img.clone();
    for e in &hsl {
        e.apply(&mut direct, &ctx(&blobs, None, 0)).unwrap();
    }
    let mut errs = Vec::new();
    for size in [33usize, 65] {
        let mut via = img.clone();
        bake_lut(&hsl, size, "hsl", &blobs).unwrap().apply_image(&mut via, 1.0);
        errs.push(max_diff(&direct, &via));
    }
    assert!(errs[0] < 5.0 / 255.0 && errs[1] <= errs[0], "Adjust HSL: {errs:?}");
    // efeito não pontual não vira LUT
    assert!(bake_lut(&[Effect::GaussianBlur { radius: 3.0 }], 17, "x", &blobs).is_err());
    // LUT dentro de uma cadeia que vira LUT (grade → .cube → cozinhada de novo)
    let (b3, h3) = lut_blob(&bake_lut(&[grade_sunset()], 33, "s", &blobs).unwrap().to_cube());
    let again = bake_lut(&[Effect::Lut { blob: h3, intensity: 1.0 }], 33, "again", &b3).unwrap();
    let mut a = img.clone();
    again.apply_image(&mut a, 1.0);
    assert!(max_diff(&run(&grade_sunset(), &img, None, 0), &a) < 2.0 / 255.0);
}

// ───────────────────────────── scopes ─────────────────────────────

#[test]
fn scopes_measure_what_they_show() {
    // histograma de cor plana
    let flat = solid(40, 30, [0.2, 0.5, 0.8, 1.0]);
    let h = scopes::histogram(&flat);
    assert_eq!((h.r[51], h.g[128], h.b[204]), (1200, 1200, 1200));
    assert_eq!(h.luma.iter().sum::<u32>(), 1200);
    assert_eq!(h.pixels, 1200);
    assert_eq!((h.image.width, h.image.height), (256, 160));
    // pixels transparentes não contam
    let mut half = flat.clone();
    for p in half.data.iter_mut().take(600) {
        *p = [0.0; 4];
    }
    assert_eq!(scopes::histogram(&half).pixels, 600);

    // forma de onda de uma rampa horizontal: coluna x tem nível x
    let mut ramp = Image::new(256, 16);
    for y in 0..16 {
        for x in 0..256 {
            let v = x as f32 / 255.0;
            let i = ramp.idx(x, y);
            ramp.data[i] = [v, v, v, 1.0];
        }
    }
    let w = scopes::waveform(&ramp);
    assert_eq!((w.image.width, w.image.height), (256, 256));
    for x in [0usize, 10, 100, 255] {
        assert_eq!(w.counts[x * 256 + x], 16, "coluna {x}");
    }
    let lit = w.image.data[(255 - 100) * 256 + 100];
    assert!(lit[0] > 0.3, "traço visível na diagonal");

    // parade de vermelho puro
    let red = solid(60, 10, [1.0, 0.0, 0.0, 1.0]);
    let p = scopes::rgb_parade(&red);
    assert_eq!(p.image.width, 3 * p.sub_width);
    assert!(p.r[255] > 0 && p.g[0] > 0 && p.b[0] > 0 && p.r[0] == 0 && p.g[255] == 0);
    assert_eq!(p.r.iter().sum::<u32>(), 600);

    // vetorscópio: cinza no centro, vermelho no canto superior esquerdo (Cb<0, Cr>0)
    let gray = solid(10, 10, [0.5, 0.5, 0.5, 1.0]);
    let v = scopes::vectorscope(&gray);
    assert_eq!(v.counts[128 * 256 + 128], 100);
    assert_eq!(v.counts.iter().sum::<u32>(), 100);
    assert_eq!((v.image.width, v.image.height), (256, 256));
    let vr = scopes::vectorscope(&solid(10, 10, [1.0, 0.0, 0.0, 1.0]));
    let (x, y) = scopes::vectorscope_pos([1.0, 0.0, 0.0]);
    assert!(x < 128 && y < 128);
    assert_eq!(vr.counts[y * 256 + x], 100);
    // determinismo e imagens opacas válidas
    let sc = make_scene(200, 112);
    let c = comp(&sc.bg, &[&sc.window, &sc.ch]);
    for im in [&scopes::histogram(&c).image, &scopes::waveform(&c).image, &scopes::rgb_parade(&c).image, &scopes::vectorscope(&c).image] {
        assert!(im.data.iter().all(|p| p[3] == 1.0 && p[..3].iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0)));
    }
    assert_eq!(scopes::waveform(&c).counts, scopes::waveform(&c).counts);
}

// ───────────────────────────── evidências (FX_EVIDENCE=1) ─────────────────────────────

fn evidence_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../sprints/registro/evidencias/S13")
}

fn want_evidence() -> bool {
    std::env::var("FX_EVIDENCE").is_ok()
}

const FONT: &[(char, [u8; 5])] = &[
    ('A', [2, 5, 7, 5, 5]), ('B', [6, 5, 6, 5, 6]), ('C', [3, 4, 4, 4, 3]), ('D', [6, 5, 5, 5, 6]), ('E', [7, 4, 6, 4, 7]),
    ('F', [7, 4, 6, 4, 4]), ('G', [3, 4, 5, 5, 3]), ('H', [5, 5, 7, 5, 5]), ('I', [7, 2, 2, 2, 7]), ('J', [1, 1, 1, 5, 2]),
    ('K', [5, 5, 6, 5, 5]), ('L', [4, 4, 4, 4, 7]), ('M', [5, 7, 7, 5, 5]), ('N', [6, 5, 5, 5, 5]), ('O', [2, 5, 5, 5, 2]),
    ('P', [6, 5, 6, 4, 4]), ('Q', [2, 5, 5, 6, 3]), ('R', [6, 5, 6, 5, 5]), ('S', [3, 4, 2, 1, 6]), ('T', [7, 2, 2, 2, 2]),
    ('U', [5, 5, 5, 5, 7]), ('V', [5, 5, 5, 5, 2]), ('W', [5, 5, 7, 7, 5]), ('X', [5, 5, 2, 5, 5]), ('Y', [5, 5, 2, 2, 2]),
    ('Z', [7, 1, 2, 4, 7]), ('0', [7, 5, 5, 5, 7]), ('1', [2, 6, 2, 2, 7]), ('2', [6, 1, 2, 4, 7]), ('3', [6, 1, 6, 1, 6]),
    ('4', [5, 5, 7, 1, 1]), ('5', [7, 4, 6, 1, 6]), ('6', [3, 4, 7, 5, 7]), ('7', [7, 1, 2, 2, 2]), ('8', [7, 5, 7, 5, 7]),
    ('9', [7, 5, 7, 1, 6]), ('_', [0, 0, 0, 0, 7]), ('-', [0, 0, 7, 0, 0]), ('.', [0, 0, 0, 0, 2]), ('(', [2, 4, 4, 4, 2]),
    (')', [2, 1, 1, 1, 2]), ('/', [1, 1, 2, 4, 4]), (':', [0, 2, 0, 2, 0]),
];

fn draw_text(img: &mut Image, x0: u32, y0: u32, text: &str, scale: u32, col: [f32; 3]) {
    let mut x = x0;
    for ch in text.chars() {
        let up = ch.to_ascii_uppercase();
        if let Some((_, rows)) = FONT.iter().find(|(c, _)| *c == up) {
            for (ry, row) in rows.iter().enumerate() {
                for rx in 0..3u32 {
                    if row & (4 >> rx) != 0 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                let (px_, py) = (x + rx * scale + sx, y0 + ry as u32 * scale + sy);
                                if px_ < img.width && py < img.height {
                                    let i = img.idx(px_, py);
                                    img.data[i] = [col[0], col[1], col[2], 1.0];
                                }
                            }
                        }
                    }
                }
            }
        }
        x += 4 * scale;
    }
}

fn contact_sheet(tiles: &[(String, Image)], cols: u32, tw: u32, th: u32) -> Image {
    let label_h = 14;
    let rows = (tiles.len() as u32).div_ceil(cols);
    let mut sheet = solid(cols * tw, rows * (th + label_h), [0.09, 0.09, 0.1, 1.0]);
    for (i, (label, img)) in tiles.iter().enumerate() {
        let (cx, cy) = (i as u32 % cols, i as u32 / cols);
        let (ox, oy) = (cx * tw, cy * (th + label_h));
        for y in 0..th.min(img.height) {
            for x in 0..tw.min(img.width) {
                let s = img.get(x, y);
                let mut d = [0.0, 0.0, 0.0, 1.0];
                // sobre xadrez para enxergar o alfa
                let chk = if ((x / 8) + (y / 8)) % 2 == 0 { 0.30 } else { 0.42 };
                for k in 0..3 {
                    d[k] = s[k] + chk * (1.0 - s[3]);
                }
                let j = sheet.idx(ox + x, oy + label_h + y);
                sheet.data[j] = d;
            }
        }
        draw_text(&mut sheet, ox + 3, oy + 2, label, 2, [0.95, 0.95, 0.8]);
    }
    sheet
}

#[test]
fn evidence() {
    if !want_evidence() {
        eprintln!("evidence: pulado (defina FX_EVIDENCE=1)");
        return;
    }
    let dir = evidence_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let (tw, th) = (400u32, 225u32);
    let (sc, blobs, hash) = setup(tw, th);
    let original = comp(&sc.bg, &[&sc.window, &sc.ch]);
    let mut tiles: Vec<(String, Image)> = vec![("original".into(), original.clone())];
    let mut items = catalog(tw, th, &hash);
    // variações extras (looks de cor da S15, glow em tela colorido, blur direcional vertical)
    let f = tw as f32;
    items.push(Item { label: "glow (screen, azul)".into(), effect: Effect::Glow { threshold: 0.65, radius: 0.07 * f, intensity: 1.4, color: Some([0.4, 0.7, 1.0]), screen: true }, target: Target::Comp });
    items.push(Item { label: "gaussian_blur gaussian vs noisy".into(), effect: Effect::GaussianBlur { radius: 0.015 * f }, target: Target::CompNoisy });
    items.push(Item { label: "grade day".into(), effect: grade_day(), target: Target::Comp });
    items.push(Item { label: "grade sunset".into(), effect: grade_sunset(), target: Target::Comp });
    items.push(Item { label: "grade night".into(), effect: grade_night(), target: Target::Comp });
    for item in &items {
        let (_, after, tile) = render_item(item, &sc, &blobs);
        assert_valid(&after, &item.label);
        tiles.push((item.label.clone(), tile));
    }
    let sheet = contact_sheet(&tiles, 4, tw, th);
    assert!(sheet.width <= 1600);
    sheet.save_png(&dir.join("contact_sheet.png")).unwrap();

    // pilha típica de um corte: BG + personagem com para/rim/light wrap, janela com 透過光, difusão, glow, grão
    let below_ch = comp(&sc.bg, &[&sc.window]);
    let mut win = sc.window.clone();
    let mut ch = sc.ch.clone();
    let cx = |frame| FxContext { frame, below: None, blobs: &blobs, seed: 5 };
    let inst = |e: Effect| EffectInstance { effect: e, enabled: true, posterize_time: 1, keys: KeyTracks::default() };
    let w1 = apply_stack(&mut win, &[inst(Effect::TransmittedLight { radius: 0.03 * f, intensity: 0.45, color: [1.0, 0.85, 0.6] })], &cx(0));
    let ctx_ch = FxContext { frame: 0, below: Some(&below_ch), blobs: &blobs, seed: 5 };
    let w2 = apply_stack(
        &mut ch,
        &[
            inst(Effect::Para { from: [0.33 * f, 0.3 * th as f32], to: [0.33 * f, 0.95 * th as f32], color_from: [1.0, 1.0, 1.0, 0.0], color_to: [0.15, 0.1, 0.4, 0.6], mode: BlendMode::Multiply }),
            inst(Effect::LightWrap { radius: 0.03 * f, amount: 0.6 }),
            inst(Effect::RimLight { angle: 345.0, width: 0.01 * f, color: [1.0, 0.9, 0.7], intensity: 0.9 }),
        ],
        &ctx_ch,
    );
    assert!(w1.is_empty() && w2.is_empty(), "{w1:?} {w2:?}");
    let mut cut = comp(&sc.bg, &[&win, &ch]);
    let w3 = apply_stack(
        &mut cut,
        &[
            inst(Effect::Diffusion { radius: 0.03 * f, amount: 0.18, threshold: 0.6 }),
            inst(Effect::Glow { threshold: 0.92, radius: 0.05 * f, intensity: 0.5, color: None, screen: false }),
            inst(grade_sunset()),
        ],
        &cx(0),
    );
    assert!(w3.is_empty());
    let cut_graded = cut.clone(); // scopes medem o corte graduado, antes de vinheta e grão
    let w4 = apply_stack(
        &mut cut,
        &[inst(Effect::Vignette { amount: 0.4, softness: 0.7, color: None }), inst(Effect::Grain { amount: 0.03, size: 1.2 })],
        &cx(0),
    );
    assert!(w4.is_empty());
    let pair = contact_sheet(&[("cut sem efeitos".into(), original.clone()), ("cut com pilha de satsuei".into(), cut.clone())], 2, tw, th);
    pair.save_png(&dir.join("stack_before_after.png")).unwrap();

    // scopes do corte final e do original (S15)
    let scopes_sheet = {
        let hs = scopes::histogram(&cut_graded).image;
        let wf = scopes::waveform(&cut_graded).image;
        let pa = scopes::rgb_parade(&cut_graded).image;
        let vs = scopes::vectorscope(&cut_graded).image;
        let (w0, h0) = (wf.width.max(pa.width) + 256 + 256 + 24, 256 + 16);
        let mut s = solid(w0, h0 + 4, [0.05, 0.05, 0.06, 1.0]);
        let mut put = |im: &Image, ox: u32, oy: u32| {
            for y in 0..im.height {
                for x in 0..im.width {
                    if ox + x < s.width && oy + y < s.height {
                        let i = s.idx(ox + x, oy + y);
                        s.data[i] = im.get(x, y);
                    }
                }
            }
        };
        put(&wf, 4, 4);
        put(&hs, wf.width.max(pa.width) + 12, 4);
        put(&vs, wf.width.max(pa.width) + 256 + 20, 4);
        let _ = pa;
        s
    };
    scopes_sheet.save_png(&dir.join("scopes_waveform_histogram_vectorscope.png")).unwrap();
    scopes::rgb_parade(&cut_graded).image.save_png(&dir.join("scopes_parade.png")).unwrap();

    // .cube de exemplo (17³) do look "sunset": versão portável (0..1) gravada em disco; reimportada e comparada com o
    // Grade direto, junto do erro da tabela sem limite (a que o motor usa internamente)
    let sunset = bake_lut(&[grade_sunset()], 17, "AnimeIAI sunset (Grade lift/gamma/gain + temperatura)", &blobs).unwrap();
    let text = sunset.clamped().to_cube();
    std::fs::write(dir.join("sunset_17.cube"), &text).unwrap();
    let back = Lut::parse(&std::fs::read_to_string(dir.join("sunset_17.cube")).unwrap()).unwrap();
    assert_eq!((back.size, back.data.len()), (17, 17 * 17 * 17));
    assert!(back.data.iter().all(|v| v.iter().all(|c| (0.0..=1.0).contains(c))));
    let mut direct = original.clone();
    grade_sunset().apply(&mut direct, &ctx(&blobs, None, 0)).unwrap();
    let mut via = original.clone();
    back.apply_image(&mut via, 1.0);
    let mut via_raw = original.clone();
    sunset.apply_image(&mut via_raw, 1.0);
    let (e_clamped, e_raw) = (max_diff(&direct, &via), max_diff(&direct, &via_raw));
    std::fs::write(
        dir.join("lut_check.txt"),
        format!(
            "Cena de teste, Grade sunset direto vs LUT 17^3:
  sunset_17.cube (portável, 0..1) reimportado: erro máximo {:.5} ({:.2}/255)
  tabela sem limite (uso interno, aplicador limita depois): erro máximo {:.5} ({:.2}/255)
",
            e_clamped, e_clamped * 255.0, e_raw, e_raw * 255.0
        ),
    )
    .unwrap();
}

fn time_ms<F: FnMut()>(mut f: F) -> (f64, f64) {
    f(); // aquecimento (alocação de páginas, pool do rayon)
    let mut t: Vec<f64> = (0..3)
        .map(|_| {
            let s = Instant::now();
            f();
            s.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    t.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (t[0], t[1])
}

#[test]
fn timings_1080p() {
    let big = want_evidence() || !cfg!(debug_assertions);
    if !big {
        eprintln!("timings_1080p: pulado no perfil de debug (use --release ou FX_EVIDENCE=1)");
        return;
    }
    let (w, h) = (1920u32, 1080u32);
    let (sc, blobs, hash) = setup(w, h);
    let c0 = comp(&sc.bg, &[&sc.window, &sc.ch]);
    let below_ch = comp(&sc.bg, &[&sc.window]);
    let noisy = add_noise(&c0, 0.05);
    let mut items = catalog(w, h, &hash);
    // casos de pior caso extras
    let f = w as f32;
    items.push(Item { label: "gaussian_blur r=30 (exato, pior caso)".into(), effect: Effect::GaussianBlur { radius: 30.0 }, target: Target::Comp });
    items.push(Item { label: "gaussian_blur r=120 (caixa×3)".into(), effect: Effect::GaussianBlur { radius: 120.0 }, target: Target::Comp });
    items.push(Item { label: "directional_blur length=200".into(), effect: Effect::DirectionalBlur { angle: 35.0, length: 200.0 }, target: Target::Comp });
    items.push(Item { label: "radial_blur amount=0.5".into(), effect: Effect::RadialBlur { center: [0.5 * f, 540.0], amount: 0.5 }, target: Target::Comp });
    items.push(Item { label: "color_aware_blur r=30".into(), effect: Effect::ColorAwareBlur { radius: 30.0, tolerance: 0.12 }, target: Target::CompNoisy });
    items.push(Item { label: "glow r=200".into(), effect: Effect::Glow { threshold: 0.7, radius: 200.0, intensity: 1.0, color: None, screen: false }, target: Target::Comp });
    items.push(Item { label: "lut 33^3 (aplicar)".into(), effect: Effect::Lut { blob: hash.clone(), intensity: 1.0 }, target: Target::Comp });
    let mut rows = String::new();
    let mut worst = 0.0f64;
    let mut slow: Vec<String> = Vec::new();
    for item in &items {
        let (base, below): (&Image, &Image) = match item.target {
            Target::Comp => (&c0, &c0),
            Target::CompNoisy => (&noisy, &noisy),
            Target::Window => (&sc.window, &sc.bg),
            Target::Char => (&sc.ch, &below_ch),
        };
        let (best, med) = time_ms(|| {
            let mut img = base.clone();
            item.effect.apply(&mut img, &FxContext { frame: 1, below: Some(below), blobs: &blobs, seed: 3 }).unwrap();
            std::hint::black_box(&img);
        });
        // custo da cópia da imagem (não é do efeito): mede e desconta
        let (copy, _) = time_ms(|| {
            let img = base.clone();
            std::hint::black_box(&img);
        });
        let net = (best - copy).max(0.0);
        worst = worst.max(net);
        if net > 400.0 {
            slow.push(format!("{} ({net:.0} ms)", item.label));
        }
        rows.push_str(&format!("| {} | {:.0} | {:.0} | {} |\n", item.label, net, med - copy.min(med), if net <= 400.0 { "ok" } else { "ACIMA de 400 ms" }));
    }
    // scopes e LUT
    let mut extra = String::new();
    let mut add = |name: &str, f: &mut dyn FnMut()| {
        let (b, m) = time_ms(f);
        extra.push_str(&format!("| {name} | {b:.0} | {m:.0} |\n"));
    };
    add("scopes::histogram", &mut || {
        std::hint::black_box(scopes::histogram(&c0));
    });
    add("scopes::waveform", &mut || {
        std::hint::black_box(scopes::waveform(&c0));
    });
    add("scopes::rgb_parade", &mut || {
        std::hint::black_box(scopes::rgb_parade(&c0));
    });
    add("scopes::vectorscope", &mut || {
        std::hint::black_box(scopes::vectorscope(&c0));
    });
    let cube65 = Lut::identity(65).to_cube();
    add("Lut::parse 65^3 (274625 linhas)", &mut || {
        std::hint::black_box(Lut::parse(&cube65).unwrap());
    });
    add("bake_lut Grade 33^3", &mut || {
        std::hint::black_box(bake_lut(&[grade_sunset()], 33, "x", &blobs).unwrap());
    });
    let release = !cfg!(debug_assertions);
    let md = format!(
        "# S13/S15 - tempos dos efeitos em 1920x1080\n\nMáquina: i7-10750H (6C/12T), CPU, rayon. Perfil: {}. Cena: céu + janela + personagem + pontos (opaca, 1920x1080, RGBA f32).\n\
         Tempo = melhor de 3 execuções (depois de 1 de aquecimento) já sem o custo de clonar a imagem; coluna 3 = mediana. Meta: <= 400 ms.\n\n\
         | efeito (parâmetros em fração da largura) | melhor ms | mediana ms | meta |\n|---|---:|---:|---|\n{rows}\n\
         ## Scopes e LUT\n\n| operação | melhor ms | mediana ms |\n|---|---:|---:|\n{extra}\n\
         Pior efeito: {worst:.0} ms. {}\n",
        if release { "release (lto thin)" } else { "dev (opt-level 1) - NÃO representativo" },
        if slow.is_empty() { "Todos dentro da meta de 400 ms.".to_string() } else { format!("Acima da meta: {}.", slow.join("; ")) }
    );
    if want_evidence() {
        let dir = evidence_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("timings.md"), md.as_bytes()).unwrap();
    }
    eprintln!("{md}");
    if release {
        assert!(worst < 3000.0, "algum efeito passou de 3 s em 1080p: {worst} ms");
    }
}
