//! Métricas entre dois PNG (diagnóstico de calibração: piso de ruído/textura e perturbações de referência).
//! uso: cargo run -p aiai-metrics --example flip_pair --release -- a.png b.png
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("uso: flip_pair a.png b.png");
        std::process::exit(2);
    }
    let load = |p: &str| aiai_paint::Image::load_png(std::path::Path::new(p)).unwrap_or_else(|e| panic!("{p}: {e}"));
    let (a, b) = (load(&args[1]), load(&args[2]));
    let r = aiai_metrics::compare(&a, &b, &aiai_metrics::CompareOptions::default()).expect("tamanhos iguais");
    println!(
        "flip {:.4} p50 {:.4} | ssim {:.4} | dE_plano {:.2} dE {:.2} | F1 bordas {:.3} | linha med {:.2}/{:.2}",
        r.flip.mean, r.flip.p50, r.ssim, r.delta_e.flat_mean, r.delta_e.overall_mean, r.edges.f1,
        r.line_width.reference.median, r.line_width.test.median
    );
}
