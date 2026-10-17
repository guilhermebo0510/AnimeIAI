#!/usr/bin/env python3
"""Prancha da interface (S20 P6e): captura todos os workspaces do app aberto em PT/EN x escuro/claro pela bridge e monta
uma folha por combinação (idioma, tema) e uma folha geral.

Uso (app aberto com --bridge, de preferência com um projeto de verdade):

    python tools/ui/prancha.py C:\\AnimeIAI\\sprints\\registro\\evidencias\\S20\\prancha

Só lê o documento (troca workspace, tema e idioma da interface); ao fim volta ao tema/idioma/workspace do início.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import aiai  # noqa: E402
from PIL import Image, ImageDraw, ImageFont  # noqa: E402

THEMES = ["dark", "light"]
LANGS = ["pt", "en"]
THUMB_W = 640
COLS = 4


def font(size: int) -> ImageFont.ImageFont:
    for name in ("segoeui.ttf", "arial.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def settle(seconds: float = 1.6) -> None:
    """Espera os renders em segundo plano (miniaturas, monitor, canvas) chegarem."""
    aiai.call("INPUT", {"events": [{"kind": "wait", "frames": 4}], "wait": True})
    time.sleep(seconds)
    aiai.call("INPUT", {"events": [{"kind": "wait", "frames": 4}], "wait": True})


def sheet(paths: list[tuple[str, Path]], title: str, out: Path) -> None:
    thumbs = []
    for label, p in paths:
        im = Image.open(p).convert("RGB")
        h = round(im.height * THUMB_W / im.width)
        thumbs.append((label, im.resize((THUMB_W, h), Image.LANCZOS)))
    th = thumbs[0][1].height
    rows = (len(thumbs) + COLS - 1) // COLS
    pad, head, cap = 16, 56, 30
    W = pad + COLS * (THUMB_W + pad)
    H = head + rows * (th + cap + pad) + pad
    board = Image.new("RGB", (W, H), (28, 29, 33))
    d = ImageDraw.Draw(board)
    d.text((pad, 14), title, fill=(235, 236, 240), font=font(26))
    for i, (label, im) in enumerate(thumbs):
        x = pad + (i % COLS) * (THUMB_W + pad)
        y = head + (i // COLS) * (th + cap + pad)
        board.paste(im, (x, y))
        d.text((x, y + th + 4), label, fill=(200, 202, 210), font=font(18))
    out.parent.mkdir(parents=True, exist_ok=True)
    board.save(out)


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    out = Path(sys.argv[1])
    state = aiai.ui_state()
    start = (state["theme"], state["language"], state["workspace"])
    workspaces = state["workspaces"]
    sheets = []
    try:
        for theme in THEMES:
            aiai.call("UI_ACTION", {"action": "set_theme", "theme": theme})
            for lang in LANGS:
                aiai.call("UI_ACTION", {"action": "set_language", "language": lang})
                shots = []
                for ws in workspaces:
                    aiai.call("UI_ACTION", {"action": "set_workspace", "workspace": ws})
                    settle()
                    p = out / f"{lang}_{theme}" / f"{ws}.png"
                    aiai.screenshot(p)
                    shots.append((ws, p))
                    print(f"{lang} {theme} {ws}", flush=True)
                s = out / f"prancha_{lang}_{theme}.png"
                sheet(shots, f"AnimeIAI Studio · {lang.upper()} · {'escuro' if theme == 'dark' else 'claro'} · {len(shots)} workspaces", s)
                sheets.append(s)
    finally:
        aiai.call("UI_ACTION", {"action": "set_theme", "theme": start[0]})
        aiai.call("UI_ACTION", {"action": "set_language", "language": start[1]})
        aiai.call("UI_ACTION", {"action": "set_workspace", "workspace": start[2]})
    print("\n".join(str(s) for s in sheets))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
