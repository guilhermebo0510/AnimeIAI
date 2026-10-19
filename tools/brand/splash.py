#!/usr/bin/env python3
"""Splash e prancha da marca do AnimeIAI a partir dos PNGs exportados pelo app (teste `brand_pngs`).

Uso:
    set AIAI_BRAND_OUT=C:\\AnimeIAI\\sprints\\registro\\evidencias\\S20\\marca
    cargo test -p aiai-studio --lib brand_pngs -- --ignored
    python tools/brand/splash.py C:\\AnimeIAI\\sprints\\registro\\evidencias\\S20\\marca

Saída na mesma pasta: splash.png (1920x1080, tema escuro) e marca_prancha.png (todos os tamanhos sobre fundo escuro e
claro, para conferir a leitura de 16 a 512 px). Fonte: Noto Sans do próprio app (SIL OFL 1.1).
"""

from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
FONT = ROOT / "app" / "crates" / "aiai-studio" / "assets" / "fonts" / "NotoSans-Regular.ttf"

# tokens do tema (theme.rs)
DARK_FRAME, DARK_PANEL, DARK_TEXT, DARK_DIM, ACCENT = (0x0F, 0x10, 0x12), (0x22, 0x23, 0x27), (0xDD, 0xDE, 0xE2), (0xA6, 0xA8, 0xB1), (0xF0, 0x98, 0x3C)
LIGHT_PANEL, LIGHT_TEXT = (0xEC, 0xED, 0xEF), (0x1F, 0x20, 0x24)


def font(px: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(str(FONT), px)


def splash(src: Path, out: Path) -> None:
    w, h = 1920, 1080
    img = Image.new("RGB", (w, h), DARK_FRAME)
    d = ImageDraw.Draw(img)
    # degradê vertical discreto moldura -> painel
    for y in range(h):
        t = y / (h - 1)
        d.line([(0, y), (w, y)], fill=tuple(int(a + (b - a) * t) for a, b in zip(DARK_FRAME, DARK_PANEL)))
    logo = Image.open(src / "logo-dark-512.png").convert("RGBA").resize((420, 420), Image.LANCZOS)
    img.paste(logo, (250, (h - 420) // 2), logo)
    x = 250 + 420 + 90
    d.text((x, 360), "AnimeIAI", font=font(150), fill=DARK_TEXT)
    d.text((x + 6, 560), "Estúdio de animação 2D com ferramentas próprias", font=font(44), fill=DARK_DIM)
    d.text((x + 6, 622), "2D animation studio with its own tools", font=font(36), fill=DARK_DIM)
    d.rectangle([x + 6, 700, x + 6 + 160, 706], fill=ACCENT)
    img.save(out / "splash.png")


def prancha(src: Path, out: Path) -> None:
    sizes = [("small", 16), ("small", 24), ("small", 32), ("dark", 48), ("dark", 64), ("dark", 128), ("dark", 256), ("dark", 512)]
    pad, label_h = 40, 40
    width = pad + sum(s + pad for _, s in sizes)
    row_h = 512 + pad * 2 + label_h
    img = Image.new("RGB", (width, row_h * 2), DARK_PANEL)
    d = ImageDraw.Draw(img)
    d.rectangle([0, row_h, width, row_h * 2], fill=LIGHT_PANEL)
    for row, (bg_tag, text) in enumerate([("dark", DARK_TEXT), ("light", LIGHT_TEXT)]):
        x = pad
        for tag, s in sizes:
            name = f"logo-{tag}-{s}.png" if tag == "small" else f"logo-{bg_tag}-{s}.png"
            logo = Image.open(src / name).convert("RGBA")
            y = row * row_h + pad + (512 - s)
            img.paste(logo, (x, y), logo)
            d.text((x, row * row_h + pad + 512 + 10), f"{s}", font=font(22), fill=text)
            x += s + pad
    img.save(out / "marca_prancha.png")


def main() -> int:
    folder = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    splash(folder, folder)
    prancha(folder, folder)
    print(folder / "splash.png", folder / "marca_prancha.png")
    return 0


if __name__ == "__main__":
    sys.exit(main())
