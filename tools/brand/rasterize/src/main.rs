//! Rasteriza SVG em PNG com o resvg do app (o mesmo caminho de `icons::rasterize`).
//!
//! Uso: `brand-rasterize <svg> <png> <largura> [altura] [--bg #rrggbb]`
//! Sem altura, o PNG segue a proporcao do viewBox. `--bg` pinta um fundo opaco por baixo (so para conferir a leitura).

use std::path::PathBuf;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut bg: Option<[u8; 3]> = None;
    if let Some(i) = args.iter().position(|a| a == "--bg") {
        let hex = args.get(i + 1).expect("--bg precisa de #rrggbb").trim_start_matches('#').to_string();
        let v = u32::from_str_radix(&hex, 16).expect("cor invalida");
        bg = Some([(v >> 16) as u8, (v >> 8) as u8, v as u8]);
        args.drain(i..=i + 1);
    }
    if args.len() < 3 {
        eprintln!("uso: brand-rasterize <svg> <png> <largura> [altura] [--bg #rrggbb]");
        std::process::exit(2);
    }
    let svg = std::fs::read_to_string(&args[0]).expect("svg ilegivel");
    let out = PathBuf::from(&args[1]);
    let w: u32 = args[2].parse().expect("largura");
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).expect("svg invalido");
    let size = tree.size();
    let h: u32 = match args.get(3) {
        Some(h) => h.parse().expect("altura"),
        None => ((w as f32 * size.height() / size.width()).round() as u32).max(1),
    };
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).expect("pixmap");
    if let Some(c) = bg {
        pixmap.fill(resvg::tiny_skia::Color::from_rgba8(c[0], c[1], c[2], 255));
    }
    let scale = (w as f32 / size.width()).min(h as f32 / size.height());
    let tx = (w as f32 - size.width() * scale) / 2.0;
    let ty = (h as f32 - size.height() * scale) / 2.0;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_translate(tx, ty).pre_scale(scale, scale), &mut pixmap.as_mut());
    // tiny-skia entrega premultiplicado; PNG e direto
    let mut data = pixmap.take();
    for p in data.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    image::save_buffer(&out, &data, w, h, image::ColorType::Rgba8).expect("png");
}
