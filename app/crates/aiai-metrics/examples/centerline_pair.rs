//! Linha central (Steger) entre dois PNG: desvio da linha central, largura pareada e R² do perfil w(s) (auditoria 4.9h).
//! uso: cargo run -p aiai-metrics --example centerline_pair --release -- referencia.png teste.png [sigma] [--json]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("uso: centerline_pair referencia.png teste.png [sigma] [--json]");
        std::process::exit(2);
    }
    let load = |p: &str| aiai_paint::Image::load_png(std::path::Path::new(p)).unwrap_or_else(|e| panic!("{p}: {e}"));
    let (a, b) = (load(&args[1]), load(&args[2]));
    let sigma = args.get(3).and_then(|s| s.parse::<f32>().ok());
    let params = aiai_metrics::StegerParams { sigma, ..Default::default() };
    let t0 = std::time::Instant::now();
    let (ra, rb, c) = aiai_metrics::centerline_compare(&a, &b, &params, 3.0);
    let ms = t0.elapsed().as_millis();
    if args.iter().any(|a| a == "--json") {
        let v = serde_json::json!({ "sigma": ra.sigma, "reference": ra.width_stats(), "test": rb.width_stats(), "compare": c, "ms": ms });
        println!("{v}");
        return;
    }
    let (sa, sb) = (ra.width_stats(), rb.width_stats());
    println!(
        "sigma {:.2} | pontos {}/{} junções {}/{} | recall {:.3} precisão {:.3} | desvio médio {:.3} p90 {:.3} px | largura {:.2}/{:.2} (med {:.2}/{:.2}) erro {:.3} R² {:.3} | {} {} | {ms} ms",
        ra.sigma, c.ref_points, c.test_points, c.junctions_ref, c.junctions_test, c.recall, c.precision, c.deviation_mean, c.deviation_p90,
        c.width_ref_mean, c.width_test_mean, sa.median, sb.median, c.width_abs_err, c.width_r2,
        if c.deviation_ok { "centro ok" } else { "CENTRO FORA" }, if c.width_profile_ok { "w(s) ok" } else { "w(s) FORA" }
    );
}
