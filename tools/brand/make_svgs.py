#!/usr/bin/env python3
"""Gera os SVG da marca do AnimeIAI (`app/crates/aiai-studio/assets/brand/`).

A marca é desenhada à mão (aqui como geometria paramétrica, sem IA generativa): um quadro de animação com perfurações
de filme e um "A" em pincelada no laranja do acento, com a cópia fantasma do "onion skin" atrás (o quadro anterior).
O wordmark é feito de traçados próprios (monolinha de ponta redonda, na mesma pincelada do A), sem fonte nem <text>: o
resvg do app é compilado sem suporte a texto.

Uso: python tools/brand/make_svgs.py     (reescreve os SVG; depois rode tools/brand/make_icons.py)

Arquivos (o fundo do quadrado acompanha a interface: -dark para UI escura, -light para UI clara):
  logo.svg, logo-dark.svg, logo-light.svg           marca completa (ícone do app a partir de 40 px)
  logo-small.svg, logo-small-light.svg              16 a 32 px: sem perfurações nem moldura
  logo-mono.svg                                     monocromática (currentColor), para tamanhos pequenos e recolorir
  logo-wordmark(|-dark|-light|-mono).svg            marca + "AnimeIAI"
  hinted/logo-<N>[-light].svg                       16/20/24/32 px desenhados na grade de pixels (ícone .ico)
  hinted/aiaiproj-<N>.svg, doc/aiaiproj*.svg        ícone de arquivo .aiaiproj
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "app" / "crates" / "aiai-studio" / "assets" / "brand"

# --------------------------------------------------------------------------------------------------- paletas
DARK = dict(
    tile_top="#2c2d34", tile_bot="#17181c", border="#565965", perf="#3d3f49", frame_fill="#202127", frame_stroke="#8d909c",
    a_top="#f8ae58", a_bot="#ec8a2b", bar="#fbd2a2", ghost="#f0983c", ghost_op="0.20", sheen="0.07",
    text="#dddee2", accent="#f0983c", accent_bar="#fbd2a2", page="#2a2b32", page_stroke="#8d909c", fold="#3d3f49",
)
LIGHT = dict(
    tile_top="#ffffff", tile_bot="#e7e9ee", border="#8f929d", perf="#c4c7cf", frame_fill="#ffffff", frame_stroke="#5a5d66",
    a_top="#d9731a", a_bot="#b95409", bar="#f2a65a", ghost="#c2600c", ghost_op="0.18", sheen="0",
    text="#1f2024", accent="#c2600c", accent_bar="#f2a65a", page="#ffffff", page_stroke="#8f929d", fold="#d9dbe1",
)

A_PATH = "M42 97 L64 36 L86 97"  # pernas do A (centro do traço, junta redonda)
BAR_PATH = "M49.500 79 L78.500 79"


def f(x: float) -> str:
    s = f"{x:.3f}".rstrip("0").rstrip(".")
    return s if s else "0"


def tile_def(p: dict) -> str:
    sheen = (
        f'<linearGradient id="sheen" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffffff" stop-opacity="{p["sheen"]}"/>'
        f'<stop offset="0.55" stop-color="#ffffff" stop-opacity="0"/></linearGradient>'
        if float(p["sheen"]) > 0
        else ""
    )
    return (
        f'<linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">'
        f'<stop offset="0" stop-color="{p["tile_top"]}"/><stop offset="1" stop-color="{p["tile_bot"]}"/></linearGradient>{sheen}'
    )


def amber_def(p: dict, y0: float, y1: float, ident: str = "amber") -> str:
    return (
        f'<linearGradient id="{ident}" gradientUnits="userSpaceOnUse" x1="0" y1="{f(y0)}" x2="0" y2="{f(y1)}">'
        f'<stop offset="0" stop-color="{p["a_top"]}"/><stop offset="1" stop-color="{p["a_bot"]}"/></linearGradient>'
    )


def mark_body(p: dict) -> str:
    perf = "".join(f'<rect x="{x}" y="{y}" width="9" height="12" rx="2.5"/>' for x in (13, 106) for y in (22, 42, 62, 82))
    parts = ['<rect x="4" y="4" width="120" height="120" rx="26" fill="url(#tile)"/>']
    if float(p["sheen"]) > 0:
        parts.append('<rect x="4" y="4" width="120" height="120" rx="26" fill="url(#sheen)"/>')
    parts += [
        f'<rect x="5" y="5" width="118" height="118" rx="25" fill="none" stroke="{p["border"]}" stroke-width="2"/>',
        f'<g fill="{p["perf"]}">{perf}</g>',
        f'<rect x="30" y="20" width="68" height="88" rx="7" fill="{p["frame_fill"]}" stroke="{p["frame_stroke"]}" stroke-width="3"/>',
        f'<path d="{A_PATH}" transform="translate(-7 0)" fill="none" stroke="{p["ghost"]}" stroke-opacity="{p["ghost_op"]}" '
        f'stroke-width="10" stroke-linecap="round" stroke-linejoin="round"/>',
        f'<path d="{A_PATH}" fill="none" stroke="url(#amber)" stroke-width="10" stroke-linecap="round" stroke-linejoin="round"/>',
        f'<path d="{BAR_PATH}" fill="none" stroke="{p["bar"]}" stroke-width="5.500" stroke-linecap="round"/>',
    ]
    return "\n  ".join(parts)


def logo_svg(p: dict, comment: str) -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128">\n'
        f'  <!-- {comment} -->\n'
        f'  <defs>{tile_def(p)}{amber_def(p, 30, 100)}</defs>\n'
        f'  {mark_body(p)}\n'
        f'</svg>\n'
    )


def small_svg(p: dict) -> str:
    """16 a 32 px: sem perfurações, moldura nem fantasma; A mais grosso."""
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128">\n'
        '  <!-- versão para 16 a 32 px: sem perfurações nem moldura, só o A em pincelada sobre o fundo -->\n'
        f'  <defs>{tile_def(p)}{amber_def(p, 20, 106)}</defs>\n'
        f'  <rect x="4" y="4" width="120" height="120" rx="28" fill="url(#tile)"/>\n'
        f'  <rect x="5" y="5" width="118" height="118" rx="27" fill="none" stroke="{p["border"]}" stroke-width="2"/>\n'
        f'  <path d="M30 104 L64 24 L98 104" fill="none" stroke="url(#amber)" stroke-width="17" stroke-linecap="round" stroke-linejoin="round"/>\n'
        f'  <path d="M44 83 L84 83" fill="none" stroke="{p["bar"]}" stroke-width="9" stroke-linecap="round"/>\n'
        '</svg>\n'
    )


MONO_PARTS = (
    '<rect x="9" y="9" width="110" height="110" rx="24" fill="none" stroke="currentColor" stroke-width="8"/>\n'
    '  <path d="M36 98 L64 28 L92 98" fill="none" stroke="currentColor" stroke-width="13" stroke-linecap="round" stroke-linejoin="round"/>\n'
    '  <path d="M47 80 L81 80" fill="none" stroke="currentColor" stroke-width="7" stroke-linecap="round"/>'
)


def mono_svg() -> str:
    """Uma cor só (currentColor, o app recolore): contorno do quadrado, A e travessa. Legível de 16 px para cima."""
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128">\n'
        '  <!-- monocromática: usa currentColor; contorno do quadro, A em pincelada e travessa -->\n'
        f'  {MONO_PARTS}\n'
        '</svg>\n'
    )


# --------------------------------------------------------------------------------------------------- wordmark
# Letras em unidades de texto: altura das maiúsculas 60 (y de 0 a 60), altura do x 42, traço 8, centro do traço.
# Cada letra: (largura visível, [(path, estilo)], [círculos]). Estilo: "r" = ponta redonda, "bar" = travessa mais fina.
GLYPHS = {
    "A": (48, [("M4 56 L24 4 L44 56", "r"), ("M10.200 40 H37.800", "bar")], []),
    "n": (36, [("M4 56 V22 M4 38 C4 28 10 22 20 22 C29 22 32 28 32 36 V56", "r")], []),
    "i": (8, [("M4 56 V22", "r")], [(4, 5, 5)]),
    "m": (60, [("M4 56 V22 M4 38 C4 28 8 22 18 22 C26 22 30 28 30 36 V56 M30 38 C30 28 35 22 44 22 C53 22 56 28 56 36 V56", "r")], []),
    "e": (42, [("M4 39 H38 A17 17 0 1 0 33 51", "r")], []),
    "I": (8, [("M4 4 V56", "r")], []),
}
LETTER_GAP = 11.0


def wordmark_group(text_color: str, accent: str, accent_bar: str, mono: bool = False) -> tuple[str, float]:
    """Devolve (SVG do texto em unidades de texto, largura total). 'Anime' na cor do texto, 'IAI' no acento."""
    x = 0.0
    out = []
    for idx, ch in enumerate("AnimeIAI"):
        w, strokes, circles = GLYPHS[ch]
        accent_letter = idx >= 5
        col = "currentColor" if mono else (accent if accent_letter else text_color)
        bar = "currentColor" if mono else (accent_bar if accent_letter else text_color)
        g = [f'<g transform="translate({f(x)} 0)">']
        for d, style in strokes:
            c = bar if style == "bar" else col
            sw = "6.500" if style == "bar" else "8"
            g.append(f'<path d="{d}" fill="none" stroke="{c}" stroke-width="{sw}" stroke-linecap="round" stroke-linejoin="round"/>')
        for cx, cy, r in circles:
            g.append(f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{col}"/>')
        g.append("</g>")
        out.append("".join(g))
        x += w + LETTER_GAP
    return "\n    ".join(out), x - LETTER_GAP


def wordmark_svg(p: dict | None) -> str:
    scale, gap = 1.18, 30.0
    mono = p is None
    body, tw = wordmark_group("", "", "", True) if mono else wordmark_group(p["text"], p["accent"], p["accent_bar"])
    total = 128 + gap + tw * scale + 6
    ty = 64 - 30 * scale  # centra as maiúsculas (0..60) na altura da marca
    if mono:
        head, mark = "", f'<g>\n  {MONO_PARTS}\n  </g>'
    else:
        head, mark = f'<defs>{tile_def(p)}{amber_def(p, 30, 100)}</defs>\n  ', f'<g>\n  {mark_body(p)}\n  </g>'
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {f(total)} 128">\n'
        f'  <!-- marca + "AnimeIAI" (letras em traçado próprio, sem texto nem fonte) -->\n'
        f'  {head}{mark}\n'
        f'  <g transform="translate({f(128 + gap)} {f(ty)}) scale({scale})">\n    {body}\n  </g>\n'
        f'</svg>\n'
    )


# --------------------------------------------------------------------------------------------------- hinted (grade de pixels)
# N: quadrado (inset, raio), A (topo, pé, ápice x0..x1, pé externo xo, pé interno xi), travessa (y, altura)
HINT = {
    16: dict(inset=0.5, rx=3.5, top=2, foot=14, ap0=7, ap1=9, xo=2, xi=4.5, bar_y=10, bar_h=1),
    20: dict(inset=0.5, rx=4.5, top=3, foot=17, ap0=8.5, ap1=11.5, xo=3, xi=6, bar_y=12, bar_h=1.5),
    24: dict(inset=0.5, rx=5.5, top=3, foot=21, ap0=10.4, ap1=13.6, xo=3.8, xi=7.2, bar_y=15, bar_h=2),
    32: dict(inset=0.5, rx=7, top=4, foot=28, ap0=14, ap1=18, xo=4.5, xi=9.5, bar_y=20, bar_h=2.5),
}
DOC_HINT = {
    16: dict(x0=2.5, x1=13.5, y0=0.5, y1=15.5, fold=4, top=6, foot=13, ap0=7, ap1=9, xo=4, xi=6, bar_y=10.5, bar_h=1),
    20: dict(x0=3.5, x1=16.5, y0=0.5, y1=19.5, fold=5, top=8, foot=16, ap0=9, ap1=11, xo=5, xi=7.5, bar_y=13, bar_h=1.5),
    24: dict(x0=3.5, x1=20.5, y0=0.5, y1=23.5, fold=6, top=9, foot=19.5, ap0=10.5, ap1=13.5, xo=5.5, xi=8.5, bar_y=15.5, bar_h=2),
    32: dict(x0=5.5, x1=26.5, y0=0.5, y1=31.5, fold=8, top=12, foot=26, ap0=14, ap1=18, xo=7.5, xi=11.5, bar_y=21, bar_h=2.5),
}


def a_shapes(n: int, h: dict, p: dict) -> str:
    """O A em dois quadriláteros (topo e pé planos na grade) e a travessa entre as pernas."""
    top, foot = h["top"], h["foot"]
    left = [(h["ap0"], top), (h["ap1"], top), (h["xi"], foot), (h["xo"], foot)]
    right = [(h["ap0"], top), (h["ap1"], top), (n - h["xi"], foot), (n - h["xo"], foot)]
    pts = lambda q: " ".join(f"{f(x)},{f(y)}" for x, y in q)

    def inner_x(y: float, side_left: bool) -> float:
        t = (y - top) / (foot - top)
        if side_left:
            return h["ap1"] + (h["xi"] - h["ap1"]) * t
        return h["ap0"] + ((n - h["xi"]) - h["ap0"]) * t

    by, bh = h["bar_y"], h["bar_h"]
    bx0 = inner_x(by + bh / 2, True) - 0.3
    bx1 = inner_x(by + bh / 2, False) + 0.3
    return (
        f'<g fill="{p["a_bot"]}"><polygon points="{pts(left)}"/><polygon points="{pts(right)}"/></g>\n'
        f'  <rect x="{f(bx0)}" y="{f(by)}" width="{f(bx1 - bx0)}" height="{f(bh)}" fill="{p["bar"]}"/>'
    )


def hinted_svg(n: int, p: dict) -> str:
    h = HINT[n]
    ins, rx = h["inset"], h["rx"]
    side = n - 2 * ins
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {n} {n}" width="{n}" height="{n}">\n'
        f'  <!-- {n} px na grade de pixels: só o A sobre o quadrado, bordas e pés alinhados aos pixels -->\n'
        f'  <defs>{tile_def(p)}</defs>\n'
        f'  <rect x="{f(ins)}" y="{f(ins)}" width="{f(side)}" height="{f(side)}" rx="{f(rx)}" fill="url(#tile)" stroke="{p["border"]}" stroke-width="1"/>\n'
        f'  {a_shapes(n, h, p)}\n'
        f'</svg>\n'
    )


def doc_hinted(n: int, p: dict) -> str:
    h = DOC_HINT[n]
    x0, x1, y0, y1, fd = h["x0"], h["x1"], h["y0"], h["y1"], h["fold"]
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {n} {n}" width="{n}" height="{n}">\n'
        f'  <!-- {n} px na grade de pixels: página com dobra e o A -->\n'
        f'  <path d="M{f(x0)} {f(y0)} H{f(x1 - fd)} L{f(x1)} {f(y0 + fd)} V{f(y1)} H{f(x0)} Z" fill="{p["page"]}" stroke="{p["page_stroke"]}" stroke-width="1" stroke-linejoin="round"/>\n'
        f'  <path d="M{f(x1 - fd)} {f(y0)} V{f(y0 + fd)} H{f(x1)}" fill="{p["fold"]}" stroke="{p["page_stroke"]}" stroke-width="1" stroke-linejoin="round"/>\n'
        f'  {a_shapes(n, h, p)}\n'
        f'</svg>\n'
    )


def doc_svg(p: dict) -> str:
    """Página com a dobra no canto e a marca (o A em pincelada) no meio."""
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128">\n'
        '  <!-- documento de projeto (.aiaiproj): página com dobra e a marca -->\n'
        f'  <defs>{amber_def(p, 54, 108, "amber")}</defs>\n'
        f'  <path d="M26 8 H78 L106 36 V114 Q106 120 100 120 H26 Q20 120 20 114 V14 Q20 8 26 8 Z" fill="{p["page"]}" stroke="{p["page_stroke"]}" stroke-width="3" stroke-linejoin="round"/>\n'
        f'  <path d="M78 8 V30 Q78 36 84 36 H106" fill="{p["fold"]}" stroke="{p["page_stroke"]}" stroke-width="3" stroke-linejoin="round"/>\n'
        f'  <path d="M38 100 L63 49 L88 100" fill="none" stroke="url(#amber)" stroke-width="10" stroke-linecap="round" stroke-linejoin="round"/>\n'
        f'  <path d="M47 84 L79 84" fill="none" stroke="{p["bar"]}" stroke-width="5.500" stroke-linecap="round"/>\n'
        f'  <rect x="36" y="19" width="26" height="4" rx="2" fill="{p["accent"]}"/>\n'
        '</svg>\n'
    )


# --------------------------------------------------------------------------------------------------- escrita
def write(rel: str, text: str) -> None:
    path = OUT / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    print("escrito", path.relative_to(ROOT))


def main() -> int:
    dark_logo = logo_svg(DARK, "marca para interface escura (quadrado escuro); é também o ícone do app")
    write("logo.svg", dark_logo)
    write("logo-dark.svg", dark_logo)
    write("logo-light.svg", logo_svg(LIGHT, "marca para interface clara (quadrado claro, borda mais forte)"))
    write("logo-small.svg", small_svg(DARK))
    write("logo-small-light.svg", small_svg(LIGHT))
    write("logo-mono.svg", mono_svg())
    wm = wordmark_svg(DARK)
    write("logo-wordmark.svg", wm)
    write("logo-wordmark-dark.svg", wm)
    write("logo-wordmark-light.svg", wordmark_svg(LIGHT))
    write("logo-wordmark-mono.svg", wordmark_svg(None))
    for n in HINT:
        write(f"hinted/logo-{n}.svg", hinted_svg(n, DARK))
        write(f"hinted/logo-{n}-light.svg", hinted_svg(n, LIGHT))
        write(f"hinted/aiaiproj-{n}.svg", doc_hinted(n, DARK))
    write("doc/aiaiproj.svg", doc_svg(DARK))
    write("doc/aiaiproj-light.svg", doc_svg(LIGHT))
    return 0


if __name__ == "__main__":
    sys.exit(main())
