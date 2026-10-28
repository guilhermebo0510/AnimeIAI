"""Opções novas do criador de pincéis (seção 3: estudo do Clip Studio) numa instância de teste:

  1. pinta uma prancha com as opções novas (set_brush com `custom`): variar para a secundária, sortear por traço,
     partículas alinhadas, borda de aquarela escura e larga, textura girada, modo escurecer e pelo menos 1 px;
  2. abre as Configurações avançadas e fotografa as categorias que ganharam controles (Tamanho, Tinta, Forma,
     Spray, Traço, Textura, Aquarela, Apagar, Correção).

Grava calibracao/tmp/brush_options_sheet.png e brush_options_<cat>.png. Uso: python tools/csp/brush_options_test.py
"""
import argparse
import base64
import json
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(__file__))
import calib_app as ca  # noqa: E402
import aiai  # noqa: E402

PROJECT = os.path.join(ca.TMP, "brush_options_test.aiai")


def click(name):
    st = aiai.call("GET_UI_STATE")
    w = st.get("widgets", {}).get(name)
    if not w:
        return False
    r = w["rect_logical"]
    x, y = r[0] + r[2] / 2, r[1] + r[3] / 2
    aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "primary"},
                                                {"kind": "mouse_up", "x": x, "y": y, "button": "primary"}, {"kind": "wait", "frames": 2}]})
    return True


def stroke(cv, pts, pressure=0.9):
    evs = []
    for i, (x, y) in enumerate(pts):
        sx, sy = ca.to_screen(cv, x, y)
        evs.append({"kind": "pen", "x": round(sx, 2), "y": round(sy, 2), "pressure": pressure, "phase": "down" if i == 0 else "move", "t_ms": i * 12.0})
    sx, sy = ca.to_screen(cv, *pts[-1])
    evs.append({"kind": "pen", "x": round(sx, 2), "y": round(sy, 2), "pressure": 0.0, "phase": "up"})
    aiai.call("INPUT", {"events": evs, "wait": True}, timeout=60)
    ca.settle(6)


def line(x0, x1, y, n=40, wave=0.0):
    import math
    return [(x0 + (x1 - x0) * i / n, y + wave * math.sin(i / n * 6.283)) for i in range(n + 1)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=os.path.join(ca.ROOT, "app", "target", "debug", "aiai-studio.exe"))
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    if os.path.exists(PROJECT):
        shutil.rmtree(PROJECT)
    proc = ca.launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--workspace", "pintura"])
    res = {}
    try:
        ca.wait_ready(proc)
        ca.settle(10)
        ca.click_widget_if("warn:ok")
        ca.PROJECT = PROJECT
        aiai.call("NEW_PROJECT", {"name": "opcoes", "settings": {"width": 1200, "height": 900}, "path": PROJECT, "overwrite": True})
        ca.settle(6)
        ca.assert_test_project()
        ep = ca.made("EP", None, {"type": "episode", "number": 1})
        sc = ca.made("S", ep, {"type": "scene", "number": 1})
        cut = ca.made("C001", sc, {"type": "cut", "number": 1, "duration": 12})
        paper = ca.made("papel", cut, {"type": "layer", "kind": "raster"})
        ca.made("c1", paper, {"type": "cel", "number": 1})
        aiai.call("FILL_SHAPE", {"cut": cut, "frame": 0, "layer": paper, "shapes": [{"points": [[0, 0], [1200, 0], [1200, 900], [0, 900]]}], "color": {"type": "direct", "rgba": [0.96, 0.94, 0.88, 1.0]}})
        layer = ca.made("pintura", cut, {"type": "layer", "kind": "raster"})
        ca.made("c1", layer, {"type": "cel", "number": 1})
        aiai.call("UI_ACTION", {"action": "select_cut", "cut": cut})
        aiai.call("UI_ACTION", {"action": "select_layer", "layer": layer})
        aiai.call("UI_ACTION", {"action": "set_tool", "tool": "brush"})
        aiai.call("UI_ACTION", {"action": "fit_view"})
        ca.settle(8)
        cv = aiai.call("GET_UI_STATE")["editor"]["canvas"]
        rows = [
            # (pincel, tamanho, cor, custom, traços)
            ("Redondo duro", 26, "#c03020", {"spacing": 1.2, "sub_jitter": 1.0}, [line(80, 560, 80)]),
            ("Redondo duro", 26, "#2050c0", {"hue_jitter": 0.5, "jitter_per_stroke": True}, [line(640, 1120, 60), line(640, 1120, 95), line(640, 1120, 130)]),
            ("Redondo duro", 70, "#203040", {"follow_direction": True, "spray": {"count": 25, "size": 0.35, "sync": True, "size_min": 0.8, "roundness_min": 0.08, "bias": 0.2, "count_exp": 0, "spread": 0.6, "align": True}}, [line(80, 560, 240, wave=40)]),
            ("Redondo duro", 70, "#203040", {"follow_direction": True, "spray": {"count": 25, "size": 0.35, "sync": True, "size_min": 0.8, "roundness_min": 0.08, "bias": 0.2, "count_exp": 0, "spread": 0.6, "align": False}}, [line(640, 1120, 240, wave=40)]),
            ("Redondo duro", 80, "#3070c0", {"wet_edges": 0.8}, [line(80, 560, 420)]),
            ("Redondo duro", 80, "#3070c0", {"wet_edges": 0.8, "wet_darkness": 1.0, "wet_width": 10}, [line(640, 1120, 420)]),
            ("Redondo duro", 90, "#402818", {"texture": {"kind": "canvas", "scale": 6, "depth": 1.0, "mode": "multiply", "seed": 2, "invert": False}, "flow": 0.3}, [line(80, 560, 590)]),
            ("Redondo duro", 90, "#402818", {"texture": {"kind": "canvas", "scale": 6, "depth": 1.0, "mode": "multiply", "seed": 2, "invert": False, "rotation": 45}, "flow": 0.3}, [line(640, 1120, 590)]),
            ("Redondo duro", 60, "#808080", {"mode": "darken"}, [[(80 + 13 * i, 360 + 9 * i) for i in range(81)]]),
            ("Redondo duro", 3, "#101010", {"size_pressure_min": 0.0}, [line(80, 560, 820)]),
            ("Redondo duro", 3, "#101010", {"size_pressure_min": 0.0, "min_one_px": True}, [line(640, 1120, 820)]),
        ]
        for name, size, color, custom, strokes in rows:
            r = aiai.call("UI_ACTION", {"action": "set_brush", "preset": name, "size": size, "opacity": 1.0, "custom": custom})
            aiai.call("UI_ACTION", {"action": "set_color", "color": color})
            ca.settle(2)
            pr = 0.15 if custom.get("size_pressure_min") == 0.0 else 0.9
            for s in strokes:
                stroke(cv, s, pr)
        ca.settle(20)
        r = aiai.call("RENDER", {"cut": cut, "frame": 0, "camera": False}, timeout=60)
        out = os.path.join(ca.TMP, "brush_options_sheet.png")
        with open(out, "wb") as f:
            f.write(base64.b64decode(r["png_base64"]))
        res["sheet"] = out
        # configurações avançadas: um pincel com os blocos ligados (mistura, spray, grão, textura, aquarela)
        aiai.call("UI_ACTION", {"action": "set_brush", "preset": "Redondo duro", "size": 30, "opacity": 1.0, "custom": {
            "paint_mix": {"amount": 0.6, "density": 0.8, "stretch": 0.3, "smear": False},
            "spray": {"count": 8, "size": 0.15, "sync": True, "size_min": 0.5, "roundness_min": 1.0, "bias": 0.3, "count_exp": 0, "spread": 1.0, "align": True},
            "grain": {"kind": "paper", "scale": 3, "depth": 0.5, "contrast": 1, "brightness": 0, "subtract": False, "moving": False, "seed": 5},
            "texture": {"kind": "paper", "scale": 6, "depth": 0.5, "mode": "multiply", "seed": 2, "invert": False, "rotation": 30},
            "wet_edges": 0.5, "wet_darkness": 0.4, "wet_width": 6}})
        ca.settle(4)
        # configurações avançadas: categorias
        res["detail_opened"] = click("tprop:detail")
        ca.settle(6)
        shots = {}
        for cat in ["size", "ink", "shape", "spray", "stroke", "texture", "watercolor", "erase", "correction"]:
            ok = click(f"tdet:cat:{cat}")
            ca.settle(4)
            p = os.path.join(ca.TMP, f"brush_options_{cat}.png")
            aiai.call("SCREENSHOT", {"out": p})
            # a lista de parâmetros rola: segunda foto com o fim dela
            d = aiai.call("GET_UI_STATE").get("widgets", {}).get("dialog:tool_detail")
            p2 = None
            if d:
                r = d["rect_logical"]
                x, y = r[0] + r[2] * 0.7, r[1] + r[3] * 0.6
                aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "wheel", "dx": 0, "dy": -600}, {"kind": "wait", "frames": 4}]})
                ca.settle(4)
                p2 = os.path.join(ca.TMP, f"brush_options_{cat}_2.png")
                aiai.call("SCREENSHOT", {"out": p2})
            shots[cat] = (ok, p, p2)
        res["shots"] = shots
        print(json.dumps(res, ensure_ascii=False, indent=1))
    finally:
        if not a.keep:
            proc.kill()


if __name__ == "__main__":
    main()
