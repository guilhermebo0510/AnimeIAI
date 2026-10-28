//! Motor de pincel raster por carimbos ("dabs"), inspirado no painel Brush Settings do Photoshop e no modelo de dab do
//! libmypaint (leitura de comportamento; implementação própria):
//!
//! - ponta procedural (diâmetro, dureza, redondeza, ângulo) ou por máscara de imagem;
//! - espaçamento em fração do diâmetro; dinâmicas de pressão → tamanho, opacidade, fluxo;
//! - dispersão, jitter de tamanho/ângulo/cor (RNG com semente → determinístico), ângulo seguindo a direção;
//! - textura de pincel em espaço de tela (multiplicar/subtrair, escala, profundidade) e pincel duplo;
//! - mistura com a cor da camada (borrar/misturar) e bordas molhadas (aquarela);
//! - duas cores no mesmo carimbo (cenário: luz + sombra da rosa, topo claro e base na sombra da nuvem, lado de
//!   sombra da folha), com a segunda cor derivada da cor do pincel ou escolhida;
//! - acumulação dentro do traço com fluxo, limitada pela opacidade do traço (como o Photoshop).
//!
//! O traço é acumulado num buffer RGBA pré-multiplicado e depois composto na camada com o modo do pincel.

use crate::blend::blend_pixel;
use crate::image::Image;
use crate::mask::Mask;
use crate::noise::{Rng, fbm, value_noise, worley};
use aiai_core::BlendMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrushTip {
    /// Ponta redonda/elíptica. `hardness` 0 = macia (queda cosseno desde o centro), 1 = borda dura.
    Round { hardness: f32, roundness: f32, angle: f32 },
    /// Máscara em escala de cinza (desenhada no app ou importada) guardada em blob ("AIM1"), esticada para o diâmetro.
    /// `mask` é carregada do blob por quem chama (aiai-tools) antes de pintar.
    /// Forma de cenário (folha, pétala, lâmina de grama, nuvem, estrela, quadrado) com proporção (largura/altura).
    Shape { shape: TipShape, #[serde(default = "d_one")] aspect: f32 },
    Image {
        blob: aiai_core::BlobHash,
        #[serde(skip)]
        #[schemars(skip)]
        mask: Option<Mask>,
    },
}

/// Formas de ponta para pincéis de cenário (folhagem, grama, flores, nuvens, brilhos).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TipShape {
    Leaf,
    Petal,
    GrassBlade,
    CloudPuff,
    Star,
    Square,
    /// Rosa em botão: disco com espiral de pétalas (vãos deixam ver o fundo escuro).
    Rose,
    /// Lateral de bastão (carvão): cheio de um lado e esmaecendo para o outro; com `follow_direction`, o lado cheio
    /// fica à esquerda do sentido do traço.
    Stick,
    /// Flor de cerejeira: cinco pétalas com o entalhe na ponta (S25, decoração).
    Sakura,
    /// Pena: haste no meio e barbas com fendas.
    Feather,
    /// Círculo de luz desfocada (bokeh): miolo claro e aro mais forte.
    Bokeh,
    /// Silhueta de árvore (conífera em camadas, com tronco).
    Tree,
    /// Folha de bordo caída: cinco pontas serrilhadas e o cabinho.
    Maple,
    /// Pedra/cascalho: polígono irregular com a face de baixo na sombra.
    Pebble,
    /// Cintilância: brilho de quatro raios finos com o miolo macio (purpurina, reflexo).
    Glint,
    // ---- 28/10: o que faltava das decorações do Clip Studio (linha com régua, roupas, fundo, efeito)
    /// Nota musical (colcheia): cabeça, haste e bandeirola (linha "Melodia").
    Note,
    /// Lombada de livro: retângulo com as faixas de cima e de baixo na segunda cor.
    Book,
    /// Prédio: silhueta com a grade de janelas na segunda cor.
    Building,
    /// Botão de roupa: disco com aro e quatro furos.
    Button,
    /// Conta (miçanga): esfera com sombra embaixo.
    Bead,
    /// Elo de corrente de frente (anel) e de lado (barra): em sequência formam a corrente.
    ChainLink,
    ChainLinkSide,
    /// Dentes do zíper, alternando de um lado e do outro.
    ZipperA,
    ZipperB,
    /// Gomos da trança, inclinados para um lado e para o outro.
    BraidL,
    BraidR,
    /// Renda: meia-lua com furos (barrado de renda).
    Scallop,
    /// Floreado: espiral de arabesco.
    Swirl,
    /// Onda: um período de senoide de ponta a ponta (linha ondulada contínua com o passo = diâmetro).
    Wave,
    /// Mancha: borrão irregular com respingos (mancha de sangue, de tinta).
    Splat,
}

/// Cobertura 0..1 de uma forma em coordenadas locais normalizadas (u, v ∈ [-1, 1]; v = -1 é a "ponta" superior).
/// `aa` = largura da borda antisserrilhada em unidades locais (≈ 1/raio_px).
pub fn shape_coverage(shape: TipShape, u: f32, v: f32, aa: f32) -> f32 {
    shape_alpha(shape, u, v, aa)
}

/// Como [`shape_coverage`], com a forma da ponta de duas cores (a rosa vira o botão inteiro): o contorno do cursor.
pub fn shape_coverage_tone(shape: TipShape, u: f32, v: f32, aa: f32, two_tone: bool) -> f32 {
    shape_alpha_tone(shape, u, v, aa, two_tone)
}

fn shape_alpha(shape: TipShape, u: f32, v: f32, aa: f32) -> f32 {
    shape_alpha_tone(shape, u, v, aa, false)
}

/// Raio e fase da espiral de pétalas da rosa (um vinco a cada π).
fn rose_phase(u: f32, v: f32) -> (f32, f32) {
    let r = (u * u + v * v).sqrt();
    let th = v.atan2(u);
    (r, th + 5.5 * r.max(0.05).ln())
}

/// Contorno com 5 lóbulos (as pétalas de fora) da rosa de duas cores.
fn rose_rim(u: f32, v: f32) -> f32 {
    0.86 + 0.07 * (5.0 * v.atan2(u) + 0.6).cos()
}

/// Cobertura da forma; com `two_tone` a rosa é o botão inteiro (os vincos viram sombra em vez de buraco).
fn shape_alpha_tone(shape: TipShape, u: f32, v: f32, aa: f32, two_tone: bool) -> f32 {
    let edge = |d: f32| (0.5 - d / aa.max(1e-4)).clamp(0.0, 1.0); // d = distância com sinal (aprox.) em unidades locais
    match shape {
        TipShape::Leaf => {
            if v.abs() > 1.0 { return 0.0 }
            let half = 0.42 * (1.0 - v * v).max(0.0).powf(0.75);
            edge(u.abs() - half)
        }
        TipShape::Petal => {
            if v.abs() > 1.0 { return 0.0 }
            let t = (v + 1.0) / 2.0; // 0 em cima
            let half = 0.55 * (t * (1.0 - t)).max(0.0).sqrt() * (0.6 + 0.8 * t) ;
            edge(u.abs() - half)
        }
        TipShape::GrassBlade => {
            if v.abs() > 1.0 { return 0.0 }
            let t = (v + 1.0) / 2.0;
            let bend = 0.25 * (1.0 - t) * (1.0 - t);
            let half = 0.12 * t.powf(0.7);
            edge((u - bend).abs() - half)
        }
        TipShape::CloudPuff => {
            // cúmulo: borda de cima definida, base que se desfaz (a borda alarga para baixo)
            let d = cloud_sdf(u, v);
            let soft = 0.5 * (v - 0.05).max(0.0);
            (0.5 - d / (aa.max(1e-4) + soft)).clamp(0.0, 1.0)
        }
        TipShape::Star => {
            let r = (u * u + v * v).sqrt();
            let a = v.atan2(u);
            let k = (a * 5.0 / 2.0).cos().abs().powf(8.0);
            let rad = 0.25 + 0.75 * k;
            edge(r - rad)
        }
        TipShape::Square => edge(u.abs().max(v.abs()) - 0.9),
        TipShape::Stick => {
            let r = (u * u + v * v).sqrt();
            // v = -1 é o lado cheio; esmaece até sumir no lado oposto
            let fade = (1.0 - smoothstep(-0.35, 1.0, v)).powf(0.8);
            edge(r - 0.95) * fade
        }
        TipShape::Rose if two_tone => {
            let (r, _) = rose_phase(u, v);
            if r > 1.0 { return 0.0 }
            edge(r - rose_rim(u, v))
        }
        TipShape::Sakura => {
            let r = (u * u + v * v).sqrt();
            // fase dentro de cada pétala (-0,5..0,5; 0 = meio da pétala, uma pétala para cima)
            let th = v.atan2(u) + std::f32::consts::FRAC_PI_2;
            let f = (th * 5.0 / std::f32::consts::TAU).rem_euclid(1.0) - 0.5;
            let notch = 0.24 * (1.0 - f.abs() / 0.07).max(0.0);
            let rad = 0.95 * (1.0 - 2.2 * f * f) - notch;
            edge(r - rad.max(0.2))
        }
        TipShape::Feather => {
            if v.abs() > 1.0 { return 0.0 }
            let t = (v + 1.0) / 2.0;
            let half = 0.36 * (std::f32::consts::PI * t).sin().max(0.0).powf(0.7);
            let shaft = edge(u.abs() - 0.035) * smoothstep(0.0, 0.1, t);
            // fendas das barbas: linhas inclinadas que abrem buracos finos fora da haste
            let k = (v * 9.0 + u.abs() * 5.0).rem_euclid(1.0);
            let slit = if u.abs() > 0.08 && k < 0.09 { 0.0 } else { 1.0 };
            (edge(u.abs() - half) * slit).max(shaft)
        }
        TipShape::Bokeh => {
            let r = (u * u + v * v).sqrt();
            edge(r - 0.9) * (0.35 + 0.65 * smoothstep(0.7, 0.88, r))
        }
        TipShape::Tree => {
            // copa: três andares de triângulo (base serrilhada), tronco embaixo
            let trunk = if v > 0.62 && v < 1.0 { edge(u.abs() - 0.07) } else { 0.0 };
            let t = ((v + 1.0) / 1.62).clamp(0.0, 1.0);
            let tier = (t * 3.0).fract();
            let half = 0.55 * (0.35 + 0.65 * t) * (0.45 + 0.55 * tier);
            let crown = if (-1.0..=0.62).contains(&v) { edge(u.abs() - half) } else { 0.0 };
            crown.max(trunk)
        }
        TipShape::Maple => {
            let r = (u * u + v * v).sqrt();
            let th = v.atan2(u) + std::f32::consts::FRAC_PI_2;
            let lobes = (th * 5.0 / 2.0).cos().abs().powf(3.0);
            let serr = 0.04 * (th * 23.0).cos();
            let leaf = edge(r - (0.38 + 0.55 * lobes + serr));
            // cabinho para baixo
            let stem = if v > 0.2 && v < 1.0 { edge(u.abs() - 0.03) } else { 0.0 };
            leaf.max(stem)
        }
        TipShape::Pebble => {
            let r = (u * u + v * v).sqrt();
            let th = v.atan2(u);
            let rad = 0.78 + 0.12 * (3.0 * th + 1.0).sin() + 0.07 * (5.0 * th + 2.0).sin();
            edge(r - rad)
        }
        TipShape::Glint => {
            let r = (u * u + v * v).sqrt();
            let th = v.atan2(u);
            let ray = (2.0 * th).cos().abs().powf(28.0);
            let core = (1.0 - r / 0.3).clamp(0.0, 1.0);
            (edge(r - (0.06 + 0.9 * ray)) * (1.0 - r * 0.6)).max(core)
        }
        TipShape::Note => {
            // colcheia: cabeça oval inclinada embaixo à esquerda, haste do lado direito, bandeirola para a direita
            let (hx, hy) = (u + 0.25, v - 0.6);
            let (c, s) = ((-0.35f32).cos(), (-0.35f32).sin());
            let (x, y) = (c * hx - s * hy, s * hx + c * hy);
            let head = (((x / 0.34).powi(2) + (y / 0.23).powi(2)).sqrt() - 1.0) * 0.23;
            let stem = ((u - 0.06).abs() - 0.055).max((v + 0.15).abs() - 0.72);
            let t = u - 0.06;
            let flag = if (0.0..=0.45).contains(&t) && v < 0.1 { (v - (-0.87 + 2.2 * t * t + 0.9 * t)).abs() - 0.08 } else { 1.0 };
            edge(head).max(edge(stem)).max(edge(flag))
        }
        TipShape::Book => edge((u.abs() - 0.34).max(v.abs() - 0.96)),
        TipShape::Building => {
            // topo com um degrau (caixa d'água) no lado esquerdo
            let body = (u.abs() - 0.62).max(v.abs() - 0.98);
            let top = (u + 0.3).abs().max(0.2) - 0.2;
            let tank = top.max((v + 1.0).abs() - 0.08);
            edge(body).max(edge(tank))
        }
        TipShape::Button => {
            let r = (u * u + v * v).sqrt();
            let hole = [(-0.22f32, -0.22f32), (0.22, -0.22), (-0.22, 0.22), (0.22, 0.22)].iter().map(|(x, y)| ((u - x).powi(2) + (v - y).powi(2)).sqrt() - 0.1).fold(f32::MAX, f32::min);
            edge(r - 0.9) * (1.0 - edge(hole))
        }
        TipShape::Bead => edge((u * u + v * v).sqrt() - 0.86),
        TipShape::ChainLink => {
            let outer = ((u / 0.95).powi(2) + (v / 0.55).powi(2)).sqrt() - 1.0;
            let inner = ((u / 0.62).powi(2) + (v / 0.24).powi(2)).sqrt() - 1.0;
            edge(outer * 0.55) * (1.0 - edge(inner * 0.24))
        }
        TipShape::ChainLinkSide => {
            // barra com as pontas redondas (o elo visto de lado)
            let x = (u.abs() - 0.8).max(0.0);
            edge((x * x + v * v).sqrt() - 0.14)
        }
        TipShape::ZipperA | TipShape::ZipperB => {
            let vv = if shape == TipShape::ZipperA { v } else { -v };
            // dente do lado de cima, com a ponta arredondada no meio
            let body = (u.abs() - 0.32).max((vv + 0.45).abs() - 0.5);
            let tip = (u * u + (vv - 0.05).powi(2)).sqrt() - 0.32;
            edge(body.min(tip.max(vv - 0.05)))
        }
        TipShape::BraidL | TipShape::BraidR => {
            let a: f32 = if shape == TipShape::BraidL { 0.6 } else { -0.6 };
            let (c, s) = (a.cos(), a.sin());
            let (x, y) = (c * u - s * v, s * u + c * v);
            edge((((x / 0.95).powi(2) + (y / 0.42).powi(2)).sqrt() - 1.0) * 0.42)
        }
        TipShape::Scallop => {
            // meia-lua para baixo de uma faixa reta, com um arco de furos
            let (x, y) = (u, v + 0.25);
            let r = (x * x + y * y).sqrt();
            let half = if y >= 0.0 { r - 0.92 } else { 1.0 };
            let band = (u.abs() - 0.98).max((v + 0.32).abs() - 0.1);
            let th = y.atan2(x);
            let holes = if y > 0.05 {
                let k = ((th / std::f32::consts::PI) * 5.0).round() / 5.0 * std::f32::consts::PI;
                ((x - 0.62 * k.cos()).powi(2) + (y - 0.62 * k.sin()).powi(2)).sqrt() - 0.09
            } else {
                1.0
            };
            edge(half.min(band)) * (1.0 - edge(holes))
        }
        TipShape::Swirl => {
            // espiral de Arquimedes r = 0,08 + 0,13·θ, duas voltas e meia, linha de 0,07
            let r = (u * u + v * v).sqrt();
            let th = v.atan2(u).rem_euclid(std::f32::consts::TAU);
            let mut d = f32::MAX;
            for n in 0..3 {
                let t = th + std::f32::consts::TAU * n as f32;
                if t <= 15.5 {
                    d = d.min((r - (0.08 + 0.055 * t)).abs());
                }
            }
            edge(d - 0.07)
        }
        TipShape::Wave => {
            if u.abs() > 1.0 {
                return 0.0;
            }
            let pi = std::f32::consts::PI;
            let slope = 0.45 * pi * (pi * u).cos();
            edge(((v - 0.45 * (pi * u).sin()).abs() / (1.0 + slope * slope).sqrt()) - 0.1)
        }
        TipShape::Splat => {
            let r = (u * u + v * v).sqrt();
            let th = v.atan2(u);
            let rad = 0.5 + 0.13 * (5.0 * th).sin() + 0.07 * (11.0 * th + 1.3).sin();
            let drops = [(0.78f32, 0.22f32, 0.1f32), (-0.62, 0.66, 0.08), (0.2, -0.86, 0.07), (-0.8, -0.35, 0.05)]
                .iter()
                .map(|(x, y, s)| ((u - x).powi(2) + (v - y).powi(2)).sqrt() - s)
                .fold(f32::MAX, f32::min);
            edge(r - rad).max(edge(drops))
        }
        TipShape::Rose => {
            let r = (u * u + v * v).sqrt();
            if r > 1.0 { return 0.0 }
            let th = v.atan2(u);
            // espiral logarítmica de pétalas: vão fino onde sin(θ + k·ln r) ≈ 0
            let phase = th + 9.0 * (r.max(0.05)).ln();
            let gap = (phase * 1.5).sin().abs();
            let outer = edge(r - 0.92);
            let inner = ((gap - 0.12) / 0.1).clamp(0.0, 1.0);
            outer * if r < 0.12 { 1.0 } else { inner.max(0.15) }
        }
    }
}

fn cloud_sdf(u: f32, v: f32) -> f32 {
    let c = |cx: f32, cy: f32, r: f32| ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() - r;
    c(0.0, -0.1, 0.55).min(c(-0.45, 0.2, 0.42)).min(c(0.45, 0.2, 0.45)).min(c(0.0, 0.35, 0.5))
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Quanto da segunda cor (sombra) vai no ponto da forma: 0 = cor do pincel, 1 = segunda cor. `aa` = 1 px em
/// unidades locais: carimbo pequeno perde o detalhe fino (senão a espiral vira pontilhado).
fn shape_tone(shape: TipShape, u: f32, v: f32, aa: f32) -> f32 {
    match shape {
        TipShape::Rose => {
            let (r, phase) = rose_phase(u, v);
            // a pétala de cima faz sombra na de baixo logo depois de cada vinco (meia-lua); a beirada da pétala pega
            // um fio de luz; o miolo fechado é o mais escuro e as pétalas de fora, as mais claras
            let p = (phase / std::f32::consts::PI).rem_euclid(1.0);
            let crescent = (1.0 - p / 0.55).clamp(0.0, 1.0).powf(1.3) * smoothstep(0.0, 0.08, p) * 0.85;
            let core = (1.0 - r / 0.38).clamp(0.0, 1.0);
            let detail = smoothstep(0.17, 0.06, aa);
            let simple = (1.0 - r / 0.8).clamp(0.0, 1.0) * 0.8;
            let rim = smoothstep(0.62, 0.86, r) * 0.6;
            (simple + (crescent.max(core) - simple) * detail - rim).clamp(0.0, 1.0)
        }
        // topo iluminado, base dos lóbulos na sombra
        TipShape::CloudPuff => smoothstep(-0.2, 0.62, v),
        // metade da folha do lado da sombra, nervura no meio
        TipShape::Leaf | TipShape::Petal => smoothstep(-0.04, 0.08, u) * 0.85,
        // base da lâmina escura, ponta clara
        TipShape::GrassBlade => smoothstep(-0.3, 0.95, v),
        // miolo rosado da flor (meio mais escuro)
        TipShape::Sakura => (1.0 - (u * u + v * v).sqrt() / 0.35).clamp(0.0, 1.0) * 0.8,
        // nervuras da folha caída e a haste da pena
        TipShape::Maple => smoothstep(0.08, 0.0, u.abs().min(v.abs())) * 0.6,
        TipShape::Feather => smoothstep(0.06, 0.0, u.abs()) * 0.7,
        // face de baixo da pedra na sombra
        TipShape::Pebble => smoothstep(-0.1, 0.7, v + 0.3 * u),
        // faixas da lombada do livro
        TipShape::Book => if (v.abs() - 0.72).abs() < 0.07 { 1.0 } else { 0.0 },
        // janelas do prédio (grade)
        TipShape::Building => {
            let (gx, gy) = ((u + 0.62) / 0.31, (v + 0.82) / 0.27);
            if u.abs() < 0.5 && v > -0.82 && v < 0.82 && gx.fract() > 0.3 && gx.fract() < 0.75 && gy.fract() > 0.3 && gy.fract() < 0.78 { 1.0 } else { 0.0 }
        }
        // aro do botão e sombra da conta
        TipShape::Button => smoothstep(0.62, 0.72, (u * u + v * v).sqrt()),
        TipShape::Bead => smoothstep(-0.5, 0.9, u + v) * 0.8,
        TipShape::Star | TipShape::Square | TipShape::Stick | TipShape::Bokeh | TipShape::Tree | TipShape::Glint => 0.0,
        TipShape::Note | TipShape::ChainLink | TipShape::ChainLinkSide | TipShape::ZipperA | TipShape::ZipperB | TipShape::BraidL | TipShape::BraidR => 0.0,
        TipShape::Scallop | TipShape::Swirl | TipShape::Wave | TipShape::Splat => 0.0,
    }
}

/// Duas cores no mesmo carimbo (pincéis de cenário). A segunda cor é `color`, ou a cor do pincel multiplicada por
/// `tint` na proporção `amount` (vale para qualquer cor: rosa vermelha → vinho, nuvem branca → cinza-azulado).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TwoTone {
    #[serde(default = "d_tint")]
    pub tint: [f32; 3],
    /// 0..1
    #[serde(default = "d_one")]
    pub amount: f32,
    /// Segunda cor fixa (sRGB); `None` = derivada da cor do pincel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
}

fn d_tint() -> [f32; 3] {
    [0.55, 0.55, 0.7]
}

impl TwoTone {
    pub fn second(&self, rgb: [f32; 3]) -> [f32; 3] {
        match self.color {
            Some(c) => c,
            None => [0, 1, 2].map(|i| rgb[i] * (1.0 - self.amount + self.amount * self.tint[i])),
        }
    }
}

impl Default for BrushTip {
    fn default() -> Self {
        BrushTip::Round { hardness: 0.8, roundness: 1.0, angle: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextureKind {
    #[default]
    Paper,
    Canvas,
    Noise,
    Cells,
    Grain,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextureMode {
    #[default]
    Multiply,
    Subtract,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BrushTexture {
    pub kind: TextureKind,
    /// Tamanho do grão em px.
    pub scale: f32,
    /// 0..1
    pub depth: f32,
    #[serde(default)]
    pub mode: TextureMode,
    #[serde(default)]
    pub seed: u32,
    /// Inverte a textura (os vales viram picos).
    #[serde(default)]
    pub invert: bool,
    /// Rotação da textura no canvas (graus).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f32,
    /// Textura de imagem (escala de cinza, blob "AIM1"; importada ou feita pelo usuário) no lugar da procedural:
    /// repetida pelo canvas, `scale` = zoom (1 = um pixel da imagem por pixel do canvas).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<aiai_core::BlobHash>,
    /// A máscara de `image`, carregada do blob por quem chama (aiai-tools) antes de pintar.
    #[serde(skip)]
    #[schemars(skip)]
    pub image_mask: Option<std::sync::Arc<crate::mask::Mask>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DualBrush {
    pub size: f32,
    pub spacing: f32,
    pub scatter: f32,
    pub hardness: f32,
}

/// Autor de um pincel (vai no manifesto do `.aiaibrush`; tudo opcional).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BrushAuthor {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
}

impl BrushAuthor {
    pub fn is_empty(&self) -> bool {
        self.name.trim().is_empty() && self.email.trim().is_empty() && self.url.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BrushPreset {
    pub name: String,
    /// Categoria na biblioteca (lapis, pintura, aquarela, textura, cenario, borracha ou uma do usuário).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    /// De onde veio (pincel importado: arquivo e pasta dentro dele); a licença é a do autor do pacote.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// Identificador estável (UUID) do pincel na biblioteca e nos pacotes; vazio = pincel sem arquivo próprio.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "BrushAuthor::is_empty")]
    pub author: BrushAuthor,
    /// Licença: um identificador SPDX (CC-BY-4.0, CC0-1.0, MIT...) ou o texto livre da licença.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    /// Versão do pincel (1.0.0), para a biblioteca saber qual cópia é mais nova.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// Criado/alterado em (ISO 8601, UTC).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub modified: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub tip: BrushTip,
    /// Diâmetro em px com pressão máxima.
    pub size: f32,
    /// Distância entre dabs em fração do diâmetro (0,02..2).
    #[serde(default = "d_spacing")]
    pub spacing: f32,
    /// Opacidade máxima do traço (0..1).
    #[serde(default = "d_one")]
    pub opacity: f32,
    /// Quanto cada dab deposita (0..1).
    #[serde(default = "d_one")]
    pub flow: f32,
    /// Tamanho mínimo com pressão 0 (fração do tamanho). 1 = pressão não muda o tamanho.
    #[serde(default = "d_size_min")]
    pub size_pressure_min: f32,
    /// Opacidade (do dab) mínima com pressão 0.
    #[serde(default = "d_one")]
    pub opacity_pressure_min: f32,
    #[serde(default = "d_one")]
    pub flow_pressure_min: f32,
    /// Deslocamento aleatório perpendicular em fração do diâmetro.
    #[serde(default)]
    pub scatter: f32,
    /// Dabs por passo (com dispersão > 0 cria nuvens de pinceladas).
    #[serde(default = "d_count")]
    pub count: u32,
    #[serde(default)]
    pub size_jitter: f32,
    /// Graus.
    #[serde(default)]
    pub angle_jitter: f32,
    /// A ponta gira seguindo a direção do traço.
    #[serde(default)]
    pub follow_direction: bool,
    /// A ponta gira para o lado em que a caneta está deitada (azimute da inclinação; Clip Studio: "Direção da caneta").
    /// Sem inclinação no ponto, vale a direção do traço (com `follow_direction`) ou 0.
    #[serde(default)]
    pub follow_tilt: bool,
    /// Jitter de matiz (0..1 da volta), saturação e valor (0..1).
    #[serde(default)]
    pub hue_jitter: f32,
    #[serde(default)]
    pub saturation_jitter: f32,
    #[serde(default)]
    pub value_jitter: f32,
    #[serde(default)]
    pub texture: Option<BrushTexture>,
    #[serde(default)]
    pub dual: Option<DualBrush>,
    /// 0 = só a cor do pincel; 1 = só a cor que já está na camada (borrar).
    #[serde(default)]
    pub mix: f32,
    /// Quanto a cor misturada "arrasta" de um dab para o próximo (borrar).
    #[serde(default)]
    pub smudge_carry: f32,
    /// Bordas molhadas (aquarela): 0..1.
    #[serde(default)]
    pub wet_edges: f32,
    #[serde(default)]
    pub mode: BrushMode,
    /// Sem antisserrilhado (lápis de pixel / linha de cel raster).
    #[serde(default)]
    pub aliased: bool,
    /// Duas cores no carimbo (só pontas de forma: rosa, nuvem, folha, pétala, grama).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub two_tone: Option<TwoTone>,
    /// Várias pontas (Multi-Tip, auditoria 4.9d): além de `tip`, estas; cada carimbo usa uma delas (sorteada ou em
    /// sequência, `tip_order`). Ex.: folhas de formatos diferentes num pincel de folhagem.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_tips: Vec<BrushTip>,
    #[serde(default, skip_serializing_if = "is_default_order")]
    pub tip_order: TipOrder,
    /// Fita (Ribbon, auditoria 4.9d): com ponta de imagem, a imagem corre ao longo do traço (repete a cada comprimento
    /// da própria imagem na escala do diâmetro) em vez de carimbar cópias: corrente, renda, fita, trilho.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ribbon: bool,
    /// Cor em degradê AO LONGO DO TRAÇO: no lugar da cor do pincel, cada dab pega a cor do degradê na distância já
    /// percorrida (`length` px = uma passada de 0 a 1; `extend` do degradê decide o que vem depois: prender, repetir,
    /// espelhar). As cores de paleta já chegam resolvidas (o pincel não enxerga o projeto; paleta = magenta).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_ramp: Option<ColorRamp>,
    /// Grupo de ferramenta (abas da prateleira, como no Clip Studio): `caneta`, `marcador`, `lapis`, `pincel`,
    /// `aerografo`, `decoracao`, `borracha`, `mistura` (ou vazio = sem grupo: pincel antigo do usuário).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Suavização de serrilhado em 4 níveis (0 sem, 1 fraca, 2 média, 3 forte, como no Clip Studio); `None` = a borda de
    /// ~1 px de sempre. Com nível, a ponta redonda usa a distância real à borda da elipse (ponta chata fica nítida).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub antialias: Option<u8>,
    /// Dinâmicas (pressão/inclinação/velocidade/aleatório com mínimo e curva). Quando presentes, substituem o
    /// `*_pressure_min` do mesmo parâmetro.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_dyn: Option<crate::brush_dyn::Dynamics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_dyn: Option<crate::brush_dyn::Dynamics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity_dyn: Option<crate::brush_dyn::Dynamics>,
    /// Espessura da ponta (achatamento da redonda: multiplica `roundness`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness_dyn: Option<crate::brush_dyn::Dynamics>,
    /// Velocidade (px/s) que conta como 100% nas dinâmicas e no afinamento pela velocidade; 0 = 2500.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub velocity_full: f32,
    /// Entrada e saída (afinamento das pontas do traço).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<crate::brush_dyn::Taper>,
    /// Estabilização do traço deste pincel (0..100, "pela velocidade"); quem desenha à mão usa no lugar da do painel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stabilization: Option<crate::brush_dyn::Stabilization>,
    /// Borda áspera da ponta redonda (canetas texturizada, rugosa, G real).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<crate::brush_dyn::EdgeNoise>,
    /// Tinta pelo máximo: dentro do traço, cada pixel fica com o MAIOR alfa dos carimbos que o tocaram, em vez de somar
    /// camadas (canetas: a borda tem o perfil da ponta e não engrossa nem endurece com a sobreposição dos carimbos).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub max_ink: bool,
    /// Grão de papel em cada carimbo (lápis, carvão, giz, pastel, ruído).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<crate::brush_dyn::Grain>,
    /// Spray: o carimbo vira partículas espalhadas (aerógrafo).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spray: Option<crate::brush_dyn::Spray>,
    /// Tinta que mistura com a camada (aquarela que mistura, guache, óleo, misturadores e Desfocar).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paint_mix: Option<crate::brush_dyn::PaintMix>,
    /// Borracha numa camada vetorial: o que sai do traço tocado (trecho, até as interseções, traço inteiro); `None` =
    /// o modo escolhido na ferramenta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_erase: Option<VectorErase>,
    /// Borracha que apaga em todas as camadas do quadro (visíveis e destravadas), não só na ativa.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_layers: bool,
    /// Encaixa na régua ativa (a borracha só encaixa quando o pincel pede; caneta e pincel sempre encaixam).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snap: bool,
    /// Tamanho na tela (Clip Studio): `size` vale em px da tela e quem pinta divide pelo zoom da vista.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub screen_size: bool,
    /// Pelo menos 1 px: a dinâmica de tamanho, o aleatório e o afinamento não deixam o carimbo menor que 1 px.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub min_one_px: bool,
    /// Variação de cor para a secundária (0..1): cada carimbo puxa a cor, um tanto sorteado até esse valor, para
    /// `sub_color` (como "Misturar com a cor secundária" do Clip Studio).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sub_jitter: f32,
    /// A cor secundária, resolvida por quem pinta (como a rampa); sem ela, `sub_jitter` não faz nada.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_color: Option<[f32; 3]>,
    /// As variações de cor são sorteadas uma vez por traço (o traço inteiro numa cor) em vez de a cada carimbo.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub jitter_per_stroke: bool,
    /// Densidade pelo intervalo: com intervalo maior, cada carimbo fica mais denso (o traço mantém o tom do
    /// intervalo de 10%); com menor, mais ralo.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flow_by_spacing: bool,
    /// Borda de aquarela: quanto a cor escurece na borda (0..1).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wet_darkness: f32,
    /// Largura da borda de aquarela (px); 0 = automática (12% do tamanho, de 1 a 12 px).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wet_width: f32,
}

/// O que a borracha tira de um traço vetorial que ela toca.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VectorErase {
    /// Só o trecho coberto.
    Segment,
    /// Até as interseções com outros traços.
    Intersection,
    /// O traço inteiro.
    Whole,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// Velocidade padrão (px/s) que conta como 100%.
pub const VELOCITY_FULL_DEFAULT: f32 = 2500.0;

/// Degradê do pincel ao longo do comprimento do traço.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColorRamp {
    pub gradient: aiai_core::Gradient,
    /// Px do caminho para ir de 0 a 1.
    #[serde(default = "d_ramp_len")]
    pub length: f32,
}

fn d_ramp_len() -> f32 {
    400.0
}

impl ColorRamp {
    fn resolved(&self) -> aiai_core::ResolvedGradient {
        self.gradient.resolve(|c| match c {
            aiai_core::ColorRef::Direct { rgba } => rgba.0,
            aiai_core::ColorRef::Palette { .. } => [1.0, 0.0, 1.0, 1.0],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrushMode {
    #[default]
    Normal,
    Erase,
    Behind,
    Multiply,
    Screen,
    Overlay,
    Add,
    Darken,
    Lighten,
    SoftLight,
    HardLight,
    Subtract,
    Difference,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

fn d_spacing() -> f32 {
    0.12
}

/// Como o carimbo escolhe entre `tip` e `extra_tips`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TipOrder {
    #[default]
    Random,
    Sequence,
}

fn is_default_order(o: &TipOrder) -> bool {
    *o == TipOrder::Random
}
fn d_one() -> f32 {
    1.0
}
fn d_size_min() -> f32 {
    0.2
}
fn d_count() -> u32 {
    1
}

impl BrushPreset {
    pub fn round(name: &str, size: f32, hardness: f32) -> Self {
        BrushPreset {
            name: name.into(),
            category: String::new(),
            source: String::new(),
            id: String::new(),
            author: BrushAuthor::default(),
            license: String::new(),
            version: String::new(),
            created: String::new(),
            modified: String::new(),
            tags: Vec::new(),
            tip: BrushTip::Round { hardness, roundness: 1.0, angle: 0.0 },
            size,
            spacing: d_spacing(),
            opacity: 1.0,
            flow: 1.0,
            size_pressure_min: 0.2,
            opacity_pressure_min: 1.0,
            flow_pressure_min: 1.0,
            scatter: 0.0,
            count: 1,
            size_jitter: 0.0,
            angle_jitter: 0.0,
            follow_direction: false,
            follow_tilt: false,
            hue_jitter: 0.0,
            saturation_jitter: 0.0,
            value_jitter: 0.0,
            texture: None,
            dual: None,
            mix: 0.0,
            smudge_carry: 0.0,
            wet_edges: 0.0,
            mode: BrushMode::Normal,
            aliased: false,
            two_tone: None,
            extra_tips: Vec::new(),
            tip_order: TipOrder::Random,
            ribbon: false,
            color_ramp: None,
            group: String::new(),
            antialias: None,
            size_dyn: None,
            flow_dyn: None,
            opacity_dyn: None,
            thickness_dyn: None,
            velocity_full: 0.0,
            taper: None,
            stabilization: None,
            edge: None,
            max_ink: false,
            grain: None,
            spray: None,
            paint_mix: None,
            vector_erase: None,
            all_layers: false,
            snap: false,
            screen_size: false,
            min_one_px: false,
            sub_jitter: 0.0,
            sub_color: None,
            jitter_per_stroke: false,
            flow_by_spacing: false,
            wet_darkness: 0.0,
            wet_width: 0.0,
        }
    }

    /// O traço lê a camada de antes dele (borrador, tinta que mistura, misturadores, Desfocar): quem pinta precisa
    /// passar a camada inteira, não só o recorte tocado.
    /// "Forma do pincel" do Clip Studio: põe neste pincel a forma de `from` (ponta e pontas extras, spray, traço,
    /// textura e grão, pincel duplo, borda de aquarela, borda áspera), deixando tamanho, tinta, cor, dinâmicas,
    /// correção e início/fim como estão.
    pub fn apply_shape(&mut self, from: &BrushPreset) {
        self.tip = from.tip.clone();
        self.extra_tips = from.extra_tips.clone();
        self.tip_order = from.tip_order;
        self.ribbon = from.ribbon;
        self.two_tone = from.two_tone.clone();
        self.follow_direction = from.follow_direction;
        self.follow_tilt = from.follow_tilt;
        self.angle_jitter = from.angle_jitter;
        self.size_jitter = from.size_jitter;
        self.scatter = from.scatter;
        self.count = from.count;
        self.spacing = from.spacing;
        self.flow_by_spacing = from.flow_by_spacing;
        self.max_ink = from.max_ink;
        self.spray = from.spray.clone();
        self.texture = from.texture.clone();
        self.grain = from.grain.clone();
        self.dual = from.dual.clone();
        self.wet_edges = from.wet_edges;
        self.wet_darkness = from.wet_darkness;
        self.wet_width = from.wet_width;
        self.edge = from.edge.clone();
        self.thickness_dyn = from.thickness_dyn.clone();
    }

    /// Nomes de todos os campos do pincel (como no JSON), para quem edita um pincel por campos (ponte/MCP).
    pub fn field_names() -> Vec<String> {
        let schema = serde_json::to_value(schemars::schema_for!(BrushPreset)).unwrap_or_default();
        let props = schema.get("properties").or_else(|| schema.pointer("/definitions/BrushPreset/properties"));
        props.and_then(|p| p.as_object()).map(|o| o.keys().cloned().collect()).unwrap_or_default()
    }

    pub fn reads_layer(&self) -> bool {
        self.mix > 0.0 || self.paint_mix.is_some()
    }

    /// Pixels por segundo que contam como velocidade 100%.
    pub fn velocity_full(&self) -> f32 {
        if self.velocity_full > 0.0 { self.velocity_full } else { VELOCITY_FULL_DEFAULT }
    }

    /// Caneta de pixel: 1 px sem antisserrilhado, linha sem degraus duplos (pixel-perfect).
    pub fn is_pixel_pen(&self) -> bool {
        (self.aliased || self.antialias == Some(0)) && self.size <= 1.0 && matches!(self.tip, BrushTip::Round { .. }) && self.mode != BrushMode::Erase
    }

    pub fn validate(&self) -> Result<(), String> {
        let unit = |v: f32, n: &str| if (0.0..=1.0).contains(&v) { Ok(()) } else { Err(format!("{n} fora de 0..1: {v}")) };
        if !(self.size.is_finite() && self.size > 0.0 && self.size <= 2000.0) {
            return Err(format!("tamanho inválido {}", self.size));
        }
        if !(0.01..=5.0).contains(&self.spacing) {
            return Err(format!("espaçamento fora de 0,01..5: {}", self.spacing));
        }
        unit(self.opacity, "opacity")?;
        unit(self.flow, "flow")?;
        unit(self.size_pressure_min, "size_pressure_min")?;
        unit(self.opacity_pressure_min, "opacity_pressure_min")?;
        unit(self.flow_pressure_min, "flow_pressure_min")?;
        unit(self.mix, "mix")?;
        unit(self.smudge_carry, "smudge_carry")?;
        unit(self.wet_edges, "wet_edges")?;
        unit(self.wet_darkness, "wet_darkness")?;
        unit(self.sub_jitter, "sub_jitter")?;
        if !(self.wet_width.is_finite() && (0.0..=64.0).contains(&self.wet_width)) {
            return Err(format!("wet_width fora de 0..64 px: {}", self.wet_width));
        }
        if self.sub_color.is_some_and(|c| c.iter().any(|v| !(0.0..=1.0).contains(v))) {
            return Err("sub_color fora de 0..1".into());
        }
        if let Some(t) = &self.two_tone {
            unit(t.amount, "two_tone.amount")?;
            if t.tint.iter().chain(t.color.iter().flatten()).any(|c| !(0.0..=1.0).contains(c)) {
                return Err("two_tone: cores fora de 0..1".into());
            }
        }
        if self.count == 0 || self.count > 64 {
            return Err("count deve estar entre 1 e 64".into());
        }
        if self.antialias.is_some_and(|a| a > 3) {
            return Err("antialias: 0 (sem), 1 (fraca), 2 (média) ou 3 (forte)".into());
        }
        for (n, d) in [("size_dyn", &self.size_dyn), ("flow_dyn", &self.flow_dyn), ("opacity_dyn", &self.opacity_dyn), ("thickness_dyn", &self.thickness_dyn)] {
            if let Some(d) = d {
                d.validate(n)?;
            }
        }
        if !(self.velocity_full.is_finite() && (0.0..=100_000.0).contains(&self.velocity_full)) {
            return Err(format!("velocity_full fora de 0..100000 px/s: {}", self.velocity_full));
        }
        if let Some(t) = &self.taper {
            t.validate()?;
        }
        if self.stabilization.is_some_and(|s| s.level > 100) {
            return Err("stabilization.level fora de 0..100".into());
        }
        if let Some(e) = &self.edge {
            e.validate()?;
        }
        if let Some(g) = &self.grain {
            g.validate()?;
        }
        if let Some(pm) = &self.paint_mix {
            pm.validate()?;
        }
        if let Some(sp) = &self.spray {
            sp.validate()?;
        }
        Ok(())
    }

    /// Borracha redonda padrão (30 px, dureza 0,9): a base da ferramenta Borracha raster. Não faz parte de `defaults()`
    /// (a biblioteca de pincéis não lista borrachas: a ferramenta Borracha cobre); o nome "Borracha" ainda vale no PAINT
    /// da bridge, e os traços já gravados guardam o preset dentro deles.
    pub fn eraser() -> BrushPreset {
        let mut p = BrushPreset::round("Borracha", 30.0, 0.9);
        p.mode = BrushMode::Erase;
        p
    }

    /// Presets iniciais do programa (S06).
    pub fn defaults() -> Vec<BrushPreset> {
        let mut v = Vec::new();
        let mut pencil = BrushPreset::round("Lápis de rascunho", 3.0, 0.9);
        pencil.opacity_pressure_min = 0.3;
        pencil.texture = Some(BrushTexture { kind: TextureKind::Grain, scale: 1.5, depth: 0.6, mode: TextureMode::Multiply, seed: 3, invert: false, rotation: 0.0, image: None, image_mask: None });
        v.push(pencil);
        v.push(BrushPreset::round("Redondo duro", 24.0, 0.95));
        let mut soft = BrushPreset::round("Redondo macio", 60.0, 0.0);
        soft.flow = 0.35;
        soft.size_pressure_min = 0.6;
        v.push(soft);
        let mut air = BrushPreset::round("Aerógrafo", 120.0, 0.0);
        air.flow = 0.08;
        air.opacity = 0.8;
        air.size_pressure_min = 1.0;
        air.spacing = 0.05;
        v.push(air);
        let mut wc = BrushPreset::round("Aquarela", 50.0, 0.5);
        wc.flow = 0.5;
        wc.opacity = 0.7;
        wc.wet_edges = 0.7;
        wc.mix = 0.15;
        wc.texture = Some(BrushTexture { kind: TextureKind::Paper, scale: 6.0, depth: 0.35, mode: TextureMode::Multiply, seed: 7, invert: false, rotation: 0.0, image: None, image_mask: None });
        v.push(wc);
        // o "Guache seco" antigo (S06) deu lugar ao do grupo pincel (S25), com o mesmo nome
        // o "Carvão" antigo (S06) deu lugar ao do grupo lápis (S25, lateral do bastão), com o mesmo nome
        let mut paper = BrushPreset::round("Textura de papel", 80.0, 0.3);
        paper.flow = 0.4;
        paper.texture = Some(BrushTexture { kind: TextureKind::Paper, scale: 10.0, depth: 0.9, mode: TextureMode::Multiply, seed: 13, invert: false, rotation: 0.0, image: None, image_mask: None });
        v.push(paper);
        let mut smudge = BrushPreset::round("Borrador", 30.0, 0.4);
        smudge.mix = 0.95;
        smudge.smudge_carry = 0.8;
        smudge.flow = 0.6;
        v.push(smudge);
        // pincéis de cenário (S11)
        let mut foliage = BrushPreset::round("Folhagem", 26.0, 1.0);
        foliage.tip = BrushTip::Shape { shape: TipShape::Leaf, aspect: 0.8 };
        foliage.spacing = 0.35;
        foliage.scatter = 1.2;
        foliage.count = 3;
        foliage.size_jitter = 0.4;
        foliage.angle_jitter = 180.0;
        foliage.value_jitter = 0.12;
        foliage.hue_jitter = 0.04;
        foliage.size_pressure_min = 0.6;
        foliage.two_tone = Some(TwoTone { tint: [0.45, 0.6, 0.62], amount: 0.7, color: None });
        v.push(foliage);
        let mut grass = BrushPreset::round("Grama", 30.0, 1.0);
        grass.tip = BrushTip::Shape { shape: TipShape::GrassBlade, aspect: 1.0 };
        grass.spacing = 0.12;
        grass.scatter = 0.3;
        grass.count = 2;
        grass.size_jitter = 0.35;
        grass.angle_jitter = 12.0;
        grass.value_jitter = 0.15;
        grass.two_tone = Some(TwoTone { tint: [0.4, 0.58, 0.5], amount: 0.65, color: None });
        v.push(grass);
        let mut rose = BrushPreset::round("Rosas", 18.0, 1.0);
        rose.tip = BrushTip::Shape { shape: TipShape::Rose, aspect: 1.0 };
        rose.spacing = 0.9;
        rose.scatter = 0.8;
        rose.count = 2;
        rose.size_jitter = 0.35;
        rose.angle_jitter = 180.0;
        rose.value_jitter = 0.1;
        // luz nas pétalas de fora, sombra vinho nos vincos e no miolo
        rose.two_tone = Some(TwoTone { tint: [0.42, 0.16, 0.3], amount: 0.85, color: None });
        v.push(rose);
        let mut petals = BrushPreset::round("Flores/pétalas", 14.0, 1.0);
        petals.tip = BrushTip::Shape { shape: TipShape::Petal, aspect: 0.8 };
        petals.spacing = 0.5;
        petals.scatter = 1.5;
        petals.count = 3;
        petals.size_jitter = 0.4;
        petals.angle_jitter = 180.0;
        petals.saturation_jitter = 0.1;
        petals.value_jitter = 0.12;
        v.push(petals);
        let mut cloud = BrushPreset::round("Nuvem", 90.0, 1.0);
        cloud.tip = BrushTip::Shape { shape: TipShape::CloudPuff, aspect: 1.2 };
        cloud.spacing = 0.25;
        cloud.scatter = 0.25;
        cloud.size_jitter = 0.3;
        // fluxo cheio: carimbos semitransparentes empilhados faziam escamas dentro da nuvem; a transparência fica
        // na opacidade do traço, e o volume vem da base de cada lóbulo na sombra
        cloud.flow = 1.0;
        cloud.opacity = 0.95;
        cloud.two_tone = Some(TwoTone { tint: [0.66, 0.72, 0.88], amount: 1.0, color: None });
        v.push(cloud);
        let mut sparkle = BrushPreset::round("Brilhos", 18.0, 1.0);
        sparkle.tip = BrushTip::Shape { shape: TipShape::Star, aspect: 1.0 };
        sparkle.spacing = 1.5;
        sparkle.scatter = 2.0;
        sparkle.size_jitter = 0.6;
        sparkle.angle_jitter = 20.0;
        v.push(sparkle);
        // sem "Borracha" na biblioteca de fábrica: a ferramenta Borracha cobre (ver `BrushPreset::eraser`)
        for p in &mut v {
            p.category = match p.name.as_str() {
                "Lápis de rascunho" => "lapis",
                "Redondo duro" | "Redondo macio" | "Aerógrafo" | "Guache seco" => "pintura",
                "Aquarela" | "Borrador" => "aquarela",
                "Textura de papel" => "textura",
                _ => "cenario",
            }
            .into();
            p.group = match p.name.as_str() {
                "Lápis de rascunho" => "lapis",
                "Aerógrafo" => "aerografo",
                "Borrador" => "mistura",
                "Redondo duro" | "Redondo macio" | "Aquarela" | "Textura de papel" => "pincel",
                _ => "decoracao",
            }
            .into();
        }
        v.extend(Self::pens());
        v.extend(Self::pencils());
        v.extend(Self::airbrushes());
        v.extend(Self::erasers());
        v.extend(Self::paint_brushes());
        v.extend(Self::blenders());
        v.extend(Self::decorations());
        for p in &mut v {
            Self::s25_decoration_tuning(p);
        }
        v
    }

    /// Ajuste S25 da decoração (calibrada nas decorações do Clip Studio em 30/60 px): os elementos caem como partículas do
    /// spray (forma da ponta, tamanho em fração do pincel, quantidade, espalhamento), menores e mais espaçados que a
    /// ponta inteira; vale também para os pincéis de cenário antigos (S11) que têm um equivalente lá.
    fn s25_decoration_tuning(p: &mut BrushPreset) {
        use crate::brush_dyn::Spray;
        let spray = |count: f32, size: f32, size_min: f32, spread: f32| Some(Spray { count, size, sync: true, size_min, roundness_min: 1.0, bias: 0.0, count_exp: 0.0, spread, align: false });
        match p.name.as_str() {
            "Brilhos" => {
                p.spray = spray(1.5, 0.3, 0.6, 1.0);
                p.spacing = 0.6;
                p.scatter = 0.0;
                p.count = 1;
                p.tip = BrushTip::Shape { shape: TipShape::Glint, aspect: 1.0 };
            }
            "Purpurina" => {
                p.spray = spray(6.0, 0.12, 0.3, 1.0);
                p.spacing = 0.2;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Círculos suaves" => {
                p.spray = spray(2.0, 0.3, 0.6, 1.2);
                p.spacing = 0.6;
                p.flow = 0.3;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Penas" => {
                p.spray = spray(2.0, 0.35, 0.6, 1.0);
                p.spacing = 0.5;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Flores/pétalas" => {
                p.spray = spray(2.0, 0.25, 0.6, 1.0);
                p.spacing = 0.5;
                p.flow = 0.7;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Rosas" => {
                p.spray = spray(1.5, 0.9, 0.6, 0.8);
                p.spacing = 0.4;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Flores de cerejeira" => {
                p.spray = spray(2.5, 0.25, 0.6, 1.0);
                p.spacing = 0.5;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Folhagem" => {
                p.spray = spray(6.0, 0.4, 0.6, 1.0);
                p.spacing = 0.15;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Folhas soltas" => {
                p.spray = spray(2.0, 0.35, 0.6, 1.0);
                p.spacing = 0.4;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Árvores" => {
                p.spray = spray(3.0, 0.7, 0.6, 0.7);
                p.spacing = 0.15;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Gramado" => {
                p.spray = spray(3.0, 0.5, 0.6, 1.0);
                p.spacing = 0.5;
                p.flow = 0.8;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Grama" => {
                p.spray = spray(3.0, 0.3, 0.6, 1.0);
                p.spacing = 0.5;
                p.flow = 0.15;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Folhas caídas" => {
                p.spray = spray(1.0, 0.5, 0.6, 1.0);
                p.spacing = 0.7;
                p.flow = 0.6;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Chão" => {
                p.spray = spray(6.0, 0.08, 0.4, 1.0);
                p.spacing = 0.2;
                p.flow = 0.5;
                p.scatter = 0.0;
                p.count = 1;
            }
            "Rachadura" => {
                p.angle_jitter = 8.0;
                p.size_jitter = 0.3;
                p.tip = BrushTip::Round { hardness: 1.0, roundness: 0.03, angle: 0.0 };
            }
            "Nuvem" => {
                p.flow = 0.5;
            }
            "Cascalho" => {
                p.spray = spray(2.0, 0.3, 0.6, 1.0);
                p.spacing = 0.5;
                p.flow = 0.7;
                p.scatter = 0.0;
                p.count = 1;
            }
            _ => {}
        }
    }

    /// Decorações de fábrica (S25, calibradas nos pincéis de Decoração do Clip Studio que servem para anime: Efeito e
    /// Plano de fundo, em 30/60 px): formas desenhadas pelo nosso motor (sakura, pena, bokeh, árvore, bordo, pedra,
    /// cintilância), espalhadas, giradas e com duas cores; nada das imagens de material dele.
    pub fn decorations() -> Vec<BrushPreset> {
        use crate::brush_dyn::{EdgeNoise, Grain, GrainKind};
        let deco = |name: &str, size: f32, shape: TipShape| {
            let mut p = BrushPreset::round(name, size, 1.0);
            p.tip = BrushTip::Shape { shape, aspect: 1.0 };
            p.group = "decoracao".into();
            p.category = "cenario".into();
            p.size_pressure_min = 1.0;
            p
        };
        let mut v = Vec::new();
        // efeito
        let mut g = deco("Purpurina", 100.0, TipShape::Glint);
        g.spacing = 0.12;
        g.count = 3;
        g.scatter = 1.0;
        g.size_jitter = 0.85;
        g.angle_jitter = 15.0;
        g.value_jitter = 0.25;
        v.push(g);
        let mut dl = BrushPreset::round("Luz entre folhas", 200.0, 0.2);
        dl.group = "decoracao".into();
        dl.category = "cenario".into();
        dl.size_pressure_min = 1.0;
        dl.spacing = 0.25;
        dl.count = 2;
        dl.scatter = 0.8;
        dl.size_jitter = 0.7;
        dl.flow = 0.35;
        dl.opacity = 0.8;
        v.push(dl);
        let mut sc = deco("Círculos suaves", 120.0, TipShape::Bokeh);
        sc.spacing = 0.2;
        sc.count = 2;
        sc.scatter = 1.0;
        sc.size_jitter = 0.7;
        sc.flow = 0.3;
        v.push(sc);
        let mut fe = deco("Penas", 120.0, TipShape::Feather);
        fe.tip = BrushTip::Shape { shape: TipShape::Feather, aspect: 0.6 };
        fe.spacing = 0.9;
        fe.scatter = 1.0;
        fe.size_jitter = 0.4;
        fe.angle_jitter = 70.0;
        fe.follow_direction = true;
        fe.two_tone = Some(TwoTone { tint: [0.55, 0.55, 0.6], amount: 0.5, color: None });
        v.push(fe);
        let mut ch = deco("Flores de cerejeira", 150.0, TipShape::Sakura);
        ch.spacing = 0.35;
        ch.count = 3;
        ch.scatter = 1.0;
        ch.size_jitter = 0.5;
        ch.angle_jitter = 180.0;
        ch.two_tone = Some(TwoTone { tint: [0.9, 0.45, 0.6], amount: 0.8, color: None });
        v.push(ch);
        // plano de fundo
        let mut lf = deco("Folhas soltas", 60.0, TipShape::Leaf);
        lf.tip = BrushTip::Shape { shape: TipShape::Leaf, aspect: 0.7 };
        lf.spacing = 0.3;
        lf.count = 3;
        lf.scatter = 1.1;
        lf.size_jitter = 0.4;
        lf.angle_jitter = 180.0;
        lf.value_jitter = 0.15;
        lf.two_tone = Some(TwoTone { tint: [0.45, 0.6, 0.62], amount: 0.7, color: None });
        v.push(lf);
        let mut tr = deco("Árvores", 200.0, TipShape::Tree);
        tr.tip = BrushTip::Shape { shape: TipShape::Tree, aspect: 0.75 };
        tr.spacing = 0.22;
        tr.scatter = 0.15;
        tr.size_jitter = 0.35;
        tr.value_jitter = 0.1;
        v.push(tr);
        let mut gl = deco("Gramado", 150.0, TipShape::GrassBlade);
        gl.spacing = 0.08;
        gl.count = 6;
        gl.scatter = 0.6;
        gl.size_jitter = 0.5;
        gl.angle_jitter = 18.0;
        gl.value_jitter = 0.2;
        gl.flow = 0.7;
        gl.two_tone = Some(TwoTone { tint: [0.4, 0.58, 0.5], amount: 0.65, color: None });
        v.push(gl);
        let mut fl = deco("Folhas caídas", 120.0, TipShape::Maple);
        fl.spacing = 0.7;
        fl.scatter = 1.4;
        fl.size_jitter = 0.4;
        fl.angle_jitter = 180.0;
        fl.value_jitter = 0.12;
        fl.two_tone = Some(TwoTone { tint: [0.7, 0.45, 0.35], amount: 0.6, color: None });
        v.push(fl);
        let mut gr = deco("Chão", 150.0, TipShape::Pebble);
        gr.spacing = 0.05;
        gr.count = 4;
        gr.scatter = 0.6;
        gr.size_jitter = 0.8;
        gr.angle_jitter = 180.0;
        gr.flow = 0.6;
        gr.grain = Some(Grain { kind: GrainKind::Speckle, scale: 1.2, depth: 0.6, contrast: 1.5, brightness: 0.0, subtract: true, moving: true, seed: 0 });
        v.push(gr);
        let mut cr = BrushPreset::round("Rachadura", 120.0, 1.0);
        cr.group = "decoracao".into();
        cr.category = "cenario".into();
        cr.size_pressure_min = 1.0;
        cr.tip = BrushTip::Round { hardness: 1.0, roundness: 0.06, angle: 0.0 };
        cr.follow_direction = true;
        cr.spacing = 0.02;
        cr.angle_jitter = 25.0;
        cr.edge = Some(EdgeNoise { amount: 0.6, grain: 3.0, seed: 0 });
        v.push(cr);
        let mut ru = deco("Cascalho", 150.0, TipShape::Pebble);
        ru.spacing = 0.3;
        ru.count = 2;
        ru.scatter = 1.0;
        ru.size_jitter = 0.6;
        ru.angle_jitter = 180.0;
        ru.value_jitter = 0.15;
        ru.two_tone = Some(TwoTone { tint: [0.5, 0.5, 0.55], amount: 0.7, color: None });
        v.push(ru);
        // ---- 28/10: o que faltava do Clip Studio
        let mut bk = deco("Livros", 120.0, TipShape::Book);
        bk.tip = BrushTip::Shape { shape: TipShape::Book, aspect: 1.0 };
        bk.spacing = 0.38;
        bk.size_jitter = 0.18;
        bk.value_jitter = 0.35;
        bk.hue_jitter = 0.12;
        bk.two_tone = Some(TwoTone { tint: [0.95, 0.85, 0.5], amount: 0.9, color: None });
        v.push(bk);
        let mut bd = deco("Prédios", 260.0, TipShape::Building);
        bd.spacing = 0.55;
        bd.size_jitter = 0.45;
        bd.value_jitter = 0.12;
        // janelas acesas: cor própria (amarelo quente), não um tom do prédio
        bd.two_tone = Some(TwoTone { tint: [1.0, 1.0, 1.0], amount: 0.85, color: Some([1.0, 0.86, 0.5]) });
        v.push(bd);
        let mut sp = deco("Mancha de sangue", 120.0, TipShape::Splat);
        sp.spacing = 0.6;
        sp.count = 2;
        sp.scatter = 0.9;
        sp.size_jitter = 0.7;
        sp.angle_jitter = 180.0;
        sp.value_jitter = 0.2;
        v.push(sp);
        // linha com régua: decoram uma linha seguindo o traço
        let line = |name: &str, size: f32, shape: TipShape, spacing: f32| {
            let mut p = deco(name, size, shape);
            p.follow_direction = true;
            p.spacing = spacing;
            p
        };
        v.push(line("Linha ondulada", 30.0, TipShape::Wave, 1.0));
        let mut dash = line("Linha tracejada", 12.0, TipShape::Square, 2.2);
        dash.tip = BrushTip::Round { hardness: 1.0, roundness: 0.3, angle: 0.0 };
        v.push(dash);
        let mut dot = BrushPreset::round("Linha pontilhada", 10.0, 1.0);
        dot.group = "decoracao".into();
        dot.category = "cenario".into();
        dot.size_pressure_min = 1.0;
        dot.spacing = 1.9;
        v.push(dot);
        v.push(line("Linha de estrelas", 30.0, TipShape::Star, 1.3));
        let mut rough = BrushPreset::round("Bordas rugosas", 16.0, 1.0);
        rough.group = "decoracao".into();
        rough.category = "cenario".into();
        rough.size_pressure_min = 1.0;
        rough.spacing = 0.04;
        rough.edge = Some(EdgeNoise { amount: 0.55, grain: 2.5, seed: 0 });
        v.push(rough);
        let mut irr = BrushPreset::round("Linha irregular", 14.0, 1.0);
        irr.group = "decoracao".into();
        irr.category = "cenario".into();
        irr.size_pressure_min = 1.0;
        irr.spacing = 0.05;
        irr.size_dyn = Some(crate::brush_dyn::Dynamics { random: Some(0.35), random_len: 18.0, ..Default::default() });
        v.push(irr);
        let mut mel = line("Melodia", 40.0, TipShape::Note, 1.4);
        mel.follow_direction = false;
        mel.scatter = 0.5;
        mel.size_jitter = 0.3;
        mel.angle_jitter = 15.0;
        v.push(mel);
        // roupas
        let mut bt = deco("Botões", 40.0, TipShape::Button);
        bt.spacing = 2.4;
        bt.two_tone = Some(TwoTone { tint: [0.7, 0.7, 0.75], amount: 0.6, color: None });
        v.push(bt);
        let mut bead = deco("Contas", 18.0, TipShape::Bead);
        bead.spacing = 1.0;
        bead.two_tone = Some(TwoTone { tint: [0.55, 0.55, 0.6], amount: 0.8, color: None });
        v.push(bead);
        for (name, size) in [("Corrente", 36.0f32), ("Corrente fina", 16.0)] {
            let mut c = line(name, size, TipShape::ChainLink, 0.62);
            c.extra_tips = vec![BrushTip::Shape { shape: TipShape::ChainLinkSide, aspect: 1.0 }];
            c.tip_order = TipOrder::Sequence;
            v.push(c);
        }
        let mut zp = line("Zíper", 28.0, TipShape::ZipperA, 0.42);
        zp.extra_tips = vec![BrushTip::Shape { shape: TipShape::ZipperB, aspect: 1.0 }];
        zp.tip_order = TipOrder::Sequence;
        v.push(zp);
        let mut br = line("Trança", 40.0, TipShape::BraidL, 0.4);
        br.extra_tips = vec![BrushTip::Shape { shape: TipShape::BraidR, aspect: 1.0 }];
        br.tip_order = TipOrder::Sequence;
        br.two_tone = Some(TwoTone { tint: [0.7, 0.62, 0.55], amount: 0.5, color: None });
        v.push(br);
        v.push(line("Renda", 40.0, TipShape::Scallop, 0.98));
        let mut sw = line("Floreado simples", 40.0, TipShape::Swirl, 1.15);
        sw.follow_direction = true;
        v.push(sw);
        v
    }

    /// Pincéis de fábrica (S25, calibrados nos pincéis Nanquim, Aquarela e Pintura grossa do Clip Studio 5.1.5 em
    /// 10/30/60 px e na folha de mistura vermelho/azul): multiplicar, borda de aquarela, grão de papel e a tinta que
    /// mistura com a camada (`paint_mix`), com os números do banco de ferramentas dele e pontas desenhadas pelo nosso motor.
    pub fn paint_brushes() -> Vec<BrushPreset> {
        use crate::brush_dyn::{DynInput, Dynamics, EdgeNoise, Grain, GrainKind, PaintMix, Spray, Stabilization};
        let brush = |name: &str, size: f32, hardness: f32| {
            let mut p = BrushPreset::round(name, size, hardness);
            p.group = "pincel".into();
            p.category = "pintura".into();
            p.antialias = Some(1);
            p.spacing = 0.1;
            p.size_pressure_min = 1.0;
            p
        };
        let paper = |depth: f32| Some(Grain { kind: GrainKind::Paper, scale: 1.6, depth, contrast: 1.0, brightness: 0.0, subtract: false, moving: false, seed: 0 });
        let mix = |amount: f32, density: f32, stretch: f32| Some(PaintMix { amount, density, stretch, smear: false, blur: 0.0, amount_dyn: None });
        let mut v = Vec::new();
        // nanquim
        let mut bp = brush("Caneta pincel", 70.0, 1.0);
        bp.antialias = Some(2);
        bp.spacing = 0.02;
        bp.stabilization = Some(Stabilization { level: 15, by_speed: true });
        bp.size_dyn = Some(Dynamics::pressure(0.0));
        v.push(bp);
        let mut di = brush("Nanquim seco", 60.0, 1.0);
        di.mode = BrushMode::Multiply;
        di.antialias = Some(3);
        di.spacing = 0.02;
        di.edge = Some(EdgeNoise { amount: 0.5, grain: 1.5, seed: 0 });
        di.grain = paper(0.85);
        di.size_dyn = Some(Dynamics { velocity: Some(DynInput { min: 0.45, curve: vec![[0.0, 1.0], [1.0, 0.0]] }), ..Default::default() });
        v.push(di);
        let mut wi = brush("Nanquim aguado", 100.0, 1.0);
        wi.mode = BrushMode::Multiply;
        wi.antialias = Some(3);
        wi.spacing = 0.02;
        wi.flow = 0.65;
        wi.max_ink = true;
        wi.edge = Some(EdgeNoise { amount: 0.2, grain: 2.0, seed: 0 });
        wi.grain = paper(0.65);
        v.push(wi);
        let mut wb = brush("Nanquim de mancha úmida", 30.0, 0.3);
        wb.mode = BrushMode::Multiply;
        wb.antialias = Some(0);
        wb.spacing = 0.05;
        wb.flow = 0.8;
        wb.max_ink = true;
        wb.wet_edges = 0.6;
        wb.spray = Some(Spray { count: 2.0, size: 1.0, sync: true, size_min: 0.6, roundness_min: 0.7, bias: 0.0, count_exp: 0.0, spread: 2.3, align: false });
        wb.grain = paper(0.7);
        v.push(wb);
        // aquarela
        let mut rw = brush("Aquarela redonda", 80.0, 1.0);
        rw.mode = BrushMode::Multiply;
        rw.flow = 0.6;
        rw.max_ink = true;
        rw.wet_edges = 0.3;
        rw.grain = paper(0.7);
        v.push(rw);
        let mut fw = brush("Aquarela chata", 100.0, 1.0);
        fw.mode = BrushMode::Multiply;
        fw.tip = BrushTip::Round { hardness: 1.0, roundness: 0.45, angle: 90.0 };
        fw.spacing = 0.01;
        fw.wet_edges = 0.4;
        fw.flow = 0.55;
        fw.max_ink = true;
        fw.grain = paper(0.6);
        v.push(fw);
        let mut sb = brush("Transbordo suave", 120.0, 1.0);
        sb.mode = BrushMode::Multiply;
        sb.antialias = Some(0);
        sb.flow = 0.45;
        sb.max_ink = true;
        sb.wet_edges = 0.6;
        sb.grain = paper(0.8);
        v.push(sb);
        let mut ww = brush("Lavagem molhada", 30.0, 0.3);
        ww.mode = BrushMode::Multiply;
        ww.antialias = Some(0);
        ww.spacing = 0.01;
        ww.spray = Some(Spray { count: 4.0, size: 1.0, sync: true, size_min: 0.6, roundness_min: 1.0, bias: 0.0, count_exp: 0.0, spread: 2.4, align: false });
        ww.paint_mix = mix(0.44, 0.5, 0.1);
        ww.grain = paper(0.54);
        v.push(ww);
        let mut rh = brush("Lavagem áspera", 150.0, 1.0);
        rh.mode = BrushMode::Multiply;
        rh.flow = 0.75;
        rh.max_ink = true;
        rh.wet_edges = 0.4;
        rh.edge = Some(EdgeNoise { amount: 0.4, grain: 3.0, seed: 0 });
        rh.grain = paper(0.5);
        v.push(rh);
        let mut sp = brush("Respingos de aquarela", 300.0, 1.0);
        sp.mode = BrushMode::Multiply;
        sp.spacing = 0.09;
        sp.flow = 0.35;
        sp.spray = Some(Spray { count: 1.0, size: 0.3, sync: true, size_min: 0.1, roundness_min: 0.5, bias: 0.0, count_exp: 0.0, spread: 1.0, align: false });
        sp.edge = Some(EdgeNoise { amount: 0.5, grain: 2.0, seed: 0 });
        v.push(sp);
        // pintura grossa
        let mut rm = brush("Pincel redondo de mistura", 50.0, 1.0);
        rm.paint_mix = mix(0.8, 0.5, 1.0);
        rm.flow = 0.88;
        rm.max_ink = true;
        rm.stabilization = Some(Stabilization { level: 2, by_speed: true });
        v.push(rm);
        let mut gu = brush("Guache", 80.0, 1.0);
        gu.antialias = Some(0);
        gu.flow = 0.82;
        gu.max_ink = true;
        gu.spacing = 0.03;
        gu.paint_mix = mix(0.8, 0.6, 0.15);
        gu.grain = paper(0.15);
        v.push(gu);
        let mut dg = brush("Guache seco", 80.0, 1.0);
        dg.antialias = Some(0);
        dg.flow = 0.75;
        dg.max_ink = true;
        dg.spacing = 0.05;
        dg.paint_mix = mix(0.9, 0.8, 0.0);
        dg.edge = Some(EdgeNoise { amount: 0.35, grain: 2.0, seed: 0 });
        dg.grain = paper(0.6);
        v.push(dg);
        let mut tg = brush("Guache fino", 20.0, 1.0);
        tg.spacing = 0.01;
        tg.paint_mix = mix(1.0, 0.55, 0.7);
        tg.grain = paper(0.1);
        tg.flow = 0.87;
        tg.max_ink = true;
        v.push(tg);
        let mut oi = brush("Óleo", 80.0, 1.0);
        oi.spacing = 0.01;
        oi.paint_mix = mix(0.61, 0.81, 0.87);
        oi.grain = paper(0.12);
        oi.flow = 0.9;
        oi.max_ink = true;
        v.push(oi);
        let mut to = brush("Óleo grosso", 60.0, 1.0);
        to.flow = 0.85;
        to.spacing = 0.02;
        to.tip = BrushTip::Round { hardness: 1.0, roundness: 0.6, angle: 70.0 };
        to.paint_mix = mix(0.6, 0.5, 0.6);
        to.grain = paper(0.5);
        v.push(to);
        let mut po = brush("Pontilhismo", 80.0, 1.0);
        po.spray = Some(Spray { count: 6.0, size: 0.3, sync: true, size_min: 0.5, roundness_min: 0.25, bias: 0.0, count_exp: 0.0, spread: 1.0, align: false });
        po.flow = 0.8;
        po.paint_mix = mix(0.5, 0.5, 0.15);
        v.push(po);
        let mut pa = brush("Pintar e aplicar", 40.0, 0.4);
        pa.antialias = Some(2);
        pa.flow = 0.8;
        pa.paint_mix = Some(PaintMix { amount: 0.8, density: 1.0, stretch: 0.5, smear: true, blur: 0.0, amount_dyn: None });
        pa.stabilization = Some(Stabilization { level: 2, by_speed: true });
        v.push(pa);
        v
    }

    /// Misturadores de fábrica (S25, calibrados nos misturadores do Clip Studio passando sobre uma grade vermelha e azul
    /// feita com a Pena G 30): não põem cor, levam a que está na camada (média ou arrastada) ou desfocam.
    pub fn blenders() -> Vec<BrushPreset> {
        use crate::brush_dyn::{PaintMix, Stabilization};
        let bl = |name: &str, size: f32, hardness: f32, stretch: f32, smear: bool| {
            let mut p = BrushPreset::round(name, size, hardness);
            p.group = "mistura".into();
            p.category = "pintura".into();
            p.antialias = Some(2);
            p.spacing = 0.1;
            p.size_pressure_min = 1.0;
            p.paint_mix = Some(PaintMix { amount: 0.0, density: 0.0, stretch, smear, blur: 0.0, amount_dyn: None });
            p
        };
        let mut v = Vec::new();
        let mut m = bl("Misturar", 80.0, 0.01, 0.85, false);
        m.flow = 0.6;
        v.push(m);
        let mut b = bl("Desfocar", 80.0, 0.01, 0.0, false);
        if let Some(m) = b.paint_mix.as_mut() {
            m.blur = 5.0;
        }
        v.push(b);
        let mut f = bl("Ponta do dedo", 30.0, 0.5, 0.7, true);
        f.spacing = 0.03;
        f.stabilization = Some(Stabilization { level: 4, by_speed: true });
        v.push(f);
        let mut pb = bl("Misturador de pintura", 100.0, 0.01, 0.9, false);
        pb.flow = 0.25;
        pb.spacing = 0.015;
        v.push(pb);
        let mut wb = bl("Misturador molhado", 150.0, 0.01, 0.6, false);
        wb.flow = 0.5;
        wb.spacing = 0.2;
        wb.antialias = Some(0);
        v.push(wb);
        let mut tb = bl("Misturador texturizado", 100.0, 1.0, 0.6, false);
        tb.opacity = 0.8;
        tb.flow = 0.6;
        tb.spacing = 0.25;
        v.push(tb);
        let mut wa = bl("Pincel aquoso", 120.0, 1.0, 0.1, false);
        wa.opacity = 0.75;
        wa.flow = 0.7;
        wa.antialias = Some(0);
        v.push(wa);
        let mut gb = bl("Misturador de guache", 80.0, 1.0, 0.6, false);
        gb.flow = 0.5;
        gb.spacing = 0.2;
        v.push(gb);
        v
    }

    /// Lápis de fábrica (S25, calibrados nas sub-ferramentas Lápis, Lapiseira, Carvão, Giz de cera, Pastel e Giz do
    /// Clip Studio 5.1.5 em 3/10/30 px): grão de papel nosso (procedural), ponta macia ou de bastão, dinâmicas de
    /// pressão do banco de ferramentas dele.
    pub fn pencils() -> Vec<BrushPreset> {
        use crate::brush_dyn::{DynInput, Dynamics, EdgeNoise, Grain, GrainKind, Stabilization};
        let curve = |min: f32, pts: &[[f32; 2]]| DynInput { min, curve: pts.to_vec() };
        let grain = |kind: GrainKind, scale: f32, depth: f32, contrast: f32, subtract: bool| Some(Grain { kind, scale, depth, contrast, brightness: 0.0, subtract, moving: false, seed: 0 });
        let lap = |name: &str, size: f32, hardness: f32| {
            let mut p = BrushPreset::round(name, size, hardness);
            p.group = "lapis".into();
            p.category = "lapis".into();
            p.antialias = Some(1);
            p.spacing = 0.1;
            p.size_pressure_min = 1.0;
            p
        };
        let mut v = Vec::new();
        // lápis: ponta macia granulada, cinza que escurece com a pressão, borda irregular
        let mut l = lap("Lápis", 10.0, 0.7);
        l.opacity = 0.9;
        l.flow = 0.4;
        l.size_dyn = Some(Dynamics::pressure(0.1));
        l.flow_dyn = Some(Dynamics { pressure: Some(curve(0.5, &[[0.0, 0.0], [0.5214, 0.0571], [0.7857, 0.4357], [0.8714, 0.7857], [1.0, 1.0]])), random: Some(0.6), ..Default::default() });
        l.grain = grain(GrainKind::Paper, 2.0, 0.9, 2.0, false);
        l.edge = Some(EdgeNoise { amount: 0.5, grain: 3.0, seed: 0 });
        l.stabilization = Some(Stabilization { level: 5, by_speed: true });
        v.push(l);
        // lapiseira: grafite fino, cinza uniforme (a tinta não soma ao repassar), grão largo e suave
        let mut m = lap("Lapiseira", 5.0, 0.5);
        m.spacing = 0.08;
        m.flow = 0.85;
        m.max_ink = true;
        m.size_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.2273, 0.0], [0.6818, 1.0], [1.0, 1.0]])), ..Default::default() });
        m.flow_dyn = Some(Dynamics { pressure: Some(curve(0.6, &[[0.0, 0.0], [0.8455, 0.1182], [1.0, 1.0]])), ..Default::default() });
        m.grain = grain(GrainKind::Paper, 4.0, 0.6, 1.3, false);
        m.edge = Some(EdgeNoise { amount: 0.3, grain: 3.0, seed: 0 });
        m.stabilization = Some(Stabilization { level: 5, by_speed: true });
        v.push(m);
        // carvão: lateral do bastão, cheio do lado esquerdo do traço e esmaecendo no grão do papel
        let mut c = lap("Carvão", 20.0, 1.0);
        c.tip = BrushTip::Shape { shape: TipShape::Stick, aspect: 1.0 };
        c.spacing = 0.08;
        c.follow_direction = true;
        c.size_dyn = Some(Dynamics { pressure: Some(curve(0.5, &[[0.0, 0.0], [0.2588, 0.0], [0.75, 1.0], [1.0, 1.0]])), ..Default::default() });
        c.flow_dyn = Some(Dynamics { pressure: Some(curve(0.3, &[[0.0, 0.0], [0.6, 0.0], [0.693, 1.0], [0.8909, 1.0], [1.0, 1.0]])), ..Default::default() });
        c.grain = grain(GrainKind::Paper, 2.0, 0.85, 1.5, true);
        v.push(c);
        // giz de cera: cera em manchas grandes que falham nos vales, borda bem irregular
        let mut g = lap("Giz de cera", 30.0, 1.0);
        g.tip = BrushTip::Round { hardness: 1.0, roundness: 0.8, angle: 0.0 };
        g.angle_jitter = 144.0;
        g.spacing = 0.12;
        g.flow = 0.8;
        g.size_dyn = Some(Dynamics::pressure(0.3));
        g.flow_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.2545, 0.0818], [0.5273, 0.5545], [0.9909, 0.7182], [1.0, 1.0]])), ..Default::default() });
        g.grain = grain(GrainKind::Coarse, 5.0, 1.0, 3.0, true);
        g.edge = Some(EdgeNoise { amount: 0.45, grain: 4.0, seed: 0 });
        v.push(g);
        // pastel seco: pó claro em pontinhos, borda dura sem antisserrilhado
        let mut ps = lap("Pastel", 40.0, 0.9);
        ps.tip = BrushTip::Round { hardness: 0.9, roundness: 0.8, angle: 0.0 };
        ps.antialias = Some(0);
        ps.flow = 0.3;
        ps.max_ink = true;
        ps.size_dyn = Some(Dynamics::pressure(0.4));
        ps.flow_dyn = Some(Dynamics::pressure(0.15));
        ps.grain = Some(Grain { kind: GrainKind::Speckle, scale: 0.6, depth: 1.0, contrast: 3.0, brightness: -0.1, subtract: true, moving: false, seed: 0 });
        v.push(ps);
        // giz: ponta larga deitada (traço horizontal largo), grão fino do papel
        let mut z = lap("Giz", 30.0, 1.0);
        z.tip = BrushTip::Round { hardness: 1.0, roundness: 0.75, angle: 90.0 };
        z.flow = 0.6;
        z.size_dyn = Some(Dynamics { pressure: Some(curve(0.5, &[[0.0, 0.0], [0.1364, 0.6909], [0.75, 1.0], [1.0, 1.0]])), ..Default::default() });
        z.flow_dyn = Some(Dynamics { pressure: Some(curve(0.5, &[[0.0, 0.0], [0.6, 0.0], [0.693, 1.0], [0.8772, 1.0], [1.0, 1.0]])), ..Default::default() });
        z.thickness_dyn = Some(Dynamics::pressure(0.7));
        z.grain = grain(GrainKind::Paper, 1.5, 0.9, 1.8, false);
        z.edge = Some(EdgeNoise { amount: 0.2, grain: 3.0, seed: 0 });
        v.push(z);
        v
    }

    /// Aerógrafos de fábrica (S25, calibrados em Suave, Spray, Gotícula, Ruído e Spray colorido pingando do Clip
    /// Studio em 10/30/60 px): ponta macia com cauda, partículas e grão que anda, desenhados pelo nosso motor.
    pub fn airbrushes() -> Vec<BrushPreset> {
        use crate::brush_dyn::{DynInput, Dynamics, EdgeNoise, Grain, GrainKind, Spray};
        let air = |name: &str, size: f32, hardness: f32| {
            let mut p = BrushPreset::round(name, size, hardness);
            p.group = "aerografo".into();
            p.category = "pintura".into();
            p.antialias = Some(2);
            p.spacing = 0.1;
            p.size_pressure_min = 1.0;
            p
        };
        let speckle = |scale: f32, contrast: f32| Some(Grain { kind: GrainKind::Speckle, scale, depth: if scale > 1.1 { 0.9 } else { 1.0 }, contrast, brightness: 0.0, subtract: true, moving: true, seed: 0 });
        let mut v = Vec::new();
        // suave: nuvem gaussiana; repassar não escurece além do fluxo
        let mut s = air("Aerógrafo suave", 150.0, 0.0);
        s.antialias = Some(3);
        s.flow = 0.62;
        s.max_ink = true;
        s.flow_dyn = Some(Dynamics::pressure(0.0));
        v.push(s);
        // spray: pontinhos soltos, mesma densidade em qualquer tamanho
        let mut sp = air("Spray", 200.0, 1.0);
        sp.flow = 0.9;
        sp.spray = Some(Spray { count: 12.0, size: 1.5, sync: false, size_min: 1.0, roundness_min: 1.0, bias: 0.0, count_exp: 2.0, spread: 1.0, align: false });
        v.push(sp);
        // gotículas: respingos pretos de tamanhos variados, achatados e de borda irregular
        let mut d = air("Gotículas", 250.0, 1.0);
        d.spray = Some(Spray { count: 3.0, size: 9.0, sync: false, size_min: 0.05, roundness_min: 0.3, bias: 0.0, count_exp: 1.0, spread: 1.0, align: false });
        d.size_dyn = Some(Dynamics { random: Some(0.3), ..Default::default() });
        d.edge = Some(EdgeNoise { amount: 0.45, grain: 2.0, seed: 0 });
        v.push(d);
        // ruído: nuvem granulada em gomos (o grão anda a cada carimbo e a tinta varia ao longo do traço)
        let mut n = air("Ruído", 250.0, 0.2);
        n.antialias = Some(0);
        n.flow = 0.3;
        n.size_dyn = Some(Dynamics::pressure(0.75));
        n.flow_dyn = Some(Dynamics { pressure: Some(DynInput::linear(0.0)), random: Some(0.4), random_len: 12.0, ..Default::default() });
        n.grain = speckle(1.2, 3.0);
        v.push(n);
        // spray colorido: bolinhas granuladas que acompanham o tamanho
        let mut rc = air("Spray colorido", 200.0, 0.5);
        rc.flow = 0.9;
        rc.flow_dyn = Some(Dynamics::pressure(0.0));
        rc.spray = Some(Spray { count: 4.0, size: 0.25, sync: true, size_min: 0.4, roundness_min: 1.0, bias: 0.0, count_exp: 0.0, spread: 1.0, align: false });
        rc.grain = speckle(1.0, 2.5);
        v.push(rc);
        v
    }

    /// Borrachas de fábrica (S25, calibradas nas borrachas do Clip Studio apagando uma camada preta em 10/30/60 px).
    /// Na camada vetorial, cada uma tira o que o nome diz (`vector_erase`); a de várias camadas apaga em todas as
    /// camadas do quadro e a de encaixe segue a régua. A ferramenta Borracha de sempre (`eraser`) continua igual.
    pub fn erasers() -> Vec<BrushPreset> {
        use crate::brush_dyn::{DynInput, Dynamics, Grain, GrainKind, Stabilization};
        let curve = |min: f32, pts: &[[f32; 2]]| DynInput { min, curve: pts.to_vec() };
        let er = |name: &str, size: f32, hardness: f32| {
            let mut p = BrushPreset::round(name, size, hardness);
            p.group = "borracha".into();
            p.category = "borracha".into();
            p.mode = BrushMode::Erase;
            p.antialias = Some(2);
            p.spacing = 0.08;
            p.size_pressure_min = 1.0;
            p.max_ink = true;
            p
        };
        let stab = Some(Stabilization { level: 2, by_speed: true });
        let mut v = Vec::new();
        let mut h = er("Borracha dura", 30.0, 1.0);
        h.size_dyn = Some(Dynamics::pressure(0.0));
        h.vector_erase = Some(VectorErase::Segment);
        h.stabilization = stab;
        v.push(h);
        let mut s = er("Borracha macia", 50.0, 0.0);
        s.flow_dyn = Some(Dynamics::pressure(0.0));
        s.stabilization = stab;
        v.push(s);
        // limpa-tipos: tira só parte (clareia) e com o grão do papel
        let mut k = er("Borracha limpa-tipos", 50.0, 1.0);
        k.antialias = Some(1);
        k.max_ink = false;
        k.opacity = 0.9;
        k.flow = 0.75;
        k.size_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.4868, 0.8202], [0.8455, 1.0], [1.0, 1.0]])), ..Default::default() });
        k.flow_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.3364, 0.2273], [0.8545, 1.0], [1.0, 1.0]])), ..Default::default() });
        k.grain = Some(Grain { kind: GrainKind::Paper, scale: 3.0, depth: 0.8, contrast: 1.5, brightness: 0.0, subtract: false, moving: false, seed: 0 });
        k.edge = Some(crate::brush_dyn::EdgeNoise { amount: 0.25, grain: 3.0, seed: 0 });
        k.snap = true;
        v.push(k);
        // áspera: grande, e na camada vetorial leva o traço inteiro
        let mut r = er("Borracha áspera", 100.0, 1.0);
        r.vector_erase = Some(VectorErase::Whole);
        v.push(r);
        // vetorial: fina, e na camada vetorial apaga até as interseções
        let mut ve = er("Borracha vetorial", 3.0, 1.0);
        ve.vector_erase = Some(VectorErase::Intersection);
        v.push(ve);
        let mut ml = er("Borracha de várias camadas", 20.0, 1.0);
        ml.antialias = Some(0);
        ml.all_layers = true;
        v.push(ml);
        let mut sn = er("Borracha de encaixe", 30.0, 1.0);
        sn.size_dyn = Some(Dynamics::pressure(0.0));
        sn.snap = true;
        sn.vector_erase = Some(VectorErase::Segment);
        sn.stabilization = stab;
        v.push(sn);
        let mut dy = er("Borracha dinâmica", 20.0, 1.0);
        dy.vector_erase = Some(VectorErase::Segment);
        dy.stabilization = stab;
        v.push(dy);
        v
    }

    /// Canetas e marcadores de fábrica (S25): o resultado calibrado nas canetas de fábrica do Clip Studio Paint 5.1.5
    /// (folha de teste em 3, 10 e 30 px; `tools/csp/calib_compare.py`), com os números do banco de ferramentas dele
    /// (tamanho padrão, curvas de pressão, mínimos, velocidade, espessura e ângulo da ponta, suavização de serrilhado,
    /// estabilização). Nomes das penas reais; pontas e bordas desenhadas pelo nosso motor.
    pub fn pens() -> Vec<BrushPreset> {
        use crate::brush_dyn::{DynInput, Dynamics, EdgeNoise, Stabilization};
        let curve = |min: f32, pts: &[[f32; 2]]| DynInput { min, curve: pts.to_vec() };
        let stab = |level: u8| Some(Stabilization { level, by_speed: true });
        let pen = |name: &str, size: f32, group: &str| {
            let mut p = BrushPreset::round(name, size, 1.0);
            p.group = group.into();
            p.category = "lapis".into();
            p.antialias = Some(2);
            p.spacing = 0.08;
            p.size_pressure_min = 1.0;
            p.max_ink = true;
            p
        };
        let mut v = Vec::new();
        // pena G: pressão do zero ao tamanho cheio, linear
        let mut g = pen("Pena G", 10.0, "caneta");
        g.size_dyn = Some(Dynamics::pressure(0.0));
        g.stabilization = stab(6);
        v.push(g);
        // pena G real: ponta levemente achatada girando ao acaso, borda granulada; a espessura também cai com pouca pressão
        let mut gr = pen("Pena G real", 10.0, "caneta");
        gr.tip = BrushTip::Round { hardness: 1.0, roundness: 0.85, angle: 0.0 };
        gr.angle_jitter = 180.0;
        gr.size_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.745, 0.255], [1.0, 1.0]])), ..Default::default() });
        gr.thickness_dyn = Some(Dynamics { pressure: Some(curve(0.5, &[[0.0, 0.0], [0.345, 0.0], [0.664, 0.955], [1.0, 1.0]])), ..Default::default() });
        gr.edge = Some(EdgeNoise { amount: 0.22, grain: 4.0, seed: 0 });
        gr.stabilization = stab(6);
        v.push(gr);
        // pena de mapeamento: linha fina que só engorda com pressão forte
        let mut m = pen("Pena de mapeamento", 10.0, "caneta");
        m.size_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.5, 0.245], [0.927, 0.709], [1.0, 1.0]])), ..Default::default() });
        m.stabilization = stab(10);
        v.push(m);
        // pena nabo: engorda logo no começo da pressão
        let mut t = pen("Pena nabo", 8.0, "caneta");
        t.size_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.182, 0.4], [0.7, 0.964], [1.0, 1.0]])), ..Default::default() });
        t.stabilization = stab(2);
        v.push(t);
        // caligrafia: ponta chata fina inclinada (fino na diagonal "/", grosso na "\"), sem pressão
        let mut c = pen("Caligrafia", 50.0, "caneta");
        c.tip = BrushTip::Round { hardness: 1.0, roundness: 0.2, angle: 148.0 };
        c.stabilization = stab(6);
        v.push(c);
        // caneta texturizada: ponta macia achatada girando ao acaso (borda em gomos, desfocada), tinta que soma
        let mut tx = pen("Caneta texturizada", 12.0, "caneta");
        tx.tip = BrushTip::Round { hardness: 0.8, roundness: 0.6, angle: 0.0 };
        tx.angle_jitter = 180.0;
        tx.antialias = Some(1);
        tx.max_ink = false;
        tx.size_dyn = Some(Dynamics::pressure(0.0));
        tx.stabilization = stab(6);
        v.push(tx);
        // caneta rugosa: fita áspera que segue a direção, largura variando aos trechos, afina um pouco quando rápida
        let mut r = pen("Caneta rugosa", 1.2, "caneta");
        r.follow_direction = true;
        r.antialias = Some(1);
        r.size_dyn = Some(Dynamics { pressure: Some(DynInput::linear(0.0)), velocity: Some(DynInput::slower_is_bigger(0.5)), random: Some(0.8), random_len: 40.0, ..Default::default() });
        r.thickness_dyn = Some(Dynamics { velocity: Some(DynInput::slower_is_bigger(0.5)), random: Some(0.85), random_len: 30.0, ..Default::default() });
        r.velocity_full = 25000.0;
        r.edge = Some(EdgeNoise { amount: 0.45, grain: 5.0, seed: 0 });
        r.stabilization = stab(6);
        v.push(r);
        // caneta milimétrica: espessura quase fixa, afina um pouco no traço rápido
        let mut mm = pen("Caneta milimétrica", 10.0, "marcador");
        mm.size_dyn = Some(Dynamics { pressure: Some(curve(0.8, &[[0.0, 0.0], [0.718, 0.309], [1.0, 1.0]])), velocity: Some(DynInput::slower_is_bigger(0.6)), ..Default::default() });
        mm.velocity_full = 13000.0;
        v.push(mm);
        // caneta de feltro: ponta larga deitada; rápida fica fina e falhada (pouca tinta), lenta fica cheia
        let mut f = pen("Caneta de feltro", 20.0, "marcador");
        f.tip = BrushTip::Round { hardness: 1.0, roundness: 0.8, angle: 0.0 };
        f.antialias = Some(1);
        f.size_dyn = Some(Dynamics { pressure: Some(DynInput::linear(0.7)), velocity: Some(DynInput::slower_is_bigger(0.7)), random: Some(0.9), ..Default::default() });
        f.flow_dyn = Some(Dynamics { velocity: Some(DynInput::slower_is_bigger(0.18)), ..Default::default() });
        f.velocity_full = 2000.0;
        f.max_ink = false;
        f.stabilization = stab(6);
        v.push(f);
        // caneta de pixel: 1 px sem antisserrilhado, sem degrau dobrado
        let mut px = pen("Caneta de pixel", 1.0, "marcador");
        px.aliased = true;
        px.antialias = Some(0);
        px.spacing = 0.1;
        v.push(px);
        // marcador chato: ponta chata deitada (traço horizontal fino, vertical largo); a pressão mexe no tamanho e na opacidade
        let mut fm = pen("Marcador chato", 50.0, "marcador");
        fm.tip = BrushTip::Round { hardness: 1.0, roundness: 0.3, angle: 0.0 };
        fm.antialias = Some(1);
        fm.spacing = 0.03;
        fm.size_dyn = Some(Dynamics { pressure: Some(curve(0.6, &[[0.0, 0.0], [0.557, 0.061], [0.934, 0.601], [1.0, 1.0]])), ..Default::default() });
        fm.opacity_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.588, 0.0], [0.921, 0.675], [1.0, 1.0]])), ..Default::default() });
        v.push(fm);
        v
    }
}

/// Amostra do caminho do pincel (canvas px).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BrushPoint {
    pub x: f32,
    pub y: f32,
    #[serde(default = "d_one")]
    pub pressure: f32,
    /// Inclinação da caneta em graus (tiltX, tiltY de -90 a 90, como o Windows Ink entrega); `None` = mouse ou caneta
    /// sem inclinação (as dinâmicas de inclinação não mexem no carimbo).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tilt: Option<[f32; 2]>,
}

/// Inclinação entre dois pontos (as duas precisam existir).
pub fn lerp_tilt(a: Option<[f32; 2]>, b: Option<[f32; 2]>, f: f32) -> Option<[f32; 2]> {
    match (a, b) {
        (Some(a), Some(b)) => Some([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

/// Quanto a caneta está deitada, 0 (em pé) a 1 (60° ou mais, o máximo das mesas comuns).
pub fn tilt_amount(t: [f32; 2]) -> f32 {
    (t[0].hypot(t[1]) / 60.0).clamp(0.0, 1.0)
}

/// Abaixo deste raio (px) a ponta clássica reparte a tinta entre pixels em vez de amostrar o centro de cada um.
const TONE_MIN_R: f32 = 0.71;

/// Fluxo de um carimbo para o traço ter o mesmo tom que teria com intervalo de 10% (densidade pelo intervalo):
/// a cobertura de 1 - (1 - fluxo)^(intervalo / 0,1).
pub fn flow_for_spacing(flow: f32, spacing: f32) -> f32 {
    let f = flow.clamp(0.0, 1.0);
    if f >= 1.0 {
        return 1.0;
    }
    1.0 - (1.0 - f).powf((spacing / 0.1).clamp(0.05, 20.0))
}

fn texture_value(t: &BrushTexture, x: f32, y: f32) -> f32 {
    let (x, y) = if t.rotation != 0.0 {
        let (sn, cs) = t.rotation.to_radians().sin_cos();
        (x * cs + y * sn, -x * sn + y * cs)
    } else {
        (x, y)
    };
    if let Some(m) = t.image_mask.as_deref().filter(|m| m.width > 0 && m.height > 0) {
        // imagem repetida, bilinear
        let z = t.scale.max(0.05);
        let (u, v) = (x / z, y / z);
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let at = |xx: f32, yy: f32| m.get((xx as i64).rem_euclid(m.width as i64) as u32, (yy as i64).rem_euclid(m.height as i64) as u32);
        let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
        let bot = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
        let r = top * (1.0 - fy) + bot * fy;
        return if t.invert { 1.0 - r } else { r };
    }
    let s = t.scale.max(0.25);
    let (u, v) = (x / s, y / s);
    let raw = match t.kind {
        TextureKind::Paper => 0.55 * fbm(u * 0.35, v * 0.35, 4, 2.0, 0.55, t.seed) + 0.45 * value_noise(u * 1.7, v * 1.7, t.seed ^ 77),
        TextureKind::Canvas => {
            // trama: senos cruzados + ruído
            let wx = (u * std::f32::consts::PI).sin().abs();
            let wy = (v * std::f32::consts::PI).sin().abs();
            0.35 + 0.35 * wx.max(wy) + 0.3 * value_noise(u * 2.0, v * 2.0, t.seed)
        }
        TextureKind::Noise => fbm(u * 0.25, v * 0.25, 5, 2.0, 0.5, t.seed),
        TextureKind::Cells => worley(u * 0.3, v * 0.3, t.seed),
        TextureKind::Grain => value_noise(u, v, t.seed),
    };
    let r = raw.clamp(0.0, 1.0);
    if t.invert { 1.0 - r } else { r }
}

/// Coordenadas locais (u, v) da ponta de forma, ou `None` se a ponta não é de forma.
fn shape_uv(tip: &BrushTip, dx: f32, dy: f32, radius: f32, extra_angle: f32) -> Option<(TipShape, f32, f32)> {
    let BrushTip::Shape { shape, aspect } = tip else { return None };
    let (sn, cs) = extra_angle.to_radians().sin_cos();
    let (lx, ly) = (dx * cs + dy * sn, -dx * sn + dy * cs);
    let r = radius.max(0.25);
    Some((*shape, lx / (r * aspect.clamp(0.1, 4.0)), ly / r))
}

fn tip_alpha(tip: &BrushTip, dx: f32, dy: f32, radius: f32, extra_angle: f32, aliased: bool) -> f32 {
    tip_alpha_tone(tip, dx, dy, radius, extra_angle, aliased, false)
}

fn tip_alpha_tone(tip: &BrushTip, dx: f32, dy: f32, radius: f32, extra_angle: f32, aliased: bool, two_tone: bool) -> f32 {
    match tip {
        BrushTip::Round { hardness, roundness, angle } => {
            let a = (angle + extra_angle).to_radians();
            let (s, c) = a.sin_cos();
            let lx = dx * c + dy * s;
            let ly = (-dx * s + dy * c) / roundness.clamp(0.05, 1.0);
            let d = (lx * lx + ly * ly).sqrt() / radius.max(0.25);
            if aliased {
                return if d <= 1.0 { 1.0 } else { 0.0 };
            }
            // borda antisserrilhada em ~1 px
            let edge = ((1.0 - d) * radius).clamp(0.0, 1.0);
            let h = hardness.clamp(0.0, 1.0);
            let inner = if d <= h { 1.0 } else { let t = ((d - h) / (1.0 - h).max(1e-4)).clamp(0.0, 1.0); 0.5 + 0.5 * (t * std::f32::consts::PI).cos() };
            inner * edge
        }
        BrushTip::Shape { .. } => {
            let (shape, u, v) = shape_uv(tip, dx, dy, radius, extra_angle).expect("ponta de forma");
            let val = shape_alpha_tone(shape, u, v, 1.0 / radius.max(0.25), two_tone);
            if aliased { if val >= 0.5 { 1.0 } else { 0.0 } } else { val }
        }
        BrushTip::Image { mask, .. } => {
            let Some(mask) = mask else { return 0.0 };
            let a = extra_angle.to_radians();
            let (s, c) = a.sin_cos();
            let lx = dx * c + dy * s;
            let ly = -dx * s + dy * c;
            let u = (lx / (2.0 * radius) + 0.5) * mask.width as f32;
            let v = (ly / (2.0 * radius) + 0.5) * mask.height as f32;
            if u < 0.0 || v < 0.0 || u >= mask.width as f32 || v >= mask.height as f32 {
                return 0.0;
            }
            let val = mask.get(u as u32, v as u32);
            if aliased { if val >= 0.5 { 1.0 } else { 0.0 } } else { val }
        }
    }
}

/// Ponta redonda do modelo de caneta (S25): borda pela distância real à elipse (a ponta chata fica nítida na lateral fina),
/// rampa do antisserrilhado no nível pedido (`aa_ramp`), dureza, espessura extra (`thick` multiplica a redondeza) e borda
/// áspera. `None` para pontas que não são redondas (seguem a cobertura de sempre).
fn tip_alpha_pen(tip: &BrushTip, dx: f32, dy: f32, radius: f32, extra_angle: f32, aa: u8, thick: f32, edge_k: f32, rough: bool) -> Option<f32> {
    let BrushTip::Round { hardness, roundness, angle } = tip else { return None };
    let a = (angle + extra_angle).to_radians();
    let (s, c) = a.sin_cos();
    let lx = dx * c + dy * s;
    let ly = -dx * s + dy * c;
    let mut rr = radius.max(0.25);
    let rho = (roundness * thick).clamp(0.02, 1.0).max((crate::brush_dyn::min_semi_axis() / rr).min(1.0));
    rr *= edge_k;
    let (ux, uy) = (lx / rr, ly / (rr * rho));
    let q = (ux * ux + uy * uy).sqrt();
    // distância (px) à borda da elipse ≈ (q - 1) / |∇q|
    let g = ((lx / (rr * rr)).powi(2) + (ly / (rr * rr * rho * rho)).powi(2)).sqrt() / q.max(1e-6);
    // ponta macia (dureza < 1): queda gaussiana que passa um pouco do raio, como a ponta macia do Clip Studio (o
    // aerógrafo de 30 px tem meia altura a ~10 px do eixo e cauda até ~20 px)
    let h = hardness.clamp(0.0, 1.0);
    let ext = 1.0 + crate::brush_dyn::SOFT_TAIL * (1.0 - h);
    let sd = if q < 1e-6 { -rr * rho * ext } else { (q - ext) / g.max(1e-6) };
    let edge_cov = if aa == 0 {
        if sd <= 0.0 { 1.0 } else { 0.0 }
    } else {
        // ponta áspera (no Clip Studio é uma imagem amostrada): a borda não recua, só a rampa
        let (w, shift) = crate::brush_dyn::aa_ramp(aa);
        let shift = if edge_k < 1.0 || rough { 0.0 } else { shift };
        ((-sd - shift) / w + 0.5).clamp(0.0, 1.0)
    };
    let inner = if q <= h || h >= 1.0 {
        1.0
    } else {
        let t = (q - h) / (1.0 - h).max(1e-4);
        (-(t * t) / (2.0 * crate::brush_dyn::SOFT_SIGMA * crate::brush_dyn::SOFT_SIGMA)).exp()
    };
    Some(inner * edge_cov)
}

fn rgb_to_hsv(c: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (c[0], c[1], c[2]);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let h = if d <= 0.0 {
        0.0
    } else if mx == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if mx == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    [h, if mx > 0.0 { d / mx } else { 0.0 }, mx]
}

fn hsv_to_rgb(c: [f32; 3]) -> [f32; 3] {
    let (h, s, v) = (c[0].rem_euclid(1.0) * 6.0, c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0));
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Resultado: caixa afetada [x0, y0, x1, y1) (para recortar em tiles).
pub struct PaintResult {
    pub bounds: [i32; 4],
}

/// Pinta um traço na `layer` (pré-multiplicada, tamanho do canvas). `color` = sRGB não pré-multiplicado com alfa.
pub fn paint_stroke(layer: &mut Image, preset: &BrushPreset, path: &[BrushPoint], color: [f32; 4], seed: u64) -> Option<PaintResult> {
    paint_stroke_at(layer, [0, 0], preset, path, color, seed)
}

/// Como [`paint_stroke`], com `layer` sendo um recorte do canvas que começa em `origin` (os pontos do caminho já vêm
/// em coordenadas do recorte); só a textura do papel usa a posição absoluta, para o grão não andar com o recorte.
pub fn paint_stroke_at(layer: &mut Image, origin: [i32; 2], preset: &BrushPreset, path: &[BrushPoint], color: [f32; 4], seed: u64) -> Option<PaintResult> {
    if path.is_empty() {
        return None;
    }
    let mut sb = StrokeBuffer::new(preset, color, seed, layer.width, layer.height, origin);
    // o borrador lê a camada de antes do traço (o buffer do traço é composto só no fim)
    sb.add(Some(&*layer), path);
    sb.finish(Some(&*layer));
    sb.composite(layer, [0, 0])
}

// ------------------------------------------------------------------------------------------------ traço incremental

const STS: i32 = 64;

/// Dab gerado e ainda não carimbado (o primeiro espera a direção do primeiro trecho, como no traço inteiro).
#[derive(Clone, Copy)]
struct Dab {
    x: f32,
    y: f32,
    pr: f32,
    dir: f32,
    /// Distância percorrida no caminho até este dab (px), para o degradê ao longo do traço e o afinamento.
    s: f32,
    /// Velocidade normalizada (0..1) no momento do dab.
    v: f32,
    /// Fator de afinamento já conhecido (entrada); a saída multiplica ao carimbar.
    taper: f32,
    /// Inclinação da caneta neste dab (graus), interpolada entre os pontos.
    tilt: Option<[f32; 2]>,
}

/// Traço de pincel em andamento, alimentado aos pedaços: guarda o buffer do traço (em tiles), o gerador aleatório, o
/// espaçamento acumulado, a direção suavizada e a pegada do borrador entre uma chamada e outra. Alimentar o caminho
/// inteiro de uma vez ou em pedaços dá o MESMO resultado (é o mesmo laço, com o estado guardado): a prévia ao vivo
/// do app carimba só o trecho novo a cada movimento e o commit passa pelo mesmo código.
#[derive(Clone)]
pub struct StrokeBuffer {
    preset: BrushPreset,
    color: [f32; 4],
    /// Limites da camada (coordenadas locais) e deslocamento da textura do papel (posição absoluta no canvas).
    w: i32,
    h: i32,
    tex_origin: [i32; 2],
    rng: Rng,
    drng: Rng,
    /// Tiles compartilhados (`Arc`): a cópia para a prévia do fim do traço custa só os ponteiros; quem escreve num tile
    /// compartilhado ganha a própria cópia dele.
    tiles: std::collections::HashMap<(i32, i32), std::sync::Arc<Vec<[f32; 4]>>>,
    dual: Option<std::collections::HashMap<(i32, i32), std::sync::Arc<Vec<f32>>>>,
    /// Retângulo [x0, y0, x1, y1) carimbado desde o último `take_dirty` (prévia: só ele é recomposto).
    dirty: Option<[i32; 4]>,
    /// Maior alfa do buffer (o nível da borda molhada; o alfa de um pixel do traço só sobe).
    max_a: f32,
    carry: Option<(i32, Vec<[f32; 4]>)>,
    /// Tinta carregada (pré-multiplicada) do `paint_mix` em modo média.
    loaded: Option<[f32; 4]>,
    /// Próxima ponta da sequência (Multi-Tip em ordem).
    tip_seq: usize,
    /// Variação de cor do traço inteiro (`jitter_per_stroke`): matiz, saturação, brilho, secundária.
    stroke_jit: Option<[f32; 4]>,
    /// Misturadores e Desfocar trocam o pixel (cor e alfa) em vez de pintar por cima: o buffer guarda o pixel final e
    /// este mapa (tiles de STS×STS) marca os pixels tocados.
    touched: Option<std::collections::HashMap<(i32, i32), Vec<bool>>>,
    prev: Option<(BrushPoint, f32)>,
    acc: f32,
    sdir: Option<f32>,
    pending: Option<Dab>,
    /// Degradê ao longo do traço (`BrushPreset::color_ramp`) e o comprimento de uma passada.
    ramp: Option<(aiai_core::ResolvedGradient, f32)>,
    /// Comprimento do caminho já percorrido (px).
    travel: f32,
    /// Velocidade do traço (só com amostras com tempo; sem tempo, conta como parado).
    speed: crate::brush_dyn::SpeedEstimator,
    /// Dabs dos últimos `taper.end` px, à espera de saber onde o traço termina (saída).
    held: std::collections::VecDeque<Dab>,
    /// Caneta de pixel: pixels ainda não carimbados (o do meio de um degrau em L sai).
    pix: Vec<(i32, i32)>,
    pix_last: Option<(i32, i32)>,
    /// Semente do aleatório suave ao longo do traço.
    noise_seed: u32,
}

impl StrokeBuffer {
    /// Traço numa camada `w`×`h` (coordenadas locais); `tex_origin` = posição do canto da camada no canvas (textura).
    pub fn new(preset: &BrushPreset, color: [f32; 4], seed: u64, w: u32, h: u32, tex_origin: [i32; 2]) -> Self {
        StrokeBuffer {
            preset: preset.clone(),
            color,
            w: w as i32,
            h: h as i32,
            tex_origin,
            rng: Rng::new(seed),
            drng: Rng::new(seed ^ 0xD0A1),
            tiles: Default::default(),
            dual: preset.dual.as_ref().map(|_| Default::default()),
            carry: None,
            loaded: None,
            tip_seq: 0,
            stroke_jit: None,
            touched: preset.paint_mix.as_ref().filter(|m| m.amount <= 0.0 || m.blur > 0.0).map(|_| Default::default()),
            prev: None,
            acc: 0.0,
            sdir: None,
            pending: None,
            dirty: None,
            max_a: 0.0,
            ramp: preset.color_ramp.as_ref().map(|r| (r.resolved(), r.length.max(1.0))),
            travel: 0.0,
            speed: Default::default(),
            held: Default::default(),
            pix: Vec::new(),
            pix_last: None,
            noise_seed: (seed ^ (seed >> 32)) as u32,
        }
    }

    /// Retângulo carimbado desde a última chamada (coordenadas locais).
    pub fn take_dirty(&mut self) -> Option<[i32; 4]> {
        self.dirty.take()
    }

    fn mark(&mut self, r: [i32; 4]) {
        self.dirty = Some(match self.dirty {
            Some(d) => [d[0].min(r[0]), d[1].min(r[1]), d[2].max(r[2]), d[3].max(r[3])],
            None => r,
        });
    }

    fn buf_get(&self, x: i32, y: i32) -> [f32; 4] {
        match self.tiles.get(&(x.div_euclid(STS), y.div_euclid(STS))) {
            Some(t) => t[(y.rem_euclid(STS) * STS + x.rem_euclid(STS)) as usize],
            None => [0.0; 4],
        }
    }

    fn buf_mut(&mut self, x: i32, y: i32) -> &mut [f32; 4] {
        let t = self.tiles.entry((x.div_euclid(STS), y.div_euclid(STS))).or_insert_with(|| std::sync::Arc::new(vec![[0.0f32; 4]; (STS * STS) as usize]));
        &mut std::sync::Arc::make_mut(t)[(y.rem_euclid(STS) * STS + x.rem_euclid(STS)) as usize]
    }

    /// Mais pontos do caminho (coordenadas locais), sem tempo (a velocidade conta como parada). `layer` = a camada antes
    /// do traço, só para o borrador (mix > 0).
    pub fn add(&mut self, layer: Option<&Image>, pts: &[BrushPoint]) {
        for &p in pts {
            self.push_point(layer, p, f32::NAN);
        }
        self.flush_held(layer);
    }

    /// Como [`add`](Self::add), com o tempo (s desde o começo do traço) de cada ponto: liga as dinâmicas de velocidade
    /// e o afinamento pela velocidade. `times` tem o mesmo tamanho de `pts`.
    pub fn add_timed(&mut self, layer: Option<&Image>, pts: &[BrushPoint], times: &[f32]) {
        for (i, &p) in pts.iter().enumerate() {
            self.push_point(layer, p, times.get(i).copied().unwrap_or(f32::NAN));
        }
        self.flush_held(layer);
    }

    fn push_point(&mut self, layer: Option<&Image>, p: BrushPoint, t: f32) {
        match self.prev {
            None => {
                self.pending = Some(Dab { x: p.x, y: p.y, pr: p.pressure, dir: 0.0, s: 0.0, v: 0.0, taper: self.start_taper(0.0, 0.0), tilt: p.tilt });
                if t.is_finite() {
                    self.speed.push(0.0, t);
                }
            }
            Some((a, ta)) => self.segment(layer, a, ta, p, t),
        }
        self.prev = Some((p, t));
    }

    /// Fim do caminho: carimba o primeiro dab se o caminho não andou e os dabs da saída com o afinamento.
    pub fn finish(&mut self, layer: Option<&Image>) {
        if let Some(d) = self.pending.take() {
            self.emit(layer, d);
        }
        if let Some(tp) = self.preset.taper.clone().filter(|t| t.end > 0.0) {
            let len = if tp.by_speed { tp.end * self.vnorm(self.speed.speed()) } else { tp.end };
            let total = self.travel;
            while let Some(mut d) = self.held.pop_front() {
                d.taper *= tp.ramp(total - d.s, len);
                self.stamp(layer, d);
            }
        }
        for c in std::mem::take(&mut self.pix) {
            self.stamp_pixel(c);
        }
    }

    fn vnorm(&self, speed: f32) -> f32 {
        (speed / self.preset.velocity_full()).clamp(0.0, 1.0)
    }

    /// Fator da entrada na distância `s` do começo (velocidade normalizada `v` para "pela velocidade").
    fn start_taper(&self, s: f32, v: f32) -> f32 {
        match &self.preset.taper {
            Some(tp) if tp.start > 0.0 => {
                let len = if tp.by_speed { tp.start * v } else { tp.start };
                tp.ramp(s, len)
            }
            _ => 1.0,
        }
    }

    /// Dab pronto: carimba, ou espera a saída (os últimos `taper.end` px).
    fn emit(&mut self, layer: Option<&Image>, d: Dab) {
        if self.preset.taper.as_ref().is_some_and(|t| t.end > 0.0) {
            self.held.push_back(d);
        } else {
            self.stamp(layer, d);
        }
    }

    /// Carimba os dabs que já estão longe o bastante do fim para não pegar a saída.
    fn flush_held(&mut self, layer: Option<&Image>) {
        let Some(end) = self.preset.taper.as_ref().map(|t| t.end).filter(|e| *e > 0.0) else { return };
        while self.held.front().is_some_and(|d| self.travel - d.s > end) {
            let d = self.held.pop_front().expect("há dab");
            self.stamp(layer, d);
        }
    }

    /// Tamanho do dab sem o aleatório (espaçamento entre carimbos).
    fn base_diam(&self, pr: f32, v: f32, taper: f32) -> f32 {
        let p = &self.preset;
        let f = match &p.size_dyn {
            Some(d) => d.base(pr, v),
            None => p.size_pressure_min + (1.0 - p.size_pressure_min) * pr.clamp(0.0, 1.0),
        };
        let tsz = if p.taper.as_ref().is_some_and(|t| t.size) { taper } else { 1.0 };
        let d = p.size * f * tsz;
        if p.min_one_px { d.max(1.0) } else { d }
    }

    fn segment(&mut self, layer: Option<&Image>, a: BrushPoint, ta: f32, b: BrushPoint, tb: f32) {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        // pincel duplo: segunda ponta ao longo do caminho (máximo das coberturas); o traço só aparece onde ela cobre
        if let Some(dual) = self.preset.dual.clone() {
            let r = (dual.size * 0.5).max(0.3);
            let step = (dual.size * dual.spacing).max(0.5);
            let n = (len / step).ceil().max(1.0) as usize;
            let tip = BrushTip::Round { hardness: dual.hardness, roundness: 1.0, angle: 0.0 };
            for k in 0..n {
                let f = k as f32 / n as f32;
                let px = a.x + (b.x - a.x) * f + dual.scatter * dual.size * self.drng.signed();
                let py = a.y + (b.y - a.y) * f + dual.scatter * dual.size * self.drng.signed();
                let (dx0, dy0, dx1, dy1) = (((px - r - 1.0) as i32).max(0), ((py - r - 1.0) as i32).max(0), ((px + r + 1.0) as i32 + 1).min(self.w), ((py + r + 1.0) as i32 + 1).min(self.h));
                if dx0 < dx1 && dy0 < dy1 {
                    self.mark([dx0, dy0, dx1, dy1]);
                }
                for y in dy0..dy1 {
                    for x in dx0..dx1 {
                        let a2 = tip_alpha(&tip, x as f32 + 0.5 - px, y as f32 + 0.5 - py, r, 0.0, false);
                        let db = self.dual.as_mut().expect("pincel duplo");
                        let t = std::sync::Arc::make_mut(db.entry((x.div_euclid(STS), y.div_euclid(STS))).or_insert_with(|| std::sync::Arc::new(vec![0.0f32; (STS * STS) as usize])));
                        let i = (y.rem_euclid(STS) * STS + x.rem_euclid(STS)) as usize;
                        t[i] = t[i].max(a2);
                    }
                }
            }
        }
        if len <= 1e-6 {
            return;
        }
        let sp = if ta.is_finite() && tb.is_finite() { self.speed.push(len, tb) } else { self.speed.speed() };
        let v = self.vnorm(sp);
        // caneta de pixel: os pixels por onde o caminho passa, um por vez, sem o degrau duplo
        if self.preset.is_pixel_pen() {
            if let Some(d) = self.pending.take() {
                self.pixel_at(d.x, d.y);
            }
            let n = (len * 4.0).ceil().max(1.0) as usize;
            for k in 1..=n {
                let f = k as f32 / n as f32;
                self.pixel_at(a.x + dx * f, a.y + dy * f);
            }
            self.travel += len;
            return;
        }
        // direção com passa-baixa (≈ meio diâmetro de memória): a ponta elíptica não gira aos trancos nas microparadas
        let memory = (self.preset.size * 0.5).max(1.0);
        let raw = dy.atan2(dx).to_degrees();
        let dir = match self.sdir {
            None => raw,
            Some(prev) => {
                let d = (raw - prev + 540.0).rem_euclid(360.0) - 180.0;
                prev + d * (1.0 - (-len / memory).exp())
            }
        };
        self.sdir = Some(dir);
        if let Some(pd) = &mut self.pending {
            pd.dir = dir;
        }
        // com saída, os dabs do fim encolhem: o passo acompanha o menor tamanho possível (sem contas)
        let end_k = self.preset.taper.as_ref().filter(|t| t.end > 0.0 && t.size).map(|t| t.min.max(0.2)).unwrap_or(1.0);
        let mut t = 0.0f32;
        loop {
            let pr = a.pressure + (b.pressure - a.pressure) * (t / len);
            let s_here = self.travel + t;
            let diam = self.base_diam(pr, v, self.start_taper(s_here, v));
            let step = (diam * end_k * self.preset.spacing).max(0.5);
            let need = step - self.acc;
            if t + need > len {
                self.acc += len - t;
                break;
            }
            t += need;
            self.acc = 0.0;
            let f = t / len;
            if let Some(d) = self.pending.take() {
                self.emit(layer, d);
            }
            let s = self.travel + t;
            let d = Dab { x: a.x + dx * f, y: a.y + dy * f, pr: a.pressure + (b.pressure - a.pressure) * f, dir, s, v, taper: self.start_taper(s, v), tilt: lerp_tilt(a.tilt, b.tilt, f) };
            self.emit(layer, d);
        }
        self.travel += len;
    }

    /// Caneta de pixel: o pixel sob (x, y) entra na fila; pulos viram linha de Bresenham e o pixel do meio de um degrau em
    /// L sai (linha de 1 px sem pixel dobrado).
    fn pixel_at(&mut self, x: f32, y: f32) {
        // um centésimo de folga: ponto em cima da divisa entre pixels (mouse em px inteiros, conta da visão com zoom)
        // não troca de pixel por erro de arredondamento
        let c = ((x + 0.01).floor() as i32, (y + 0.01).floor() as i32);
        if self.pix_last == Some(c) {
            return;
        }
        let mut cells = Vec::new();
        match self.pix_last {
            Some(l) if (c.0 - l.0).abs() > 1 || (c.1 - l.1).abs() > 1 => {
                let (mut x0, mut y0) = l;
                let (dx, dy) = ((c.0 - x0).abs(), -(c.1 - y0).abs());
                let (sx, sy) = (if c.0 > x0 { 1 } else { -1 }, if c.1 > y0 { 1 } else { -1 });
                let mut err = dx + dy;
                while (x0, y0) != c {
                    let e2 = 2 * err;
                    if e2 >= dy {
                        err += dy;
                        x0 += sx;
                    }
                    if e2 <= dx {
                        err += dx;
                        y0 += sy;
                    }
                    cells.push((x0, y0));
                }
            }
            _ => cells.push(c),
        }
        self.pix_last = Some(c);
        for c in cells {
            if let [.., a, b] = self.pix[..] {
                let orth = |p: (i32, i32), q: (i32, i32)| (p.0 - q.0).abs() + (p.1 - q.1).abs() == 1;
                if orth(a, b) && orth(b, c) && (a.0 - c.0).abs() == 1 && (a.1 - c.1).abs() == 1 {
                    self.pix.pop();
                }
            }
            self.pix.push(c);
            while self.pix.len() > 2 {
                let p = self.pix.remove(0);
                self.stamp_pixel(p);
            }
        }
    }

    fn stamp_pixel(&mut self, (x, y): (i32, i32)) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let p = &self.preset;
        let da = (p.flow * self.color[3]).clamp(0.0, 1.0);
        let rgb = [self.color[0], self.color[1], self.color[2]];
        self.mark([x, y, x + 1, y + 1]);
        let d = self.buf_mut(x, y);
        let k = 1.0 - da;
        for c in 0..3 {
            d[c] = rgb[c] * da + d[c] * k;
        }
        d[3] = da + d[3] * k;
        let a = d[3];
        self.max_a = self.max_a.max(a);
    }

    /// Um carimbo de spray: partículas espalhadas no disco de raio `r` em volta de (cx, cy).
    #[allow(clippy::too_many_arguments)]
    fn stamp_spray(&mut self, preset: &BrushPreset, sp: &crate::brush_dyn::Spray, cx: f32, cy: f32, r: f32, angle: f32, thick: f32, alpha: f32, rgb: [f32; 3], grain_off: (f32, f32)) {
        let count = sp.count_for(preset.size).min(5000.0);
        let mut n = count.floor() as u32;
        if self.rng.f32() < count.fract() {
            n += 1;
        }
        let hardness = match preset.tip {
            BrushTip::Round { hardness, .. } => hardness,
            _ => 1.0,
        };
        let aa = preset.antialias.unwrap_or(if preset.aliased { 0 } else { 1 });
        // decoração: cada partícula é a forma da ponta (folha, sakura, pedra…), girada pelo `angle_jitter` do pincel
        // (árvores ficam em pé) e com a segunda cor
        let shape_tip = matches!(preset.tip, BrushTip::Shape { .. });
        let tip_angle = match preset.tip {
            BrushTip::Round { angle, .. } => angle,
            _ => 0.0,
        };
        let two = preset.two_tone.as_ref().filter(|_| shape_tip).map(|t| t.second(rgb));
        for _ in 0..n {
            let u = self.rng.f32();
            let ang = self.rng.f32() * std::f32::consts::TAU;
            let rad = r * sp.spread * u.powf(0.5 + 0.5 * sp.bias.clamp(0.0, 1.0) * 2.0);
            let (px, py) = (cx + rad * ang.cos(), cy + rad * ang.sin());
            let k = sp.size_min + (1.0 - sp.size_min) * self.rng.f32();
            let pr = (sp.diameter(preset.size) * 0.5 * k).max(0.3);
            let round = sp.roundness_min + (1.0 - sp.roundness_min) * self.rng.f32();
            let pang = if shape_tip {
                angle + preset.angle_jitter * self.rng.signed()
            } else if sp.align {
                // partículas alinhadas: o ângulo da ponta (com "seguir o traço", a direção do traço)
                angle + tip_angle
            } else {
                angle + self.rng.f32() * 360.0
            };
            let tip = if shape_tip { preset.tip.clone() } else { BrushTip::Round { hardness, roundness: round, angle: 0.0 } };
            let pad = 1.0 + crate::brush_dyn::aa_ramp(aa).0 * 0.5 + pr * crate::brush_dyn::SOFT_TAIL;
            let (bx0, by0) = (((px - pr - pad).floor() as i32).max(0), ((py - pr - pad).floor() as i32).max(0));
            let (bx1, by1) = (((px + pr + pad).ceil() as i32).min(self.w), ((py + pr + pad).ceil() as i32).min(self.h));
            if bx0 >= bx1 || by0 >= by1 {
                continue;
            }
            self.mark([bx0, by0, bx1, by1]);
            for y in by0..by1 {
                for x in bx0..bx1 {
                    let (dx, dy) = (x as f32 + 0.5 - px, y as f32 + 0.5 - py);
                    let tipa = if shape_tip { Some(tip_alpha_tone(&tip, dx, dy, pr, pang, preset.aliased, two.is_some())) } else { tip_alpha_pen(&tip, dx, dy, pr, pang, aa, thick, 1.0, false) };
                    let Some(mut a) = tipa else { continue };
                    if let Some(g) = &preset.grain {
                        a = g.apply(a, (x + self.tex_origin[0]) as f32 + grain_off.0, (y + self.tex_origin[1]) as f32 + grain_off.1);
                    }
                    let da = (a * alpha).clamp(0.0, 1.0);
                    if da <= 0.0 {
                        continue;
                    }
                    let mut c = rgb;
                    if let (Some(c2), Some((shape, u, v))) = (two, shape_uv(&tip, dx, dy, pr, pang)) {
                        let k = shape_tone(shape, u, v, 1.0 / pr.max(0.25));
                        for i in 0..3 {
                            c[i] += (c2[i] - c[i]) * k;
                        }
                    }
                    let max_ink = preset.max_ink;
                    let d = self.buf_mut(x, y);
                    if max_ink {
                        if da > d[3] {
                            *d = [c[0] * da, c[1] * da, c[2] * da, da];
                        }
                    } else {
                        let k = 1.0 - da;
                        *d = [c[0] * da + d[0] * k, c[1] * da + d[1] * k, c[2] * da + d[2] * k, da + d[3] * k];
                    }
                    let a = d[3];
                    self.max_a = self.max_a.max(a);
                }
            }
        }
    }

    /// Uma fatia da fita: os pixels a até meio passo do carimbo, ao longo da direção do traço, pegam a coluna da imagem
    /// na distância percorrida (`s`); na transversal, a altura da imagem ocupa o diâmetro. Tinta pelo máximo: as fatias
    /// vizinhas se encostam sem dobrar a cor.
    fn stamp_ribbon(&mut self, preset: &BrushPreset, cx: f32, cy: f32, r: f32, dir_deg: f32, s: f32, step: f32, alpha: f32, rgb: [f32; 3]) {
        let BrushTip::Image { mask: Some(mask), .. } = &preset.tip else { return };
        if mask.width == 0 || mask.height == 0 {
            return;
        }
        let len = 2.0 * r * mask.width as f32 / mask.height as f32;
        let (sn, cs) = dir_deg.to_radians().sin_cos();
        let half = step * 0.5 + 0.75;
        let ext = half.max(r) + 1.0;
        let (bx0, by0) = (((cx - ext).floor() as i32).max(0), ((cy - ext).floor() as i32).max(0));
        let (bx1, by1) = (((cx + ext).ceil() as i32).min(self.w), ((cy + ext).ceil() as i32).min(self.h));
        if bx0 >= bx1 || by0 >= by1 {
            return;
        }
        self.mark([bx0, by0, bx1, by1]);
        for y in by0..by1 {
            for x in bx0..bx1 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let along = dx * cs + dy * sn;
                let across = -dx * sn + dy * cs;
                if along.abs() > half || across.abs() > r {
                    continue;
                }
                let u = ((s + along) / len.max(1.0)).rem_euclid(1.0) * mask.width as f32;
                let v = (across / (2.0 * r) + 0.5) * mask.height as f32;
                let a = mask.get((u as u32).min(mask.width - 1), (v.max(0.0) as u32).min(mask.height - 1));
                let da = (a * alpha).clamp(0.0, 1.0);
                if da <= 0.0 {
                    continue;
                }
                let d = self.buf_mut(x, y);
                if da > d[3] {
                    *d = [rgb[0] * da, rgb[1] * da, rgb[2] * da, da];
                }
                let a = d[3];
                self.max_a = self.max_a.max(a);
            }
        }
    }

    /// Sorteio do aleatório de uma dinâmica: por carimbo, ou suave ao longo do caminho (`random_len`).
    fn dyn_rnd(&mut self, dy: &crate::brush_dyn::Dynamics, s: f32, k: u32) -> f32 {
        if !dy.uses_random() {
            0.0
        } else if dy.random_len > 0.0 {
            crate::brush_dyn::smooth_random(s, dy.random_len, self.noise_seed.wrapping_add(k.wrapping_mul(0x51ED)))
        } else {
            self.rng.f32()
        }
    }

    fn stamp(&mut self, layer: Option<&Image>, d: Dab) {
        let mut preset = self.preset.clone();
        let tips: Vec<BrushTip> = if preset.extra_tips.is_empty() { Vec::new() } else { std::iter::once(preset.tip.clone()).chain(preset.extra_tips.iter().cloned()).collect() };
        let (cx, cy, dir) = (d.x, d.y, d.dir);
        let pr = d.pr.clamp(0.0, 1.0);
        let v = d.v;
        let color = match &self.ramp {
            Some((g, len)) => g.color_at(d.s / len),
            None => self.color,
        };
        let csp_tip = preset.antialias.is_some() || preset.edge.is_some() || preset.thickness_dyn.is_some();
        let (taper_size, taper_op) = match &preset.taper {
            Some(t) => (if t.size { d.taper } else { 1.0 }, if t.opacity { d.taper } else { 1.0 }),
            None => (1.0, 1.0),
        };
        // quanto a caneta está deitada neste dab (sem inclinação: as dinâmicas de inclinação ficam de fora)
        let tm = d.tilt.map(tilt_amount);
        for _ in 0..preset.count {
            // Multi-Tip: a ponta deste carimbo
            if !tips.is_empty() {
                let k = match preset.tip_order {
                    TipOrder::Sequence => {
                        self.tip_seq = (self.tip_seq + 1) % tips.len();
                        self.tip_seq
                    }
                    TipOrder::Random => ((self.rng.f32() * tips.len() as f32) as usize).min(tips.len() - 1),
                };
                preset.tip = tips[k].clone();
            }
            let mut size = match &preset.size_dyn {
                Some(dy) => {
                    let rnd = self.dyn_rnd(dy, d.s, 1);
                    preset.size * dy.factor_tilt(pr, v, tm, rnd)
                }
                None => preset.size * (preset.size_pressure_min + (1.0 - preset.size_pressure_min) * pr),
            };
            size *= taper_size;
            if preset.size_jitter > 0.0 {
                size *= 1.0 + preset.size_jitter * self.rng.signed();
            }
            if preset.min_one_px {
                size = size.max(1.0);
            }
            let r = (size * 0.5).max(0.3);
            let (mut px, mut py) = (cx, cy);
            if preset.scatter > 0.0 {
                let perp = (dir + 90.0).to_radians();
                let off = preset.scatter * preset.size * self.rng.signed();
                px += perp.cos() * off;
                py += perp.sin() * off;
                if preset.count > 1 {
                    px += preset.scatter * preset.size * 0.5 * self.rng.signed();
                }
            }
            let mut angle = match d.tilt.filter(|t| preset.follow_tilt && t[0].hypot(t[1]) > 1.0) {
                Some(t) => t[1].atan2(t[0]).to_degrees(),
                None if preset.follow_direction => dir,
                None => 0.0,
            };
            if preset.angle_jitter > 0.0 {
                angle += preset.angle_jitter * self.rng.signed();
            }
            let thick = match &preset.thickness_dyn {
                Some(dy) => {
                    let rnd = self.dyn_rnd(dy, d.s, 2);
                    dy.factor_tilt(pr, v, tm, rnd)
                }
                None => 1.0,
            };
            let edge = preset.edge.clone();
            let mut dab_opacity = match &preset.opacity_dyn {
                Some(dy) => {
                    let rnd = self.dyn_rnd(dy, d.s, 3);
                    dy.factor_tilt(pr, v, tm, rnd)
                }
                None => preset.opacity_pressure_min + (1.0 - preset.opacity_pressure_min) * pr,
            };
            dab_opacity *= taper_op;
            let flow = match &preset.flow_dyn {
                Some(dy) => {
                    let rnd = self.dyn_rnd(dy, d.s, 4);
                    preset.flow * dy.factor_tilt(pr, v, tm, rnd)
                }
                None => preset.flow * (preset.flow_pressure_min + (1.0 - preset.flow_pressure_min) * pr),
            };
            let flow = if preset.flow_by_spacing { flow_for_spacing(flow, preset.spacing) } else { flow };
            let mut rgb = [color[0], color[1], color[2]];
            let any_hsv = preset.hue_jitter > 0.0 || preset.saturation_jitter > 0.0 || preset.value_jitter > 0.0;
            let sub = preset.sub_color.filter(|_| preset.sub_jitter > 0.0);
            if any_hsv || sub.is_some() {
                let j = match self.stroke_jit {
                    Some(j) if preset.jitter_per_stroke => j,
                    _ => {
                        let mut j = [0.0f32; 4];
                        if any_hsv {
                            j[0] = self.rng.signed();
                            j[1] = self.rng.signed();
                            j[2] = self.rng.signed();
                        }
                        if sub.is_some() {
                            j[3] = self.rng.f32();
                        }
                        if preset.jitter_per_stroke {
                            self.stroke_jit = Some(j);
                        }
                        j
                    }
                };
                if let Some(c2) = sub {
                    let k = preset.sub_jitter * j[3];
                    for i in 0..3 {
                        rgb[i] += (c2[i] - rgb[i]) * k;
                    }
                }
                if any_hsv {
                    let mut hsv = rgb_to_hsv(rgb);
                    hsv[0] += preset.hue_jitter * 0.5 * j[0];
                    hsv[1] = (hsv[1] + preset.saturation_jitter * j[1]).clamp(0.0, 1.0);
                    hsv[2] = (hsv[2] + preset.value_jitter * j[2]).clamp(0.0, 1.0);
                    rgb = hsv_to_rgb(hsv);
                }
            }
            let two = preset.two_tone.as_ref().filter(|_| matches!(preset.tip, BrushTip::Shape { .. })).map(|t| t.second(rgb));
            // grão que anda: deslocamento sorteado a cada carimbo
            let grain_off = match &preset.grain {
                Some(g) if g.moving => (self.rng.f32() * 997.0, self.rng.f32() * 991.0),
                _ => (0.0, 0.0),
            };
            // fita: a fatia da imagem que cabe neste trecho do traço
            if preset.ribbon && matches!(&preset.tip, BrushTip::Image { mask: Some(_), .. }) {
                let step = (preset.spacing * preset.size).max(1.0);
                self.stamp_ribbon(&preset, px, py, r, dir, d.s, step, flow * dab_opacity * color[3], rgb);
                continue;
            }
            if let Some(sp) = preset.spray.clone() {
                self.stamp_spray(&preset, &sp, px, py, r, angle, thick, flow * dab_opacity * color[3], [rgb[0], rgb[1], rgb[2]], grain_off);
                continue;
            }
            // tinta que mistura (S25): a quantidade (com a dinâmica), o borrar estrutural e a tinta média carregada
            let pm = preset.paint_mix.as_ref();
            let amt = pm.map(|m| (m.amount * m.amount_dyn.as_ref().map(|dy| dy.factor(pr, v, 1.0)).unwrap_or(1.0)).clamp(0.0, 1.0)).unwrap_or(1.0);
            let (mix_eff, carry_k) = match pm {
                Some(m) if m.smear && m.blur <= 0.0 => (1.0 - amt, m.stretch),
                _ => (preset.mix, preset.smudge_carry),
            };
            let mut paint: Option<[f32; 4]> = None;
            let mut blur_src: Option<([i32; 2], usize, Vec<[f32; 4]>)> = None;
            if let Some(m) = pm.filter(|m| !m.smear || m.blur > 0.0) {
                if m.blur > 0.0 {
                    blur_src = Some(self.blurred_area(layer, px, py, r, m.blur));
                } else {
                    // a cor de baixo é a da camada ANTES do traço (o próprio traço à frente da ponta não conta)
                    let s = self.sample_mean(layer, px, py, r);
                    let k = m.stretch.clamp(0.0, 1.0);
                    let l = match self.loaded {
                        Some(l) => [0, 1, 2, 3].map(|c| l[c] * k + s[c] * (1.0 - k)),
                        None => s,
                    };
                    self.loaded = Some(l);
                    // cor: a do pincel puxada pela carregada (pesa a quantidade que falta e o alfa de baixo; canvas
                    // vazio = cor pura do pincel); alfa: o de baixo subindo até a quantidade/densidade do pincel
                    if amt <= 0.0 {
                        // misturador: só a cor carregada da camada, nada do pincel
                        paint = Some(l);
                    } else {
                        let w = (1.0 - amt) * l[3];
                        let lc = if l[3] > 1e-6 { [l[0] / l[3], l[1] / l[3], l[2] / l[3]] } else { [rgb[0], rgb[1], rgb[2]] };
                        let pa = l[3] + (1.0 - l[3]) * amt.max(m.density.clamp(0.0, 1.0));
                        paint = Some([rgb[0] * (1.0 - w) + lc[0] * w, rgb[1] * (1.0 - w) + lc[1] * w, rgb[2] * (1.0 - w) + lc[2] * w, 1.0].map(|v| v * pa));
                        if let Some(p) = paint.as_mut() {
                            p[3] = pa;
                        }
                    }
                }
            }
            if mix_eff > 0.0 {
                // amostra a pegada sob o dab (camada com o traço em andamento por cima) e mistura com a que vem
                // carregada: puxa a estrutura dos pixels (listras, bordas), não uma cor média
                let pr_i = r.ceil() as i32;
                let side = (2 * pr_i + 1) as usize;
                let (sx, sy) = (px.round() as i32, py.round() as i32);
                let mut sample = vec![[0.0f32; 4]; side * side];
                for j in 0..side as i32 {
                    for i in 0..side as i32 {
                        let (xx, yy) = (sx - pr_i + i, sy - pr_i + j);
                        if xx < 0 || yy < 0 || xx >= self.w || yy >= self.h {
                            continue;
                        }
                        let l = layer.filter(|l| (xx as u32) < l.width && (yy as u32) < l.height).map(|l| l.get(xx as u32, yy as u32)).unwrap_or([0.0; 4]);
                        let b = self.buf_get(xx, yy);
                        sample[j as usize * side + i as usize] = [0, 1, 2, 3].map(|c| b[c] + l[c] * (1.0 - b[3]));
                    }
                }
                let k = carry_k.clamp(0.0, 1.0);
                self.carry = Some(match self.carry.take() {
                    Some((cr, prev)) if cr == pr_i => (pr_i, prev.iter().zip(&sample).map(|(a, b)| [0, 1, 2, 3].map(|c| a[c] * k + b[c] * (1.0 - k))).collect()),
                    _ => (pr_i, sample),
                });
            }
            // a rampa do antisserrilhado forte passa um pouco do raio
            let pad = 1.0 + preset.antialias.map(|l| crate::brush_dyn::aa_ramp(l).0 * 0.5).unwrap_or(0.0) + if csp_tip { r * crate::brush_dyn::SOFT_TAIL } else { 0.0 };
            // ponta clássica minúscula (menos de ~1,4 px): amostrar só o centro do pixel faz o carimbo sumir quando cai na
            // divisa entre pixels; aí a tinta do disco (a área) é repartida entre os 4 pixels mais perto (bilinear)
            let tiny = !csp_tip && !preset.aliased && r < TONE_MIN_R;
            let tiny_area = (std::f32::consts::PI * r * r).min(1.0);
            let (bx0, by0) = (((px - r - pad).floor() as i32).max(0), ((py - r - pad).floor() as i32).max(0));
            let (bx1, by1) = (((px + r + pad).ceil() as i32).min(self.w), ((py + r + pad).ceil() as i32).min(self.h));
            if bx0 < bx1 && by0 < by1 {
                self.mark([bx0, by0, bx1, by1]);
            }
            for y in by0..by1 {
                for x in bx0..bx1 {
                    let (dx, dy) = (x as f32 + 0.5 - px, y as f32 + 0.5 - py);
                    let mut a = if csp_tip {
                        let ek = edge.as_ref().map(|e| e.radius_at((x + self.tex_origin[0]) as f32 + 0.5, (y + self.tex_origin[1]) as f32 + 0.5)).unwrap_or(1.0);
                        tip_alpha_pen(&preset.tip, dx, dy, r, angle, preset.antialias.unwrap_or(if preset.aliased { 0 } else { 1 }), thick, ek, edge.is_some())
                            .unwrap_or_else(|| tip_alpha_tone(&preset.tip, dx, dy, r, angle, preset.aliased, two.is_some()))
                    } else if tiny {
                        tiny_area * (1.0 - dx.abs()).max(0.0) * (1.0 - dy.abs()).max(0.0)
                    } else {
                        tip_alpha_tone(&preset.tip, dx, dy, r, angle, preset.aliased, two.is_some())
                    };
                    if a <= 0.0 {
                        continue;
                    }
                    if let Some(t) = &preset.texture {
                        let tv = texture_value(t, (x + self.tex_origin[0]) as f32, (y + self.tex_origin[1]) as f32);
                        a = match t.mode {
                            TextureMode::Multiply => a * (1.0 - t.depth + t.depth * tv),
                            TextureMode::Subtract => (a - (1.0 - tv) * t.depth).max(0.0),
                        };
                    }
                    if let Some(g) = &preset.grain {
                        a = g.apply(a, (x + self.tex_origin[0]) as f32 + grain_off.0, (y + self.tex_origin[1]) as f32 + grain_off.1);
                        if a <= 0.0 {
                            continue;
                        }
                    }
                    let mut da = (a * flow * dab_opacity * color[3]).clamp(0.0, 1.0);
                    if da <= 0.0 {
                        continue;
                    }
                    if self.touched.is_some() {
                        // misturador/Desfocar: o pixel final = o de baixo puxado para a cor levada (ou desfocada)
                        let target = match (&blur_src, paint) {
                            (Some((o, side, bv)), _) => {
                                let (i, j) = (x - o[0], y - o[1]);
                                if i < 0 || j < 0 || i as usize >= *side || j as usize >= *side {
                                    continue;
                                }
                                bv[j as usize * side + i as usize]
                            }
                            (None, Some(p)) => p,
                            // ponta do dedo: a pegada arrastada (estrutura dos pixels)
                            (None, None) => match &self.carry {
                                Some((cr, cp)) => {
                                    let side = (2 * cr + 1) as usize;
                                    let (ci, cj) = (x - (px.round() as i32 - cr), y - (py.round() as i32 - cr));
                                    if ci < 0 || cj < 0 || ci as usize >= side || cj as usize >= side {
                                        continue;
                                    }
                                    cp[cj as usize * side + ci as usize]
                                }
                                None => continue,
                            },
                        };
                        let u = self.under(layer, x, y);
                        let out = [0, 1, 2, 3].map(|c| u[c] * (1.0 - da) + target[c] * da);
                        *self.buf_mut(x, y) = out;
                        let t = self.touched.as_mut().expect("modo de troca").entry((x.div_euclid(STS), y.div_euclid(STS))).or_insert_with(|| vec![false; (STS * STS) as usize]);
                        t[(y.rem_euclid(STS) * STS + x.rem_euclid(STS)) as usize] = true;
                        self.max_a = self.max_a.max(out[3]);
                        continue;
                    }
                    let mut src = [rgb[0], rgb[1], rgb[2], 1.0];
                    if let Some(p) = paint {
                        if p[3] <= 1e-6 {
                            continue;
                        }
                        src = [p[0] / p[3], p[1] / p[3], p[2] / p[3], 1.0];
                        // sobre tinta que já está na camada, a mistura troca a cor por inteiro (a cor de baixo já entrou
                        // pela quantidade); no vazio, o alfa é o da tinta
                        let dp = da * p[3];
                        da = if amt > 0.0 {
                            let la = self.layer_px(layer, x, y)[3];
                            dp + (a.clamp(0.0, 1.0) - dp).max(0.0) * la
                        } else {
                            dp
                        };
                    }
                    if let Some((o, side, bv)) = &blur_src {
                        let (i, j) = (x - o[0], y - o[1]);
                        if i < 0 || j < 0 || i as usize >= *side || j as usize >= *side {
                            continue;
                        }
                        let b = bv[j as usize * side + i as usize];
                        if b[3] <= 1e-6 {
                            continue;
                        }
                        src = [b[0] / b[3], b[1] / b[3], b[2] / b[3], 1.0];
                        da *= b[3];
                    }
                    if let (Some(c2), Some((shape, u, v))) = (two, shape_uv(&preset.tip, dx, dy, r, angle)) {
                        let k = shape_tone(shape, u, v, 1.0 / r.max(0.25));
                        for c in 0..3 {
                            src[c] += (c2[c] - src[c]) * k;
                        }
                    }
                    if let Some((cr, cp)) = &self.carry {
                        let side = (2 * cr + 1) as usize;
                        let (ci, cj) = (x - (px.round() as i32 - cr), y - (py.round() as i32 - cr));
                        if ci >= 0 && cj >= 0 && (ci as usize) < side && (cj as usize) < side {
                            let cv = cp[cj as usize * side + ci as usize];
                            for c in 0..4 {
                                src[c] = src[c] * (1.0 - mix_eff) + cv[c] * mix_eff;
                            }
                        }
                    }
                    let max_ink = preset.max_ink;
                    // tinta que mistura com teto: o carimbo troca a cor, mas não sobe o alfa além do dele (a densidade
                    // do traço do Clip Studio); as canetas (`max_ink` sem mistura) ficam com o maior alfa
                    let ceiling = max_ink && (paint.is_some() || blur_src.is_some() || self.carry.is_some());
                    let d = self.buf_mut(x, y);
                    if ceiling {
                        let k = 1.0 - da;
                        let out = [0, 1, 2, 3].map(|c| src[c] * da + d[c] * k);
                        let cap = d[3].max(da);
                        let s = if out[3] > cap && out[3] > 0.0 { cap / out[3] } else { 1.0 };
                        for c in 0..4 {
                            d[c] = out[c] * s;
                        }
                    } else if max_ink {
                        if da > d[3] {
                            for c in 0..4 {
                                d[c] = src[c] * da;
                            }
                        }
                    } else {
                        let k = 1.0 - da;
                        for c in 0..4 {
                            d[c] = src[c] * da + d[c] * k;
                        }
                    }
                    let a = d[3];
                    self.max_a = self.max_a.max(a);
                }
            }
        }
    }

    /// Cor (pré-multiplicada) sob o pixel: o traço em andamento por cima da camada de antes do traço.
    fn under(&self, layer: Option<&Image>, x: i32, y: i32) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return [0.0; 4];
        }
        let l = layer.filter(|l| (x as u32) < l.width && (y as u32) < l.height).map(|l| l.get(x as u32, y as u32)).unwrap_or([0.0; 4]);
        let b = self.buf_get(x, y);
        [0, 1, 2, 3].map(|c| b[c] + l[c] * (1.0 - b[3]))
    }

    /// Cor (pré-multiplicada) da camada de antes do traço sob o pixel.
    fn layer_px(&self, layer: Option<&Image>, x: i32, y: i32) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return [0.0; 4];
        }
        layer.filter(|l| (x as u32) < l.width && (y as u32) < l.height).map(|l| l.get(x as u32, y as u32)).unwrap_or([0.0; 4])
    }

    /// Cor média (pré-multiplicada) da camada de antes do traço no disco da ponta (amostras em grade; `paint_mix` em
    /// modo média).
    fn sample_mean(&self, layer: Option<&Image>, px: f32, py: f32, r: f32) -> [f32; 4] {
        let ri = r.ceil() as i32;
        let step = ((r / 6.0).floor() as i32).max(1);
        let (cx, cy) = (px.round() as i32, py.round() as i32);
        let (mut acc, mut n) = ([0.0f32; 4], 0.0f32);
        let mut j = -ri;
        while j <= ri {
            let mut i = -ri;
            while i <= ri {
                if ((i * i + j * j) as f32) <= r * r {
                    let u = self.layer_px(layer, cx + i, cy + j);
                    for c in 0..4 {
                        acc[c] += u[c];
                    }
                    n += 1.0;
                }
                i += step;
            }
            j += step;
        }
        if n > 0.0 { acc.map(|v| v / n) } else { [0.0; 4] }
    }

    /// O que está sob a ponta, desfocado (`blur` px, três passadas de caixa ≈ gaussiana): origem, lado e pixels
    /// pré-multiplicados do quadrado que cobre a ponta (ferramenta Desfocar).
    fn blurred_area(&self, layer: Option<&Image>, px: f32, py: f32, r: f32, blur: f32) -> ([i32; 2], usize, Vec<[f32; 4]>) {
        let br = blur.ceil() as i32;
        let ri = (r + 2.0).ceil() as i32;
        let outer = ri + 2 * br;
        let (ox, oy) = (px.round() as i32 - outer, py.round() as i32 - outer);
        let side = (2 * outer + 1) as usize;
        let mut ch: Vec<Vec<f32>> = vec![vec![0.0; side * side]; 4];
        for j in 0..side {
            for i in 0..side {
                let u = self.under(layer, ox + i as i32, oy + j as i32);
                for c in 0..4 {
                    ch[c][j * side + i] = u[c];
                }
            }
        }
        let pass = ((blur / 1.7).round() as i32).max(1);
        for c in ch.iter_mut() {
            for _ in 0..3 {
                *c = box_blur(c, side, side, pass);
            }
        }
        let inner = (2 * ri + 1) as usize;
        let off = (outer - ri) as usize;
        let mut out = vec![[0.0f32; 4]; inner * inner];
        for j in 0..inner {
            for i in 0..inner {
                let k = (j + off) * side + i + off;
                out[j * inner + i] = [ch[0][k], ch[1][k], ch[2][k], ch[3][k]];
            }
        }
        ([ox + off as i32, oy + off as i32], inner, out)
    }

    /// Caixa [x0, y0, x1, y1) dos tiles que o traço tocou (coordenadas locais), presa à camada.
    pub fn bounds(&self) -> Option<[i32; 4]> {
        if self.tiles.is_empty() {
            return None;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for (tx, ty) in self.tiles.keys() {
            x0 = x0.min(tx * STS);
            y0 = y0.min(ty * STS);
            x1 = x1.max((tx + 1) * STS);
            y1 = y1.max((ty + 1) * STS);
        }
        let b = [x0.max(0), y0.max(0), x1.min(self.w), y1.min(self.h)];
        (b[0] < b[2] && b[1] < b[3]).then_some(b)
    }

    /// Compõe o traço em `img` (um recorte da camada que começa em `img_origin`, coordenadas locais) com a opacidade
    /// e o modo do pincel, depois do pincel duplo e das bordas molhadas. Devolve a caixa composta (local).
    pub fn composite(&self, img: &mut Image, img_origin: [i32; 2]) -> Option<PaintResult> {
        let b = self.bounds()?;
        self.composite_in(img, img_origin, b, None)
    }

    /// Como [`composite`](Self::composite), só no retângulo `rect` [x0, y0, x1, y1) (prévia ao vivo: só o que mudou).
    /// O nível da borda molhada é o do traço inteiro (sem pincel duplo, exato; com ele, aproximado pelo alfa antes
    /// da máscara da segunda ponta).
    pub fn composite_rect(&self, img: &mut Image, img_origin: [i32; 2], rect: [i32; 4]) -> Option<PaintResult> {
        self.composite_in(img, img_origin, rect, Some(self.max_a))
    }

    fn composite_in(&self, img: &mut Image, img_origin: [i32; 2], area: [i32; 4], level: Option<f32>) -> Option<PaintResult> {
        let [bx0, by0, bx1, by1] = area;
        if self.tiles.is_empty() || bx0 >= bx1 || by0 >= by1 {
            return None;
        }
        let preset = &self.preset;
        // borda molhada desfoca a máscara até `rad` px: a região densa ganha essa margem (presa à camada)
        let rad = if preset.wet_width > 0.0 { preset.wet_width.round().clamp(1.0, 64.0) as i32 } else { (preset.size * 0.12).clamp(1.0, 12.0) as i32 };
        let m = if preset.wet_edges > 0.0 { rad } else { 0 };
        let (x0, y0, x1, y1) = ((bx0 - m).max(0), (by0 - m).max(0), (bx1 + m).min(self.w), (by1 + m).min(self.h));
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let (bw, bh) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut buf = vec![[0.0f32; 4]; bw * bh];
        for ((tx, ty), t) in &self.tiles {
            for j in 0..STS {
                let y = ty * STS + j;
                if y < y0 || y >= y1 {
                    continue;
                }
                for i in 0..STS {
                    let x = tx * STS + i;
                    if x < x0 || x >= x1 {
                        continue;
                    }
                    buf[(y - y0) as usize * bw + (x - x0) as usize] = t[(j * STS + i) as usize];
                }
            }
        }
        if let Some(db) = &self.dual {
            for y in y0..y1 {
                for x in x0..x1 {
                    let mval = db.get(&(x.div_euclid(STS), y.div_euclid(STS))).map(|t| t[(y.rem_euclid(STS) * STS + x.rem_euclid(STS)) as usize]).unwrap_or(0.0);
                    for c in buf[(y - y0) as usize * bw + (x - x0) as usize].iter_mut() {
                        *c *= mval;
                    }
                }
            }
        }
        // bordas molhadas: a poça tem borda definida e o pigmento se acumula na borda de FORA (como a aquarela seca).
        if preset.wet_edges > 0.0 {
            let level = level.unwrap_or_else(|| buf.iter().map(|p| p[3]).fold(0.0f32, f32::max));
            let smooth = |a: f32| {
                let t = ((a / level.max(1e-6) - 0.08) / 0.30).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            };
            let mask: Vec<f32> = buf.iter().map(|p| smooth(p[3])).collect();
            let blurred = box_blur(&mask, bw, bh, rad);
            for (i, p) in buf.iter_mut().enumerate() {
                if p[3] <= 0.0 {
                    continue;
                }
                let edge = ((mask[i] - blurred[i]).max(0.0) * 3.0).min(1.0);
                let factor = (1.0 - preset.wet_edges * 0.5) + preset.wet_edges * 0.8 * edge;
                let na = (mask[i] * level * factor).min(1.0);
                let s = if p[3] > 0.0 { na / p[3] } else { 0.0 };
                // escurecer a borda: a cor (pré-multiplicada) cai, o alfa fica
                let dk = 1.0 - preset.wet_darkness * 0.45 * edge;
                for c in p[..3].iter_mut() {
                    *c *= s * dk;
                }
                p[3] *= s;
            }
        }
        let mode = match preset.mode {
            BrushMode::Normal => BlendMode::Normal,
            BrushMode::Behind => BlendMode::Behind,
            BrushMode::Multiply => BlendMode::Multiply,
            BrushMode::Screen => BlendMode::Screen,
            BrushMode::Overlay => BlendMode::Overlay,
            BrushMode::Add => BlendMode::Add,
            BrushMode::Darken => BlendMode::Darken,
            BrushMode::Lighten => BlendMode::Lighten,
            BrushMode::SoftLight => BlendMode::SoftLight,
            BrushMode::HardLight => BlendMode::HardLight,
            BrushMode::Subtract => BlendMode::Subtract,
            BrushMode::Difference => BlendMode::Difference,
            BrushMode::Hue => BlendMode::Hue,
            BrushMode::Saturation => BlendMode::Saturation,
            BrushMode::Color => BlendMode::Color,
            BrushMode::Luminosity => BlendMode::Luminosity,
            BrushMode::Erase => BlendMode::Normal,
        };
        let (iw, ih) = (img.width as i32, img.height as i32);
        let (wx0, wy0, wx1, wy1) = if level.is_some() { (bx0.max(x0), by0.max(y0), bx1.min(x1), by1.min(y1)) } else { (x0, y0, x1, y1) };
        for y in wy0..wy1 {
            let iy = y - img_origin[1];
            if iy < 0 || iy >= ih {
                continue;
            }
            for x in wx0..wx1 {
                let ix = x - img_origin[0];
                if ix < 0 || ix >= iw {
                    continue;
                }
                let s = buf[(y - y0) as usize * bw + (x - x0) as usize];
                if let Some(t) = &self.touched {
                    // troca: o pixel já traz a camada de baixo; a opacidade do pincel mistura com o original
                    let hit = t.get(&(x.div_euclid(STS), y.div_euclid(STS))).is_some_and(|v| v[(y.rem_euclid(STS) * STS + x.rem_euclid(STS)) as usize]);
                    if hit {
                        let i = img.idx(ix as u32, iy as u32);
                        let op = preset.opacity.clamp(0.0, 1.0);
                        for c in 0..4 {
                            img.data[i][c] = img.data[i][c] * (1.0 - op) + s[c] * op;
                        }
                    }
                    continue;
                }
                if s[3] <= 0.0 {
                    continue;
                }
                let i = img.idx(ix as u32, iy as u32);
                if preset.mode == BrushMode::Erase {
                    let k = 1.0 - s[3] * preset.opacity;
                    for c in img.data[i].iter_mut() {
                        *c *= k;
                    }
                } else {
                    blend_pixel(&mut img.data[i], s, mode, preset.opacity);
                }
            }
        }
        Some(PaintResult { bounds: [wx0, wy0, wx1, wy1] })
    }
}

fn box_blur(src: &[f32], w: usize, h: usize, r: i32) -> Vec<f32> {
    let mut tmp = vec![0.0f32; w * h];
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = ((x as i32 - r).max(0) as usize, ((x as i32 + r) as usize).min(w - 1));
            let s: f32 = src[y * w + a..=y * w + b].iter().sum();
            tmp[y * w + x] = s / (b - a + 1) as f32;
        }
    }
    for x in 0..w {
        for y in 0..h {
            let (a, b) = ((y as i32 - r).max(0) as usize, ((y as i32 + r) as usize).min(h - 1));
            let mut s = 0.0;
            for yy in a..=b {
                s += tmp[yy * w + x];
            }
            out[y * w + x] = s / (b - a + 1) as f32;
        }
    }
    out
}

/// Degradê linear/radial/angular/diamante entre dois pontos do canvas, paradas sRGB (t, cor não pré-multiplicada),
/// preso nas pontas: a ferramenta Degradê de sempre, agora pelo avaliador único de `aiai_core::paint`.
pub fn gradient_fill(layer: &mut Image, kind: GradientKind, p0: [f32; 2], p1: [f32; 2], stops: &[(f32, [f32; 4])], mode: BlendMode, opacity: f32) {
    if stops.is_empty() {
        return;
    }
    let g = aiai_core::ResolvedGradient::simple(stops, kind.shape(), p0, p1);
    let bbox = [0.0, 0.0, layer.width as f32, layer.height as f32];
    gradient_fill_resolved(layer, &g, bbox, mode, opacity);
}

/// Pinta a camada inteira com um degradê resolvido (geometria relativa a `bbox` ou absoluta).
pub fn gradient_fill_resolved(layer: &mut Image, g: &aiai_core::ResolvedGradient, bbox: [f32; 4], mode: BlendMode, opacity: f32) {
    if g.stops.is_empty() {
        return;
    }
    let m = g.mapper(bbox);
    let wdt = layer.width as usize;
    for (i, px) in layer.data.iter_mut().enumerate() {
        let (x, y) = ((i % wdt) as f32 + 0.5, (i / wdt) as f32 + 0.5);
        let c = g.sample(&m, [x, y]);
        blend_pixel(px, [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]], mode, opacity);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GradientKind {
    Linear,
    Radial,
    /// Cônico em volta de `from`, começando na direção de `to`.
    Angular,
    /// Losango em volta de `from` (raio = distância até `to`).
    Diamond,
}

impl GradientKind {
    pub fn shape(self) -> aiai_core::GradientShape {
        match self {
            GradientKind::Linear => aiai_core::GradientShape::Linear,
            GradientKind::Radial => aiai_core::GradientShape::Radial,
            GradientKind::Angular => aiai_core::GradientShape::Angular,
            GradientKind::Diamond => aiai_core::GradientShape::Diamond,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: usize) -> Vec<BrushPoint> {
        (0..n).map(|i| BrushPoint { x: 20.0 + i as f32 * 4.0, y: 50.0, pressure: 1.0, tilt: None }).collect()
    }

    #[test]
    fn deterministic_and_respects_opacity_cap() {
        let mut p = BrushPreset::round("t", 20.0, 0.9);
        p.opacity = 0.5;
        p.flow = 0.3;
        let mut a = Image::new(200, 100);
        let mut b = Image::new(200, 100);
        paint_stroke(&mut a, &p, &line(40), [1.0, 0.0, 0.0, 1.0], 7).unwrap();
        paint_stroke(&mut b, &p, &line(40), [1.0, 0.0, 0.0, 1.0], 7).unwrap();
        assert_eq!(a.content_hash(), b.content_hash());
        let max_a = a.data.iter().map(|q| q[3]).fold(0.0, f32::max);
        assert!(max_a <= 0.5 + 1e-4 && max_a > 0.45, "opacidade do traço limita o acúmulo: {max_a}");
    }

    #[test]
    fn eraser_removes_and_texture_varies_alpha() {
        let mut img = Image::filled(200, 100, [0.2, 0.4, 0.6, 1.0]);
        let mut e = BrushPreset::round("e", 20.0, 1.0);
        e.mode = BrushMode::Erase;
        paint_stroke(&mut img, &e, &line(30), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
        assert!(img.get(60, 50)[3] < 0.01);
        let mut t = BrushPreset::round("t", 30.0, 1.0);
        t.texture = Some(BrushTexture { kind: TextureKind::Grain, scale: 2.0, depth: 1.0, mode: TextureMode::Multiply, seed: 1, invert: false, rotation: 0.0, image: None, image_mask: None });
        let mut img2 = Image::new(200, 100);
        paint_stroke(&mut img2, &t, &line(30), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
        let vals: Vec<f32> = (40..80).map(|x| img2.get(x, 50)[3]).collect();
        let mn = vals.iter().cloned().fold(1.0, f32::min);
        let mx = vals.iter().cloned().fold(0.0, f32::max);
        assert!(mx - mn > 0.2, "textura modula o alfa: {mn}..{mx}");
    }

    #[test]
    fn eraser_is_not_a_factory_preset_but_still_a_valid_eraser() {
        // S25: as borrachas de fábrica são pincéis do grupo borracha (com nome próprio); o preset "Borracha" da
        // ferramenta continua fora da biblioteca, e nenhum pincel de outro grupo apaga
        let all = BrushPreset::defaults();
        assert!(all.iter().all(|p| p.name != "Borracha"));
        assert!(all.iter().all(|p| (p.mode == BrushMode::Erase) == (p.group == "borracha")));
        assert_eq!(all.iter().filter(|p| p.group == "borracha").count(), 8);
        let e = BrushPreset::eraser();
        e.validate().unwrap();
        assert_eq!((e.name.as_str(), e.mode), ("Borracha", BrushMode::Erase));
        let mut img = Image::filled(120, 60, [0.2, 0.4, 0.9, 1.0]);
        let before = img.content_hash();
        paint_stroke(&mut img, &e, &line(30), [0.0, 0.0, 0.0, 1.0], 1);
        assert_ne!(img.content_hash(), before, "apaga");
    }

    #[test]
    fn all_default_presets_paint_something() {
        for p in BrushPreset::defaults() {
            p.validate().unwrap();
            let mut img = Image::filled(220, 100, [0.9, 0.9, 0.9, 1.0]);
            let before = img.content_hash();
            paint_stroke(&mut img, &p, &line(40), [0.2, 0.3, 0.8, 1.0], 3);
            assert_ne!(img.content_hash(), before, "preset {} não mudou nada", p.name);
        }
    }

    #[test]
    fn watercolor_pigment_gathers_at_the_outer_edge() {
        let wc = BrushPreset::defaults().into_iter().find(|p| p.name == "Aquarela").unwrap();
        let mut plain = wc.clone();
        plain.texture = None;
        let mut img = Image::new(240, 120);
        paint_stroke(&mut img, &plain, &line(40), [0.2, 0.3, 0.8, 1.0], 3).unwrap();
        // corte vertical no meio do traço (y = 50 é o eixo; raio 25)
        let col: Vec<f32> = (20..80).map(|y| img.get(100, y)[3]).collect();
        let center = img.get(100, 50)[3];
        let peak = col.iter().cloned().fold(0.0, f32::max);
        let peak_y = 20 + col.iter().position(|&v| v == peak).unwrap() as i32;
        assert!(peak > center * 1.15, "borda mais carregada que o centro: pico {peak} x centro {center}");
        assert!((peak_y - 50).abs() >= 15, "o pico fica perto da borda de fora (raio 25), não no joelho de dentro: y {peak_y}");
        assert!(img.get(100, 50 + 30)[3] < 0.02, "fora da poça não há halo");
    }

    #[test]
    fn smudge_drags_pixel_structure_not_an_average_color() {
        // listras horizontais vermelho/azul de 6 px à esquerda; o borrador puxa da esquerda para a direita
        let mut img = Image::new(240, 120);
        for y in 0..120u32 {
            let c = if (y / 6) % 2 == 0 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] };
            for x in 0..100u32 {
                let i = img.idx(x, y);
                img.data[i] = c;
            }
        }
        let sm = BrushPreset::defaults().into_iter().find(|p| p.name == "Borrador").unwrap();
        let path: Vec<BrushPoint> = (0..40).map(|i| BrushPoint { x: 80.0 + i as f32 * 1.5, y: 60.0, pressure: 1.0, tilt: None }).collect();
        paint_stroke(&mut img, &sm, &path, [0.5, 0.5, 0.5, 1.0], 1).unwrap();
        // logo depois da borda da tinta, as linhas continuam distintas (uma média daria roxo igual nas duas)
        let (x, red_row, blue_row) = (106, 60, 66);
        let (a, b) = (img.get(x, red_row), img.get(x, blue_row));
        assert!(a[3] > 0.2 && b[3] > 0.2, "a tinta foi arrastada para fora: {a:?} {b:?}");
        let ra = a[0] / a[3].max(1e-6);
        let rb = b[0] / b[3].max(1e-6);
        assert!(ra - rb > 0.3, "linha vermelha continua mais vermelha que a azul: {ra} x {rb}");
    }

    /// Prancha de todos os presets (evidência visual): `AIAI_EVIDENCE_DIR=... cargo test -p aiai-paint brush_board -- --ignored`.
    #[test]
    #[ignore]
    fn brush_board() {
        let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") else { return };
        let presets = BrushPreset::defaults();
        let (cw, rh) = (560u32, 70u32);
        let rows = presets.len().div_ceil(2) as u32;
        let mut img = Image::filled(cw * 2, rh * rows, [0.97, 0.96, 0.93, 1.0]);
        // listras para o borrador ter o que arrastar
        for (k, p) in presets.iter().enumerate() {
            let (ox, oy) = ((k as u32 % 2) * cw, (k as u32 / 2) * rh);
            if p.mix > 0.5 {
                for y in oy + 10..oy + rh - 10 {
                    let c = if ((y - oy) / 6) % 2 == 0 { [0.85, 0.2, 0.2, 1.0] } else { [0.2, 0.3, 0.8, 1.0] };
                    for x in ox + 20..ox + 200 {
                        let i = img.idx(x, y);
                        img.data[i] = c;
                    }
                }
            }
            let path: Vec<BrushPoint> = (0..120)
                .map(|i| {
                    let t = i as f32 / 119.0;
                    BrushPoint { x: ox as f32 + 40.0 + t * 480.0, y: oy as f32 + rh as f32 / 2.0 + 14.0 * (t * 6.0).sin(), pressure: 0.3 + 0.7 * (t * 3.14).sin(), tilt: None }
                })
                .collect();
            paint_stroke(&mut img, p, &path, [0.15, 0.35, 0.6, 1.0], 9);
        }
        img.save_png(&std::path::Path::new(&dir).join("pinceis_prancha.png")).unwrap();
    }

    #[test]
    fn color_ramp_runs_along_the_stroke_length_and_feeds_in_chunks_the_same() {
        let mut p = BrushPreset::round("r", 10.0, 1.0);
        p.color_ramp = Some(ColorRamp { gradient: aiai_core::Gradient::from_colors(&[[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]], 0.0), length: 100.0 });
        let path: Vec<BrushPoint> = (0..=40).map(|i| BrushPoint { x: 20.0 + i as f32 * 4.0, y: 50.0, pressure: 1.0, tilt: None }).collect();
        let mut a = Image::new(220, 100);
        paint_stroke(&mut a, &p, &path, [0.0, 1.0, 0.0, 1.0], 3).unwrap();
        let un = |q: [f32; 4]| [q[0] / q[3].max(1e-6), q[1] / q[3].max(1e-6), q[2] / q[3].max(1e-6)];
        let start = un(a.get(21, 50));
        let mid = un(a.get(70, 50));
        let end = un(a.get(170, 50));
        assert!(start[0] > 0.85 && start[2] < 0.15, "começo vermelho: {start:?}");
        assert!(mid[0] > 0.3 && mid[2] > 0.3, "meio misturado: {mid:?}");
        assert!(end[2] > 0.95 && end[1] < 0.01, "depois de 100 px fica azul (pad), e a cor do pincel não entra: {end:?}");
        // aos pedaços: o mesmo resultado
        let mut sb = StrokeBuffer::new(&p, [0.0, 1.0, 0.0, 1.0], 3, 220, 100, [0, 0]);
        for chunk in path.chunks(7) {
            sb.add(None, chunk);
        }
        sb.finish(None);
        let mut b = Image::new(220, 100);
        sb.composite(&mut b, [0, 0]);
        let d = a.data.iter().zip(&b.data).map(|(x, y)| (0..4).map(|k| (x[k] - y[k]).abs()).fold(0.0, f32::max)).fold(0.0, f32::max);
        assert!(d < 1e-4, "pedaços = inteiro: {d}");
    }

    #[test]
    fn gradient_fill_kinds_cover_the_layer() {
        let stops = [(0.0, [0.0, 0.0, 0.0, 1.0]), (1.0, [1.0, 1.0, 1.0, 1.0])];
        for kind in [GradientKind::Linear, GradientKind::Radial, GradientKind::Angular, GradientKind::Diamond] {
            let mut img = Image::new(64, 64);
            gradient_fill(&mut img, kind, [32.0, 32.0], [64.0, 32.0], &stops, BlendMode::Normal, 1.0);
            assert!(img.data.iter().all(|q| q[3] > 0.999), "{kind:?} cobre tudo");
            let vals: Vec<f32> = img.data.iter().map(|q| q[0]).collect();
            let (mn, mx) = (vals.iter().cloned().fold(1.0, f32::min), vals.iter().cloned().fold(0.0, f32::max));
            assert!(mx - mn > 0.5, "{kind:?} varia: {mn}..{mx}");
        }
    }
}
