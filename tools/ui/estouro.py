#!/usr/bin/env python3
"""Varredura de estouro de layout (S20 P6e): percorre todos os workspaces em PT e EN, em várias larguras de janela
(da mínima, 1024x680 pontos, até a maximizada), lê o `overflow` do GET_UI_STATE (widgets cortados na horizontal pela
borda do painel ou da janela) e grava um relatório JSON + uma captura de cada combinação que estourou.

Uso (app aberto com --bridge, de preferência com um projeto de verdade, numa CÓPIA — lição 61):

    python tools/ui/estouro.py C:\\AnimeIAI\\sprints\\registro\\evidencias\\S20\\estouro

Painéis com rolagem horizontal intencional (linha do tempo, canvas) e widgets totalmente escondidos dentro deles não contam.
Mede também a barra do canvas com cada ferramenta (na Animação) e abre e mede os diálogos (Preferências, Sobre). Só lê o documento; ao fim volta ao idioma/workspace/janela do início.
Saída: código 0 sem estouro, 1 com estouro (lista no stdout e em relatorio.json).
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import aiai  # noqa: E402

SIZES = [(1024, 680), (1280, 720), (1440, 864), "max"]
LANGS = ["pt", "en"]
DIALOGS = ["prefs", "about"]


def settle(seconds: float = 1.0) -> None:
    aiai.call("INPUT", {"events": [{"kind": "wait", "frames": 4}], "wait": True})
    time.sleep(seconds)
    aiai.call("INPUT", {"events": [{"kind": "wait", "frames": 4}], "wait": True})


def counted(ov: list[dict]) -> list[dict]:
    return [o for o in ov if not o.get("scroll_x")]


def set_size(size) -> list[float]:
    if size == "max":
        aiai.call("UI_ACTION", {"action": "set_window_size", "width": 0, "height": 0, "maximized": True})
    else:
        aiai.call("UI_ACTION", {"action": "set_window_size", "width": size[0], "height": size[1]})
    settle(1.2)
    return aiai.call("GET_UI_STATE")["window"]["size_logical"]


def main() -> int:
    out_dir = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    st0 = aiai.call("GET_UI_STATE")
    ws_all, lang0, ws0, max0 = st0["workspaces"], st0["language"], st0["workspace"], st0["window"]["maximized"]
    tools_all, tool0 = st0["tools"], st0["active_tool"]
    size0 = st0["window"]["size_logical"]
    report: list[dict] = []
    hits = 0
    try:
        for size in SIZES:
            real = set_size(size)
            for lang in LANGS:
                aiai.call("UI_ACTION", {"action": "set_language", "language": lang})
                # cada ferramenta muda a barra do canvas: todas medidas na Animação
                views = [("ws", w) for w in ws_all] + [("tool", t) for t in tools_all] + [("dialog", d) for d in DIALOGS]
                for kind, name in views:
                    if kind == "ws":
                        aiai.call("UI_ACTION", {"action": "set_workspace", "workspace": name})
                    elif kind == "tool":
                        aiai.call("UI_ACTION", {"action": "set_workspace", "workspace": "animacao"})
                        aiai.call("UI_ACTION", {"action": "select_tool", "tool": name})
                    else:
                        aiai.call("UI_ACTION", {"action": "open_dialog", "dialog": name})
                    settle(1.0 if kind == "ws" else 0.6)
                    st = aiai.call("GET_UI_STATE")
                    ov = counted(st["overflow"])
                    entry = {"size": real, "requested": size, "language": lang, kind: name, "overflow": ov, "ignored_scroll_x": len(st["overflow"]) - len(ov)}
                    if ov:
                        hits += 1
                        tag = f"{'tool-' if kind == 'tool' else ''}{name}_{lang}_{int(real[0])}x{int(real[1])}"
                        png = out_dir / f"estouro_{tag}.png"
                        aiai.screenshot(str(png))
                        entry["screenshot"] = str(png)
                        print(f"ESTOURO {tag}: " + "; ".join(f"{o['panel']}:{o.get('label') or o.get('type')} +{o['excess']}" for o in ov[:6]))
                    else:
                        print(f"ok {name} {lang} {real}")
                    if kind == "dialog":
                        aiai.call("UI_ACTION", {"action": "close_dialog", "dialog": name})
                    report.append(entry)
    finally:
        aiai.call("UI_ACTION", {"action": "set_language", "language": lang0})
        aiai.call("UI_ACTION", {"action": "select_tool", "tool": tool0})
        aiai.call("UI_ACTION", {"action": "set_workspace", "workspace": ws0})
        if max0:
            aiai.call("UI_ACTION", {"action": "set_window_size", "width": 0, "height": 0, "maximized": True})
        else:
            aiai.call("UI_ACTION", {"action": "set_window_size", "width": size0[0], "height": size0[1]})
    total = sum(len(e["overflow"]) for e in report)
    summary = {"combinations": len(report), "with_overflow": hits, "overflow_items": total, "sizes": [e for e in SIZES if e != "max"] + ["max"], "languages": LANGS}
    (out_dir / "relatorio.json").write_text(json.dumps({"summary": summary, "entries": report}, ensure_ascii=False, indent=1), encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False))
    return 1 if hits else 0


if __name__ == "__main__":
    sys.exit(main())
