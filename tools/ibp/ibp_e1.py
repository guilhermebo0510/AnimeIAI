"""E1 (R16 §11, CX04 §4) sobre o protótipo: custo da prévia de intercalação na interface e do cálculo síncrono.

Mesma repetição (cursor passeando no canvas + zoom pela roda, nada que mude o projeto: qualquer mudança derruba a
prévia) com a prévia DESLIGADA e LIGADA, em dois desenhos: normal (rosto, 4 traços) e no limite (perto de 200 traços e
20 000 pontos por chave). Amostras brutas por quadro: CPU de montar o quadro inteiro (`ui_ms`) e o desenho da prévia
(`ibp_paint_ms`); mais o tempo do cálculo síncrono (`compute_ms`, várias repetições).

Uso: python tools/ibp/ibp_e1.py [--exe release|debug] [--frames 3000] [--reps 2]
Grava sprints/registro/evidencias/S26/e1.json (amostras brutas) e imprime o resumo.
"""
import argparse
import json
import math
import os
import platform
import shutil
import statistics
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "csp"))
sys.path.insert(0, os.path.join(HERE, "..", "calib"))
sys.path.insert(0, HERE)
import calib_app as ca  # noqa: E402
import aiai  # noqa: E402
from drive import Drive  # noqa: E402
import ibp_e2e as e2e  # noqa: E402

EVID = e2e.EVID


def heavy_keys(n_strokes, pts_per):
    """Muitos traços curtos espalhados (cabelo/hachura): A e B deslocados."""
    a, b = [], []
    for i in range(n_strokes):
        cx, cy = 200 + (i % 20) * 44, 120 + (i // 20) * 48
        pa = [(cx + 30 * k / (pts_per - 1), cy + 8 * math.sin(k / (pts_per - 1) * math.pi * 2)) for k in range(pts_per)]
        pb = [(x + 60, y + 10) for x, y in pa]
        a.append(e2e.stroke(f"a{i}", pa, width=2.0))
        b.append(e2e.stroke(f"b{i}", pb, width=2.0))
    return a, b


def build_heavy(dst, n_strokes=190, pts_per=50):
    if os.path.exists(dst):
        shutil.rmtree(dst)
    d = Drive(dst, exe=e2e.MCP)
    d.call("NEW_PROJECT", {"path": dst, "name": "ibp-e1", "settings": {"width": 1280, "height": 720}})
    ep = d.create("EP", {"type": "episode"})
    sc = d.create("S", {"type": "scene"}, ep)
    cut = d.create("C001", {"type": "cut", "duration": 24}, sc)
    layer = d.create("A", {"type": "layer", "kind": "vector"}, cut)
    a, b = heavy_keys(n_strokes, pts_per)
    d.create("A1", {"type": "cel", "number": 1, "role": "key", "strokes": a}, layer)
    d.create("A2", {"type": "cel", "number": 2, "role": "key", "strokes": b}, layer)
    d.apply({"kind": "add_track", "cut": cut, "track": {"id": "trk-a", "name": "A", "kind": "drawing", "layer": layer, "cells": {}}})
    d.apply({"kind": "set_cells", "cut": cut, "track": "trk-a", "cells": [{"frame": 0, "cell": {"type": "cel", "number": 1}}, {"frame": 12, "cell": {"type": "cel", "number": 2}}]})
    return cut, layer


def replay(frames):
    """Cursor em círculos sobre o canvas e zoom de vai e volta, `frames` quadros."""
    cv = aiai.call("GET_UI_STATE")["widgets"]["canvas:area"]["rect_logical"]
    cx, cy, r = cv[0] + cv[2] / 2, cv[1] + cv[3] / 2, min(cv[2], cv[3]) * 0.3
    done = 0
    while done < frames:
        evs = []
        for k in range(100):
            a = (done + k) / 40.0
            evs.append({"kind": "mouse_move", "x": cx + r * math.cos(a), "y": cy + r * math.sin(a)})
            evs.append({"kind": "wait", "frames": 1})
            if (done + k) % 50 == 25:
                evs.append({"kind": "wheel", "dx": 0, "dy": 60 if ((done + k) // 50) % 2 else -60})
        aiai.call("INPUT", {"events": evs, "wait": True}, timeout=600)
        done += 100


def pct(v, q):
    if not v:
        return None
    s = sorted(v)
    return s[min(len(s) - 1, int(round(q / 100 * (len(s) - 1))))]


def summary(v):
    return {"n": len(v), "p50": pct(v, 50), "p95": pct(v, 95), "p99": pct(v, 99), "max": max(v) if v else None, "mean": statistics.fmean(v) if v else None}


def run_case(name, exe, build_fn, frame, frames, reps, out):
    dst = os.path.join(ca.TMP, f"ibp-e1-{name}.aiai")
    cut, layer = build_fn(dst)
    proc = ca.launch_hidden([exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--project", dst, "--workspace", "animacao"])
    case = {"conditions": {}}
    try:
        ca.wait_ready(proc)
        ca.settle(30)
        ca.click_widget_if("warn:ok")
        aiai.call("UI_ACTION", {"action": "set_frame", "frame": frame})
        ca.settle(10)
        computes = []
        for _ in range(10):
            p = aiai.call("INBETWEEN_PREVIEW", {"cut": cut, "layer": layer, "frame": frame})
            computes.append(p["compute_ms"])
        case["compute_ms"] = computes
        case["counts"] = p["counts"]
        aiai.call("INBETWEEN_DISCARD", {"proposal_id": p["proposal_id"]})
        for rep in range(reps):
            for cond in ("off", "on"):
                pid = None
                if cond == "on":
                    pid = aiai.call("INBETWEEN_PREVIEW", {"cut": cut, "layer": layer, "frame": frame})["proposal_id"]
                ca.settle(20)
                aiai.call("UI_ACTION", {"action": "perf_samples", "op": "start"})
                replay(frames)
                s = aiai.call("UI_ACTION", {"action": "perf_samples", "op": "stop"})
                case["conditions"].setdefault(cond, []).append({"ui_ms": s["ui_ms"], "ibp_paint_ms": s["ibp_paint_ms"]})
                if pid:
                    alive = (aiai.call("GET_UI_STATE")["editor"].get("ibp") or {}).get("proposal_id") == pid
                    case["conditions"][cond][-1]["preview_alive_at_end"] = alive
                    aiai.call("INBETWEEN_DISCARD", {"proposal_id": pid})
        aiai.call("SCREENSHOT", {"out": os.path.join(EVID, f"e1_{name}.png")})
    finally:
        proc.kill()
    out[name] = case


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default="release", choices=["release", "debug"])
    ap.add_argument("--frames", type=int, default=3000)
    ap.add_argument("--reps", type=int, default=2)
    a = ap.parse_args()
    exe = os.path.join(ca.ROOT, "app", "target", a.exe, "aiai-studio.exe")
    os.makedirs(EVID, exist_ok=True)
    out = {"meta": {"exe": exe, "frames_per_condition": a.frames, "reps": a.reps, "machine": platform.processor(), "platform": platform.platform()}}
    try:
        gpu = subprocess.run(["powershell", "-NoProfile", "-Command", "(Get-CimInstance Win32_VideoController).Name"], capture_output=True, text=True, timeout=30).stdout.strip()
        cpu = subprocess.run(["powershell", "-NoProfile", "-Command", "(Get-CimInstance Win32_Processor).Name"], capture_output=True, text=True, timeout=30).stdout.strip()
        out["meta"].update({"gpu": gpu, "cpu": cpu})
    except Exception as e:  # noqa: BLE001
        out["meta"]["hw_error"] = str(e)
    run_case("normal", exe, e2e.build, 6, a.frames, a.reps, out)
    run_case("limite", exe, build_heavy, 6, a.frames, a.reps, out)
    with open(os.path.join(EVID, "e1.json"), "w", encoding="utf-8") as f:
        json.dump(out, f)
    res = {"meta": out["meta"]}
    for name in ("normal", "limite"):
        c = out[name]
        res[name] = {"counts": c["counts"], "compute_ms": summary(c["compute_ms"])}
        for cond, runs in c["conditions"].items():
            ui = [x for r in runs for x in r["ui_ms"]]
            paint = [x for r in runs for x in r["ibp_paint_ms"]]
            res[name][cond] = {"ui_ms": summary(ui), "ibp_paint_ms": summary(paint), "per_rep_p95": [pct(r["ui_ms"], 95) for r in runs],
                               "preview_alive_at_end": [r.get("preview_alive_at_end") for r in runs]}
    with open(os.path.join(EVID, "e1_resumo.json"), "w", encoding="utf-8") as f:
        json.dump(res, f, ensure_ascii=False, indent=1)
    print(json.dumps(res, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
