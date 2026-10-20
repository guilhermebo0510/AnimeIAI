#!/usr/bin/env python3
"""Gera os PNGs e os .ico da marca do AnimeIAI a partir dos SVG de `assets/brand/` (rode antes `make_svgs.py`).

Rasterização: `tools/brand/rasterize` (Rust, o mesmo resvg que o app usa em `icons.rs`); Pillow só monta os arquivos.
Saída em `app/crates/aiai-studio/assets/brand/`:
  aiai.ico        16, 20, 24, 32 (desenhados na grade de pixels, `hinted/`), 40, 48, 64, 96, 128 e 256 (marca completa)
  aiaiproj.ico    o mesmo conjunto de tamanhos para o ícone de arquivo .aiaiproj (não é registrado no sistema)
  png/            cada tamanho do .ico (aiai-<N>.png, aiai-light-<N>.png, aiaiproj-<N>.png) e os wordmarks em 1024 px

O 256 vai como PNG dentro do .ico e os demais como DIB 32 bpp (máscara AND zerada), o formato que o Explorer e o `rc.exe`
leem em qualquer versão do Windows.

Uso: python tools/brand/make_icons.py
"""

from __future__ import annotations

import io
import os
import struct
import subprocess
import sys
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
BRAND = ROOT / "app" / "crates" / "aiai-studio" / "assets" / "brand"
PNG = BRAND / "png"
TOOL_DIR = ROOT / "tools" / "brand" / "rasterize"
TARGET = ROOT / "app" / "target" / "brand_rasterize"
EXE = TARGET / "release" / ("brand-rasterize.exe" if os.name == "nt" else "brand-rasterize")

SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
HINTED = {16, 20, 24, 32}


def rasterizer() -> Path:
    if not EXE.exists():
        env = dict(os.environ, CARGO_TARGET_DIR=str(TARGET), CARGO_INCREMENTAL="0")
        subprocess.run(["cargo", "build", "--release", "-j", "2", "--manifest-path", str(TOOL_DIR / "Cargo.toml")], check=True, env=env)
    return EXE


def raster(svg: Path, out: Path, width: int, height: int | None = None) -> Image.Image:
    cmd = [str(rasterizer()), str(svg), str(out), str(width)] + ([str(height)] if height else [])
    subprocess.run(cmd, check=True)
    return Image.open(out).convert("RGBA")


def build_set(name_svg_full: str, name_svg_hinted: str, png_prefix: str) -> dict[int, Image.Image]:
    """Rasteriza todos os tamanhos: hinted (SVG na grade de pixels) até 32, a marca completa dali em diante."""
    imgs: dict[int, Image.Image] = {}
    for n in SIZES:
        svg = BRAND / (name_svg_hinted.format(n=n) if n in HINTED else name_svg_full)
        out = PNG / f"{png_prefix}-{n}.png"
        imgs[n] = raster(svg, out, n, n)
        assert imgs[n].size == (n, n), (svg, imgs[n].size)
    return imgs


def write_ico(path: Path, imgs: dict[int, Image.Image]) -> None:
    entries = []
    for n in sorted(imgs):
        im = imgs[n]
        if n >= 256:
            buf = io.BytesIO()
            im.save(buf, "PNG")
            data = buf.getvalue()
        else:
            # DIB: BITMAPINFOHEADER (altura dobrada: XOR + AND), BGRA de baixo para cima, máscara AND zerada (alfa manda)
            hdr = struct.pack("<IiiHHIIiiII", 40, n, n * 2, 1, 32, 0, 0, 0, 0, 0, 0)
            px = im.tobytes("raw", "BGRA")
            rows = [px[y * n * 4:(y + 1) * n * 4] for y in range(n)][::-1]
            mask_row = (n + 31) // 32 * 4
            data = hdr + b"".join(rows) + bytes(mask_row * n)
        entries.append((n, data))
    head = struct.pack("<HHH", 0, 1, len(entries))
    offset = 6 + 16 * len(entries)
    table = b""
    blobs = b""
    for n, data in entries:
        table += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(data), offset + len(blobs))
        blobs += data
    path.write_bytes(head + table + blobs)
    print("escrito", path.relative_to(ROOT), f"({path.stat().st_size} bytes, {len(entries)} tamanhos)")


def main() -> int:
    PNG.mkdir(parents=True, exist_ok=True)
    dark = build_set("logo-dark.svg", "hinted/logo-{n}.svg", "aiai")
    build_set("logo-light.svg", "hinted/logo-{n}-light.svg", "aiai-light")
    doc = build_set("doc/aiaiproj.svg", "hinted/aiaiproj-{n}.svg", "aiaiproj")
    write_ico(BRAND / "aiai.ico", dark)
    write_ico(BRAND / "aiaiproj.ico", doc)
    for tag in ("dark", "light", "mono"):
        raster(BRAND / f"logo-wordmark-{tag}.svg", PNG / f"logo-wordmark-{tag}.png", 1024)
    raster(BRAND / "logo-mono.svg", PNG / "logo-mono-128.png", 128, 128)
    return 0


if __name__ == "__main__":
    sys.exit(main())
