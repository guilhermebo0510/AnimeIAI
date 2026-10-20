//! Camada de texto editável: o texto e o estilo moram em `params.text` da camada (`TextParams`, versionado, com padrões
//! para campos que faltam) e o compositor refaz o layout e a vetorização a cada render, então o texto continua editável.
//!
//! Este módulo tem o modelo (`TextParams`), o **layout** (quebra de linha, alinhamento, entrelinha, rastreamento,
//! kerning da tabela `kern`, tategaki simples) e os contornos dos glifos. Pintar (preenchimento, contorno, sombra) é do
//! `aiai-render`.
//!
//! Espaço local do layout: a âncora (`pos`) é a origem. Texto horizontal corre em x, linhas descem em y. Texto vertical
//! (tategaki) corre em y, colunas vão da direita para a esquerda. Texto de ponto: `pos` é a borda esquerda, o centro ou a
//! borda direita da linha conforme o alinhamento (no vertical, a borda direita da primeira coluna). Texto de caixa:
//! `pos` é o canto superior esquerdo do retângulo `box_size`.

use crate::fonts::{self, LoadedFont};
use ab_glyph::{Font, FontRef, GlyphId, OutlineCurve};
use aiai_core::ColorRef;
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Versão atual do esquema de `params.text`.
pub const TEXT_PARAMS_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    /// Distribui as linhas da caixa de ponta a ponta (a última linha de cada parágrafo fica à esquerda).
    Justify,
}

/// Contorno do texto, por fora das letras.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct TextStroke {
    /// Espessura visível em px do canvas.
    pub width: f32,
    pub color: ColorRef,
}

impl Default for TextStroke {
    fn default() -> Self {
        TextStroke { width: 4.0, color: ColorRef::Direct { rgba: aiai_core::Rgba::WHITE } }
    }
}

/// Sombra projetada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct TextShadow {
    /// Deslocamento (x, y) em px do canvas.
    pub offset: [f32; 2],
    /// Desfoque (raio gaussiano em px); 0 = sombra dura.
    pub blur: f32,
    /// Cor com alfa (a opacidade da sombra é o alfa).
    pub color: ColorRef,
}

impl Default for TextShadow {
    fn default() -> Self {
        TextShadow { offset: [4.0, 4.0], blur: 4.0, color: ColorRef::Direct { rgba: aiai_core::Rgba([0.0, 0.0, 0.0, 0.6]) } }
    }
}

/// Texto e estilo de uma camada de texto (`LayerData.params["text"]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct TextParams {
    /// Versão do esquema (`TEXT_PARAMS_VERSION`).
    pub v: u32,
    pub content: String,
    /// Família da fonte (instalada, da pasta do usuário ou "Noto Sans JP", embutida). Vazio = a embutida.
    pub font: String,
    pub bold: bool,
    pub italic: bool,
    /// Tamanho do corpo em px do canvas (altura do em, como no CSS e no Photoshop).
    pub size: f32,
    pub color: ColorRef,
    /// Rastreamento: espaço extra entre letras em milésimos de em (0 = o da fonte; 100 = 0,1 em).
    pub tracking: f32,
    /// Entrelinha: distância entre linhas como múltiplo do tamanho (1,25 = 125%).
    pub leading: f32,
    pub align: TextAlign,
    /// Tategaki: colunas verticais da direita para a esquerda.
    pub vertical: bool,
    /// Âncora no canvas (ver o cabeçalho do módulo).
    pub pos: [f32; 2],
    /// Texto de caixa: largura e altura do retângulo (quebra automática dentro dele). `None` = texto de ponto.
    pub box_size: Option<[f32; 2]>,
    /// Graus (horário) em torno da âncora.
    pub rotation: f32,
    pub stroke: Option<TextStroke>,
    pub shadow: Option<TextShadow>,
}

impl Default for TextParams {
    fn default() -> Self {
        TextParams {
            v: TEXT_PARAMS_VERSION,
            content: String::new(),
            font: fonts::DEFAULT_FAMILY.to_owned(),
            bold: false,
            italic: false,
            size: 48.0,
            color: ColorRef::default(),
            tracking: 0.0,
            leading: 1.25,
            align: TextAlign::Left,
            vertical: false,
            pos: [0.0, 0.0],
            box_size: None,
            rotation: 0.0,
            stroke: None,
            shadow: None,
        }
    }
}

impl TextParams {
    /// Chave em `LayerData.params`.
    pub const KEY: &'static str = "text";

    /// Os parâmetros de texto de uma camada: `None` se `params` não tem `text`; `Err` se está malformado.
    pub fn from_layer_params(params: &serde_json::Value) -> Option<Result<TextParams, String>> {
        let v = params.get(Self::KEY).filter(|v| !v.is_null())?;
        Some(serde_json::from_value::<TextParams>(v.clone()).map(|p| p.sanitized()).map_err(|e| format!("params.text inválido: {e}")))
    }

    /// Valor completo (todos os campos, os opcionais ausentes como `null`): serve para um JSON Merge Patch em
    /// `params` que SUBSTITUI o texto inteiro (um `null` apaga a chave antiga, que volta ao padrão).
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// O patch de `LayerData` que grava estes parâmetros.
    pub fn to_layer_patch(&self) -> serde_json::Value {
        serde_json::json!({ "params": { Self::KEY: self.to_value() } })
    }

    /// Valores dentro de limites que o layout aguenta.
    pub fn sanitized(mut self) -> TextParams {
        let fin = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.size = fin(self.size, 48.0).clamp(1.0, 4000.0);
        self.leading = fin(self.leading, 1.25).clamp(0.2, 10.0);
        self.tracking = fin(self.tracking, 0.0).clamp(-500.0, 5000.0);
        self.rotation = fin(self.rotation, 0.0);
        self.pos = [fin(self.pos[0], 0.0), fin(self.pos[1], 0.0)];
        self.box_size = self.box_size.map(|b| [fin(b[0], 100.0).clamp(1.0, 100_000.0), fin(b[1], 100.0).clamp(1.0, 100_000.0)]);
        if let Some(s) = &mut self.stroke {
            s.width = fin(s.width, 0.0).clamp(0.0, 400.0);
        }
        if let Some(s) = &mut self.shadow {
            s.offset = [fin(s.offset[0], 0.0), fin(s.offset[1], 0.0)];
            s.blur = fin(s.blur, 0.0).clamp(0.0, 400.0);
        }
        self
    }

    /// Transformação local → canvas: rotação (horária, em graus) em torno da âncora e depois a translação até `pos`.
    pub fn local_to_canvas(&self, p: [f32; 2]) -> [f32; 2] {
        let (s, c) = self.rotation.to_radians().sin_cos();
        [self.pos[0] + p[0] * c - p[1] * s, self.pos[1] + p[0] * s + p[1] * c]
    }

    /// Inversa de `local_to_canvas`.
    pub fn canvas_to_local(&self, p: [f32; 2]) -> [f32; 2] {
        let (s, c) = self.rotation.to_radians().sin_cos();
        let (dx, dy) = (p[0] - self.pos[0], p[1] - self.pos[1]);
        [dx * c + dy * s, -dx * s + dy * c]
    }
}

/// Um glifo posicionado.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    /// Índice do caractere em `content` (em chars).
    pub ch: usize,
    /// 0 = fonte do texto; 1 = reserva (Noto Sans JP) para o que a fonte não tem.
    pub face: u8,
    pub id: u16,
    /// Posição do glifo no eixo do fluxo (x no horizontal, topo da célula em y no vertical).
    pub pen: f32,
    /// Extensão no eixo do fluxo (avanço + kerning + rastreamento).
    pub adv: f32,
    /// Origem do glifo (linha de base) no espaço local.
    pub origin: [f32; 2],
    /// Tategaki: o glifo gira 90° no sentido horário dentro da célula (ー, parênteses, reticências...).
    pub rot90: bool,
    /// Tategaki: centro da célula, em torno do qual `rot90` gira.
    pub cell_center: [f32; 2],
}

/// Uma linha (ou coluna, no vertical).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Chars de `content`: de `start` até `end` (sem o `\n` que fecha o parágrafo).
    pub start: usize,
    pub end: usize,
    pub glyphs: Range<usize>,
    /// Horizontal: y do topo da linha. Vertical: x da borda direita da coluna.
    pub cross: f32,
    /// Altura da linha (horizontal) ou largura da coluna (vertical): a entrelinha.
    pub cross_size: f32,
    /// Onde o conteúdo começa e termina no eixo do fluxo.
    pub flow_start: f32,
    pub flow_end: f32,
    /// Linha de base (horizontal: y; vertical: x do centro da coluna).
    pub baseline: f32,
}

#[derive(Debug, Clone)]
pub struct TextLayout {
    pub glyphs: Vec<Glyph>,
    pub lines: Vec<Line>,
    /// Caixa no espaço local [x0, y0, x1, y1]: a da caixa de parágrafo, ampliada pelo que ultrapassa; no texto de ponto,
    /// a do texto.
    pub bounds: [f32; 4],
    pub vertical: bool,
    pub size: f32,
    /// Número de chars do conteúdo (para o cursor).
    pub char_count: usize,
    faces: Vec<LoadedFont>,
}

struct Atom {
    ch: usize,
    c: char,
    face: u8,
    id: GlyphId,
    /// Avanço horizontal da fonte (sem rastreamento), para centrar glifos no tategaki.
    hadv: f32,
    /// Extensão no fluxo: avanço + kerning + rastreamento.
    adv: f32,
    is_space: bool,
}

fn is_cjk(c: char) -> bool {
    let u = c as u32;
    matches!(u, 0x2E80..=0x9FFF | 0xA000..=0xA4CF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x2FFFF)
}

/// Não pode começar linha (kinsoku): pontuação de fechamento, sinais finais, kana pequeno, prolongamento.
fn no_line_start(c: char) -> bool {
    "、。，．・：；？！ー）」』】〕〉》〗ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ々ゝゞヽヾ,.:;?!)]}".contains(c)
}

/// Não pode terminar linha: pontuação de abertura.
fn no_line_end(c: char) -> bool {
    "（「『【〔〈《〖([{".contains(c)
}

/// No tategaki, giram 90° (a forma vertical delas é a horizontal girada).
fn rotates_in_vertical(c: char) -> bool {
    "ー〜～…‥—―－–()（）[]［］「」『』【】〔〕〈〉《》{}｛｝<>＜＞=＝".contains(c)
}

/// No tategaki, vão para o canto superior direito da célula.
fn corner_punct(c: char) -> bool {
    "、。，．､｡".contains(c)
}

fn metrics<F: Font>(f: &F) -> (f32, f32, f32) {
    let upem = f.units_per_em().filter(|u| *u > 0.0).unwrap_or_else(|| f.height_unscaled().max(1.0));
    (upem, f.ascent_unscaled(), f.descent_unscaled())
}

/// Layout com as fontes escolhidas por `fonts::load`.
pub fn layout(p: &TextParams) -> TextLayout {
    let primary = fonts::load(&p.font, p.bold, p.italic);
    layout_with(p, primary)
}

/// Layout com a fonte primária dada (a reserva é sempre a Noto Sans JP embutida).
pub fn layout_with(p: &TextParams, primary: LoadedFont) -> TextLayout {
    let p = p.clone().sanitized();
    let fallback = fonts::embedded(false);
    let faces = vec![primary, fallback];
    let size = p.size;
    let track = p.tracking / 1000.0 * size;
    let parsed: Vec<Option<FontRef>> = faces.iter().map(|f| f.data.font()).collect();
    // se a primária não abre, a reserva assume (nunca sem fonte)
    let p0 = parsed[0].as_ref().or(parsed[1].as_ref());
    let (upem0, asc0, desc0) = p0.map(metrics).unwrap_or((1000.0, 880.0, -120.0));
    let k0 = size / upem0;
    let (asc, desc) = (asc0 * k0, -desc0 * k0);

    // ---- átomos por parágrafo
    let mut paras: Vec<Vec<Atom>> = vec![Vec::new()];
    let mut para_starts: Vec<usize> = vec![0];
    let mut count = 0usize;
    for (ci, c) in p.content.chars().enumerate() {
        count = ci + 1;
        if c == '\n' {
            paras.push(Vec::new());
            para_starts.push(ci + 1);
            continue;
        }
        if c == '\r' {
            // CRLF: o \r conta como char (o índice do cursor segue o texto), mas não tem largura
            paras.last_mut().unwrap().push(Atom { ch: ci, c, face: 0, id: GlyphId(0), hadv: 0.0, adv: 0.0, is_space: true });
            continue;
        }
        let draw_c = if c == '\t' { ' ' } else { c };
        let want_face = {
            let has0 = parsed[0].as_ref().is_some_and(|f| f.glyph_id(draw_c).0 != 0);
            let has1 = parsed[1].as_ref().is_some_and(|f| f.glyph_id(draw_c).0 != 0);
            if has0 || !has1 { 0 } else { 1 }
        };
        let face_idx = if parsed[want_face].is_some() { want_face } else { 1 - want_face };
        let font = parsed[face_idx].as_ref();
        let (id, hadv) = match font {
            Some(f) => {
                let (upem, _, _) = metrics(f);
                let id = f.glyph_id(draw_c);
                (id, f.h_advance_unscaled(id) * size / upem)
            }
            None => (GlyphId(0), size * 0.5),
        };
        let reps = if c == '\t' { 4 } else { 1 };
        let hadv = hadv * reps as f32;
        let atoms = paras.last_mut().unwrap();
        // kerning com o glifo anterior (mesma face): entra no avanço do anterior
        if !p.vertical {
            if let (Some(prev), Some(f)) = (atoms.last_mut(), font) {
                if prev.face as usize == face_idx && prev.id.0 != 0 && id.0 != 0 {
                    let (upem, _, _) = metrics(f);
                    prev.adv += f.kern_unscaled(prev.id, id) * size / upem;
                }
            }
        }
        let adv = if p.vertical { size + track } else { hadv + track };
        atoms.push(Atom { ch: ci, c, face: face_idx as u8, id, hadv, adv, is_space: draw_c == ' ' });
    }

    // ---- quebra de linhas
    let flow_limit = p.box_size.map(|b| if p.vertical { b[1] } else { b[0] }).unwrap_or(f32::INFINITY);
    let mut line_atoms: Vec<(usize, Range<usize>, bool)> = Vec::new(); // (parágrafo, faixa, última do parágrafo)
    for (pi, atoms) in paras.iter().enumerate() {
        let n = atoms.len();
        if n == 0 {
            line_atoms.push((pi, 0..0, true));
            continue;
        }
        let mut start = 0usize;
        let mut w_all = 0.0f32;
        let mut last_break: Option<usize> = None;
        let mut i = 0usize;
        while i < n {
            let a = &atoms[i];
            if !a.is_space && w_all + a.adv > flow_limit + 0.01 && i > start {
                let brk = match last_break {
                    Some(b) if b > start => b,
                    _ => i,
                };
                // kinsoku por empurrão: o fechamento que sobraria no começo da linha de baixo fica na de cima
                let brk = if brk < n && no_line_start(atoms[brk].c) && brk > start + 1 && brk == i { brk - 1 } else { brk };
                line_atoms.push((pi, start..brk, false));
                start = brk;
                w_all = 0.0;
                last_break = None;
                // remede o que já estava na nova linha
                for j in start..i {
                    w_all += atoms[j].adv;
                    last_break = break_after(atoms, j, start).or(last_break);
                }
                // o átomo `i` é reavaliado na nova linha
                continue;
            }
            w_all += a.adv;
            last_break = break_after(atoms, i, start).or(last_break);
            i += 1;
        }
        line_atoms.push((pi, start..n, true));
    }

    // ---- posições
    let pitch = size * p.leading;
    let box_flow = p.box_size.map(|b| if p.vertical { b[1] } else { b[0] });
    let box_cross = p.box_size.map(|b| if p.vertical { b[0] } else { b[1] });
    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    for (li, (pi, range, last_of_para)) in line_atoms.iter().enumerate() {
        let atoms = &paras[*pi][range.clone()];
        // extensão do conteúdo sem os espaços e o rastreamento finais
        let mut used = atoms.len();
        while used > 0 && atoms[used - 1].is_space && atoms[used - 1].c != '\r' {
            used -= 1;
        }
        let mut content_w: f32 = atoms[..used].iter().map(|a| a.adv).sum();
        if used > 0 {
            content_w -= track;
        }
        let (flow_off, stretch_extra) = match (p.align, box_flow) {
            (TextAlign::Left, _) | (TextAlign::Justify, None) => (0.0, 0.0),
            (TextAlign::Center, Some(b)) => ((b - content_w) / 2.0, 0.0),
            (TextAlign::Right, Some(b)) => (b - content_w, 0.0),
            (TextAlign::Center, None) => (-content_w / 2.0, 0.0),
            (TextAlign::Right, None) => (-content_w, 0.0),
            (TextAlign::Justify, Some(b)) => (0.0, if *last_of_para { 0.0 } else { (b - content_w).max(0.0) }),
        };
        // justificar: reparte a sobra nos espaços (ou entre todas as letras, se não há espaço)
        let gaps: Vec<usize> = {
            let sp: Vec<usize> = (0..used.saturating_sub(1)).filter(|&j| atoms[j].is_space).collect();
            if !sp.is_empty() { sp } else { (0..used.saturating_sub(1)).collect() }
        };
        let per_gap = if stretch_extra > 0.0 && !gaps.is_empty() { stretch_extra / gaps.len() as f32 } else { 0.0 };
        let cross = if p.vertical { box_cross.unwrap_or(0.0) - li as f32 * pitch } else { li as f32 * pitch };
        let baseline = if p.vertical { cross - pitch / 2.0 } else { cross + (pitch - (asc + desc)) / 2.0 + asc };
        let gstart = glyphs.len();
        let mut pen = flow_off;
        let first_ch = atoms.first().map(|a| a.ch).unwrap_or(para_starts[*pi]);
        for (j, a) in atoms.iter().enumerate() {
            let mut g = Glyph { ch: a.ch, face: a.face, id: a.id.0, pen, adv: a.adv, origin: [0.0, 0.0], rot90: false, cell_center: [0.0, 0.0] };
            if p.vertical {
                let cell_top = pen;
                let cx = baseline;
                g.cell_center = [cx, cell_top + size / 2.0];
                g.origin = [cx - a.hadv / 2.0, cell_top + size * 0.88];
                if rotates_in_vertical(a.c) {
                    g.rot90 = true;
                } else if corner_punct(a.c) {
                    g.origin[0] += size * 0.5;
                    g.origin[1] -= size * 0.55;
                }
            } else {
                g.origin = [pen, baseline];
            }
            glyphs.push(g);
            pen += a.adv;
            if gaps.contains(&j) {
                pen += per_gap;
            }
        }
        let end_ch = atoms.last().map(|a| a.ch + 1).unwrap_or(first_ch);
        lines.push(Line { start: first_ch, end: end_ch, glyphs: gstart..glyphs.len(), cross, cross_size: pitch, flow_start: flow_off, flow_end: flow_off + content_w.max(0.0) + stretch_extra, baseline });
    }

    // ---- caixa
    let (mut fmin, mut fmax) = (f32::MAX, f32::MIN);
    for l in &lines {
        fmin = fmin.min(l.flow_start);
        fmax = fmax.max(l.flow_end.max(l.flow_start));
    }
    if fmin > fmax {
        (fmin, fmax) = (0.0, 0.0);
    }
    let ncross = lines.len().max(1) as f32 * pitch;
    let bounds = match (p.box_size, p.vertical) {
        (Some(b), false) => [0.0, 0.0, b[0].max(fmax), b[1].max(ncross)],
        (Some(b), true) => [(b[0] - ncross).min(0.0), 0.0, b[0], b[1].max(fmax)],
        (None, false) => [fmin, 0.0, fmax.max(fmin + 1.0), ncross],
        (None, true) => [-ncross, fmin, 0.0, fmax.max(fmin + 1.0)],
    };
    TextLayout { glyphs, lines, bounds, vertical: p.vertical, size, char_count: count, faces }
}

/// Posição (índice do átomo seguinte) onde uma quebra de linha é permitida depois do átomo `i`.
fn break_after(atoms: &[Atom], i: usize, line_start: usize) -> Option<usize> {
    let a = &atoms[i];
    let next = atoms.get(i + 1)?;
    if a.is_space {
        return Some(i + 1);
    }
    if (is_cjk(a.c) || is_cjk(next.c)) && !next.is_space && !no_line_start(next.c) && !no_line_end(a.c) && i + 1 > line_start {
        return Some(i + 1);
    }
    None
}

impl TextLayout {
    /// Número de linhas (ou colunas).
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// A linha que contém o cursor `c` (índice de char, 0..=char_count). Num ponto de quebra automática o cursor fica
    /// no começo da linha de baixo.
    pub fn line_of(&self, c: usize) -> usize {
        for (i, l) in self.lines.iter().enumerate() {
            if c < l.end {
                return i;
            }
            if c == l.end {
                let wrapped = self.lines.get(i + 1).is_some_and(|n| n.start == c);
                if !wrapped {
                    return i;
                }
            }
        }
        self.lines.len().saturating_sub(1)
    }

    /// Posição do cursor `c` no eixo do fluxo da linha.
    pub fn flow_of(&self, c: usize) -> f32 {
        let Some(l) = self.lines.get(self.line_of(c)) else { return 0.0 };
        if c <= l.start || l.glyphs.is_empty() {
            return l.flow_start;
        }
        if c >= l.end {
            return l.flow_end;
        }
        self.glyphs.get(l.glyphs.start + (c - l.start)).map(|g| g.pen).unwrap_or(l.flow_end)
    }

    /// Barra do cursor `c` no espaço local: [x0, y0, x1, y1].
    pub fn caret_rect(&self, c: usize) -> [f32; 4] {
        let Some(l) = self.lines.get(self.line_of(c)) else { return [0.0, 0.0, 0.0, self.size] };
        let f = self.flow_of(c);
        if self.vertical {
            [l.cross - l.cross_size, f, l.cross, f]
        } else {
            [f, l.cross, f, l.cross + l.cross_size]
        }
    }

    /// Retângulos (espaço local) cobrindo os chars `a..b` (seleção); um por linha.
    pub fn selection_rects(&self, a: usize, b: usize) -> Vec<[f32; 4]> {
        let (a, b) = (a.min(b), a.max(b));
        let mut out = Vec::new();
        if a == b {
            return out;
        }
        for l in &self.lines {
            // a seleção que atravessa o fim do parágrafo cobre também um pouco do "fim de linha"
            let covers_nl = b > l.end && a <= l.end && self.lines.iter().any(|n| n.start > l.end);
            let (s, e) = (a.max(l.start), b.min(l.end));
            if s >= e && !covers_nl {
                continue;
            }
            let f0 = if s >= l.end { l.flow_end } else { self.flow_of(s) };
            let mut f1 = if e >= l.end { l.flow_end } else { self.flow_of(e) };
            if covers_nl {
                f1 += self.size * 0.3;
            }
            if f1 < f0 {
                continue;
            }
            out.push(if self.vertical { [l.cross - l.cross_size, f0, l.cross, f1] } else { [f0, l.cross, f1, l.cross + l.cross_size] });
        }
        out
    }

    /// Cursor mais próximo de um ponto do espaço local.
    pub fn hit(&self, pt: [f32; 2]) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        let (cross_v, flow_v) = if self.vertical { (pt[0], pt[1]) } else { (pt[1], pt[0]) };
        // linha pelo eixo cruzado: a mais próxima
        let li = self
            .lines
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let d = |l: &Line| {
                    let (lo, hi) = if self.vertical { (l.cross - l.cross_size, l.cross) } else { (l.cross, l.cross + l.cross_size) };
                    if cross_v < lo { lo - cross_v } else if cross_v > hi { cross_v - hi } else { 0.0 }
                };
                d(a).total_cmp(&d(b))
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.char_at_flow(li, flow_v)
    }

    /// Cursor da linha `li` mais próximo da posição `flow` no eixo do fluxo.
    pub fn char_at_flow(&self, li: usize, flow: f32) -> usize {
        let Some(l) = self.lines.get(li) else { return 0 };
        for g in &self.glyphs[l.glyphs.clone()] {
            if flow < g.pen + g.adv / 2.0 {
                return g.ch;
            }
        }
        l.end
    }

    /// Move o cursor `c` uma linha para `dir` (-1 acima / +1 abaixo; no vertical, para a direita/esquerda), mantendo a
    /// posição no fluxo `flow` (guardada pelo editor entre movimentos).
    pub fn move_line(&self, c: usize, dir: i32, flow: f32) -> usize {
        let li = self.line_of(c) as i32 + dir;
        if li < 0 {
            return 0;
        }
        if li as usize >= self.lines.len() {
            return self.char_count;
        }
        self.char_at_flow(li as usize, flow)
    }

    /// Extensão do conteúdo no espaço local (sem a caixa): [x0, y0, x1, y1].
    pub fn text_extent(&self) -> [f32; 4] {
        let (mut f0, mut f1) = (f32::MAX, f32::MIN);
        for l in &self.lines {
            f0 = f0.min(l.flow_start);
            f1 = f1.max(l.flow_end);
        }
        if f0 > f1 {
            (f0, f1) = (0.0, 0.0);
        }
        let (c0, c1) = match (self.lines.first(), self.lines.last()) {
            (Some(a), Some(b)) if self.vertical => (b.cross - b.cross_size, a.cross),
            (Some(a), Some(b)) => (a.cross, b.cross + b.cross_size),
            _ => (0.0, self.size),
        };
        if self.vertical { [c0, f0, c1, f1] } else { [f0, c0, f1, c1] }
    }

    /// Contornos fechados dos glifos no canvas (regra não-zero), já com rotação e âncora de `p`.
    /// `detail`: px de saída por px do canvas (escolhe quantos segmentos achatam as curvas).
    pub fn contours(&self, p: &TextParams, detail: f32) -> Vec<Vec<[f32; 2]>> {
        let p = p.clone().sanitized();
        let parsed: Vec<Option<FontRef>> = self.faces.iter().map(|f| f.data.font()).collect();
        let mut out: Vec<Vec<[f32; 2]>> = Vec::new();
        let skew = if self.faces[0].synth_italic && !self.vertical { 0.2f32 } else { 0.0 };
        for g in &self.glyphs {
            if g.id == 0 {
                continue;
            }
            let Some(font) = parsed.get(g.face as usize).and_then(|f| f.as_ref()) else { continue };
            let Some(outline) = font.outline(GlyphId(g.id)) else { continue };
            let (upem, _, _) = metrics(font);
            let k = self.size / upem;
            let hadv = font.h_advance_unscaled(GlyphId(g.id)) * k;
            let center = [hadv / 2.0, -0.38 * self.size];
            let tp = |q: ab_glyph::Point| -> [f32; 2] {
                // glifo (y para cima, unidades da fonte) → local (y para baixo)
                let (mut gx, gy) = (q.x * k, -q.y * k);
                gx += skew * (-gy);
                let l = if g.rot90 {
                    // gira 90° horário em torno do centro do em e leva esse centro ao centro da célula
                    let (rx, ry) = (gx - center[0], gy - center[1]);
                    [g.cell_center[0] - ry, g.cell_center[1] + rx]
                } else {
                    [g.origin[0] + gx, g.origin[1] + gy]
                };
                p.local_to_canvas(l)
            };
            let seg_n = |pts: &[[f32; 2]]| -> usize {
                let len: f32 = pts.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt()).sum();
                ((len * detail / 3.0).ceil() as usize).clamp(3, 24)
            };
            let mut cur: Vec<[f32; 2]> = Vec::new();
            let mut last: Option<ab_glyph::Point> = None;
            for c in &outline.curves {
                let (start, end) = match c {
                    OutlineCurve::Line(a, b) => (*a, *b),
                    OutlineCurve::Quad(a, _, c2) => (*a, *c2),
                    OutlineCurve::Cubic(a, _, _, d) => (*a, *d),
                };
                let continuous = last.is_some_and(|l| (l.x - start.x).abs() < 1e-3 && (l.y - start.y).abs() < 1e-3);
                if !continuous {
                    if cur.len() >= 3 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(tp(start));
                }
                match c {
                    OutlineCurve::Line(_, b) => cur.push(tp(*b)),
                    OutlineCurve::Quad(a, b, c2) => {
                        let (p0, p1, p2) = (tp(*a), tp(*b), tp(*c2));
                        let n = seg_n(&[p0, p1, p2]);
                        for s in 1..=n {
                            let t = s as f32 / n as f32;
                            let u = 1.0 - t;
                            cur.push([u * u * p0[0] + 2.0 * u * t * p1[0] + t * t * p2[0], u * u * p0[1] + 2.0 * u * t * p1[1] + t * t * p2[1]]);
                        }
                    }
                    OutlineCurve::Cubic(a, b, c2, d) => {
                        let (p0, p1, p2, p3) = (tp(*a), tp(*b), tp(*c2), tp(*d));
                        let n = seg_n(&[p0, p1, p2, p3]);
                        for s in 1..=n {
                            let t = s as f32 / n as f32;
                            let u = 1.0 - t;
                            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                            cur.push([w0 * p0[0] + w1 * p1[0] + w2 * p2[0] + w3 * p3[0], w0 * p0[1] + w1 * p1[1] + w2 * p2[1] + w3 * p3[1]]);
                        }
                    }
                }
                last = Some(end);
            }
            if cur.len() >= 3 {
                out.push(cur);
            }
        }
        out
    }

    /// Negrito sintético: a fonte não tem a face e o render engrossa as letras.
    pub fn synth_bold(&self) -> bool {
        self.faces[0].synth_bold
    }

    /// Nome da família que o layout de fato usou (a pedida pode não existir e cair na embutida).
    pub fn family_used(&self) -> &str {
        &self.faces[0].family
    }
}

/// Caixa do texto no canvas [x0, y0, x1, y1] (envolve a caixa local girada): para clique, moldura e alças.
pub fn canvas_bounds(p: &TextParams, lay: &TextLayout) -> [f32; 4] {
    let b = lay.bounds;
    let pts = [[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]]].map(|q| p.local_to_canvas(q));
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for q in pts {
        x0 = x0.min(q[0]);
        y0 = y0.min(q[1]);
        x1 = x1.max(q[0]);
        y1 = y1.max(q[1]);
    }
    [x0, y0, x1, y1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(content: &str) -> TextParams {
        TextParams { content: content.into(), size: 40.0, ..Default::default() }
    }

    fn width(l: &TextLayout, i: usize) -> f32 {
        l.lines[i].flow_end - l.lines[i].flow_start
    }

    #[test]
    fn params_round_trip_and_defaults_fill_missing_fields() {
        let tp = TextParams { content: "Olá".into(), stroke: Some(TextStroke::default()), ..Default::default() };
        let v = tp.to_value();
        let back: TextParams = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(tp, back);
        // JSON antigo/parcial: só o texto; o resto vem do padrão
        let partial = serde_json::json!({"text": {"content": "x", "size": 20}});
        let q = TextParams::from_layer_params(&partial).unwrap().unwrap();
        assert_eq!((q.content.as_str(), q.size, q.v, q.leading), ("x", 20.0, TEXT_PARAMS_VERSION, 1.25));
        // opcionais ausentes viajam como null (o merge patch apaga a chave antiga)
        assert!(v.get("stroke").is_some() && v.get("shadow").is_some_and(|s| s.is_null()));
        assert!(TextParams::from_layer_params(&serde_json::json!({})).is_none());
        assert!(TextParams::from_layer_params(&serde_json::json!({"text": {"size": "grande"}})).unwrap().is_err());
    }

    #[test]
    fn newlines_make_lines_with_the_leading_as_pitch() {
        let l = layout(&TextParams { leading: 1.5, ..p("ab\ncd\n\nef") });
        assert_eq!(l.line_count(), 4);
        assert!((l.lines[1].cross - l.lines[0].cross - 60.0).abs() < 1e-3, "40 px × 1,5");
        assert_eq!(l.lines[2].start, l.lines[2].end, "linha vazia");
        assert_eq!((l.lines[0].start, l.lines[0].end), (0, 2));
        assert_eq!((l.lines[1].start, l.lines[1].end), (3, 5));
        assert_eq!(l.char_count, 9);
    }

    #[test]
    fn tracking_widens_every_gap_but_not_the_ends() {
        let a = layout(&p("abcd"));
        let b = layout(&TextParams { tracking: 500.0, ..p("abcd") }); // 0,5 em = 20 px por vão
        let extra = width(&b, 0) - width(&a, 0);
        assert!((extra - 60.0).abs() < 0.5, "3 vãos × 20 px, achou {extra}");
        assert!((b.glyphs[1].pen - a.glyphs[1].pen - 20.0).abs() < 0.01);
    }

    #[test]
    fn box_text_wraps_at_spaces_and_respects_the_width() {
        let single = layout(&p("hello world again"));
        let w = single.lines[0].flow_end;
        let boxed = layout(&TextParams { box_size: Some([w * 0.6, 400.0]), ..p("hello world again") });
        assert!(boxed.line_count() >= 2, "quebrou: {}", boxed.line_count());
        for l in &boxed.lines {
            assert!(l.flow_end - l.flow_start <= w * 0.6 + 0.5, "linha larga demais: {}", l.flow_end - l.flow_start);
        }
        // palavras inteiras: nenhuma linha começa no meio de "hello"
        assert_eq!(boxed.lines[0].end - boxed.lines[0].start, "hello ".len(), "quebra depois do espaço");
        // sem caixa não quebra
        assert_eq!(single.line_count(), 1);
    }

    #[test]
    fn long_word_breaks_by_character_and_cjk_breaks_anywhere() {
        let l = layout(&TextParams { box_size: Some([100.0, 400.0]), ..p("supercalifragilistico") });
        assert!(l.line_count() >= 2);
        let jp = layout(&TextParams { box_size: Some([100.0, 400.0]), ..p("日本語のテキストを折り返す") });
        assert!(jp.line_count() >= 3, "40 px cada: 2 letras por linha, achou {}", jp.line_count());
        // kinsoku: 、 e 。 não abrem linha
        let k = layout(&TextParams { box_size: Some([81.0, 400.0]), ..p("ああ、いい。うう") });
        for l in &k.lines[1..] {
            let first = "ああ、いい。うう".chars().nth(l.start).unwrap();
            assert!(first != '、' && first != '。', "linha começa com {first}");
        }
    }

    #[test]
    fn alignment_places_lines_inside_the_box() {
        let base = TextParams { box_size: Some([400.0, 300.0]), ..p("ab") };
        let left = layout(&TextParams { align: TextAlign::Left, ..base.clone() });
        let center = layout(&TextParams { align: TextAlign::Center, ..base.clone() });
        let right = layout(&TextParams { align: TextAlign::Right, ..base.clone() });
        let w = width(&left, 0);
        assert!(left.lines[0].flow_start.abs() < 1e-3);
        assert!((center.lines[0].flow_start - (400.0 - w) / 2.0).abs() < 0.01);
        assert!((right.lines[0].flow_end - 400.0).abs() < 0.01);
        // texto de ponto: a âncora fica no centro/borda direita
        let pc = layout(&TextParams { align: TextAlign::Center, ..p("ab") });
        assert!((pc.lines[0].flow_start + w / 2.0).abs() < 0.01);
        let pr = layout(&TextParams { align: TextAlign::Right, ..p("ab") });
        assert!(pr.lines[0].flow_end.abs() < 0.01);
    }

    #[test]
    fn justify_stretches_all_but_the_last_line() {
        let l = layout(&TextParams { box_size: Some([300.0, 400.0]), align: TextAlign::Justify, ..p("aa bb cc dd ee ff gg hh ii jj kk ll") });
        assert!(l.line_count() >= 2);
        for li in &l.lines[..l.line_count() - 1] {
            assert!((li.flow_end - 300.0).abs() < 0.5, "linha justificada vai até a borda: {}", li.flow_end);
        }
        assert!(l.lines.last().unwrap().flow_end < 300.0, "a última fica à esquerda");
    }

    #[test]
    fn caret_and_hit_test_agree() {
        let l = layout(&p("abc\ndef"));
        for c in 0..=7 {
            let r = l.caret_rect(c);
            let mid = [r[0] + 0.01, (r[1] + r[3]) / 2.0];
            let h = l.hit(mid);
            assert_eq!(h, c, "cursor {c} em {mid:?} voltou {h}");
        }
        assert_eq!(l.line_of(3), 0, "fim da linha 1");
        assert_eq!(l.line_of(4), 1);
        let down = l.move_line(1, 1, l.flow_of(1));
        assert_eq!(down, 5);
    }

    #[test]
    fn wrapped_boundary_caret_goes_to_the_lower_line() {
        let l = layout(&TextParams { box_size: Some([100.0, 400.0]), ..p("aaaa bbbb") });
        assert!(l.line_count() >= 2);
        let b = l.lines[0].end;
        assert_eq!(l.line_of(b), 1, "na quebra automática o cursor fica embaixo");
    }

    #[test]
    fn vertical_text_flows_down_and_columns_go_right_to_left() {
        let l = layout(&TextParams { vertical: true, box_size: Some([200.0, 90.0]), ..p("あいうえお") });
        assert!(l.line_count() >= 3, "90 px de altura: 2 letras por coluna, achou {}", l.line_count());
        assert!(l.glyphs[1].origin[1] > l.glyphs[0].origin[1], "desce");
        assert!(l.lines[1].cross < l.lines[0].cross, "a segunda coluna fica à esquerda");
        assert!((l.lines[0].cross - 200.0).abs() < 1e-3, "a primeira coluna encosta na borda direita da caixa");
        let c = layout(&TextParams { vertical: true, ..p("あい") });
        assert!(c.bounds[2].abs() < 1e-3, "texto de ponto: a âncora é a borda direita");
    }

    #[test]
    fn contours_land_where_the_layout_says() {
        let tp = TextParams { pos: [100.0, 50.0], size: 60.0, ..p("H") };
        let l = layout(&tp);
        let cs = l.contours(&tp, 1.0);
        assert!(!cs.is_empty());
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in &cs {
            for q in c {
                x0 = x0.min(q[0]);
                y0 = y0.min(q[1]);
                x1 = x1.max(q[0]);
                y1 = y1.max(q[1]);
            }
        }
        let b = canvas_bounds(&tp, &l);
        assert!(x0 >= b[0] - 0.5 && x1 <= b[2] + 0.5 && y0 >= b[1] - 0.5 && y1 <= b[3] + 0.5, "glifos dentro da caixa: {:?} vs {:?}", [x0, y0, x1, y1], b);
        assert!(x0 > 100.0 && y0 > 50.0, "à direita e abaixo da âncora");
        // rotação em torno da âncora: 90° leva a linha para baixo da âncora a ficar à esquerda dela
        let rot = TextParams { rotation: 90.0, ..tp.clone() };
        let rc = layout(&rot).contours(&rot, 1.0);
        let rx1 = rc.iter().flatten().map(|q| q[0]).fold(f32::MIN, f32::max);
        assert!(rx1 < 100.0 + 1.0, "girou 90° horário: tudo à esquerda da âncora, achou x1 = {rx1}");
    }

    #[test]
    fn missing_glyphs_fall_back_to_the_embedded_font() {
        // fonte inexistente cai na embutida; o texto não some
        let l = layout(&TextParams { font: "Fonte Que Nao Existe 123".into(), ..p("Aあ") });
        assert_eq!(l.family_used(), fonts::DEFAULT_FAMILY);
        assert!(l.glyphs.iter().all(|g| g.id != 0));
    }

    #[test]
    fn bold_picks_the_black_face_and_italic_is_synthetic() {
        let tp = TextParams { bold: true, italic: true, ..p("A") };
        let l = layout(&tp);
        assert!(!l.synth_bold());
        let reg = layout(&p("A"));
        assert!(width(&l, 0) != width(&reg, 0), "a face Black tem outro avanço");
        assert!(!l.contours(&tp, 1.0).is_empty());
    }
}
