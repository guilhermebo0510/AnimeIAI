//! Diagnóstico: grava a referência sob P5T (decisão 24) para conferir o tremor. uso: p5t_dump ref.png saida.png [k] [amp]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let img = aiai_paint::Image::load_png(std::path::Path::new(&a[1])).expect("ref");
    let k: f32 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(1.05);
    let amp: f32 = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let out = aiai_tools::compare::smooth_warp(&aiai_tools::compare::perturb(&img, k, 1, 0), amp, 0x5EED_0001);
    out.save_png(std::path::Path::new(&a[2])).expect("salvar");
}
