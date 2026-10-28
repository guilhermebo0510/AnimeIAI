"""Retângulo sujo do canvas (auditoria 4.4g) na instância de teste: fundo raster pintado, camada de pintura, linha
vetorial e sombra (multiplicar) por cima; pinceladas na pintura uma a uma. O canvas (render com cache, refazendo só os
tiles mudados) tem de ficar igual ao render completo do mesmo quadro: compara a captura da tela com o RENDER.

Grava calibracao/tmp/dirty_rect_canvas.png (tela) e dirty_rect_render.png. Uso: python tools/csp/dirty_rect_test.py
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

PROJECT = os.path.join(ca.TMP, "dirty_rect_test.aiai")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=os.path.join(ca.ROOT, "app", "target", "debug", "aiai-studio.exe"))
    a = ap.parse_args()
    if os.path.exists(PROJECT):
        shutil.rmtree(PROJECT)
    proc = ca.launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--workspace", "pintura"])
    try:
        ca.wait_ready(proc)
        ca.settle(10)
        ca.click_widget_if("warn:ok")
        ca.PROJECT = PROJECT
        aiai.call("NEW_PROJECT", {"name": "sujo", "settings": {"width": 1280, "height": 720}, "path": PROJECT, "overwrite": True})
        ca.settle(6)
        ca.assert_test_project()
        ep = ca.made("EP", None, {"type": "episode", "number": 1})
        sc = ca.made("S", ep, {"type": "scene", "number": 1})
        cut = ca.made("C001", sc, {"type": "cut", "number": 1, "duration": 12})
        bg = ca.made("fundo", cut, {"type": "layer", "kind": "raster"})
        ca.made("c1", bg, {"type": "cel", "number": 1})
        aiai.call("SKY", {"cut": cut, "frame": 0, "layer": bg, "sky": {"time_of_day": "day", "horizon_y": 600, "clouds": 0.4, "cloud_scale": 80}})
        paint = ca.made("pintura", cut, {"type": "layer", "kind": "raster"})
        ca.made("c1", paint, {"type": "cel", "number": 1})
        line = ca.made("linha", cut, {"type": "layer", "kind": "vector"})
        ca.made("c1", line, {"type": "cel", "number": 1})
        aiai.call("DRAW_CURVES", {"cut": cut, "frame": 0, "layer": line, "curves": [{"points": [[100, 600, 1], [640, 120, 1], [1180, 600, 1]], "width": 8, "tip": "g_pen", "color": {"type": "direct", "rgba": [0.1, 0.1, 0.1, 1]}}]})
        shade = ca.made("sombra", cut, {"type": "layer", "kind": "vector", "blend": "multiply", "opacity": 0.6})
        ca.made("c1", shade, {"type": "cel", "number": 1})
        aiai.call("DRAW_CURVES", {"cut": cut, "frame": 0, "layer": shade, "curves": [{"points": [[200, 200, 1], [1100, 500, 1]], "width": 60, "tip": "marker", "color": {"type": "direct", "rgba": [0.4, 0.4, 0.7, 1]}}]})
        aiai.call("UI_ACTION", {"action": "select_cut", "cut": cut})
        aiai.call("UI_ACTION", {"action": "select_layer", "layer": paint})
        aiai.call("UI_ACTION", {"action": "set_tool", "tool": "brush"})
        aiai.call("UI_ACTION", {"action": "set_brush", "preset": "Redondo duro", "size": 40})
        aiai.call("UI_ACTION", {"action": "set_color", "color": "#e04020"})
        ca.settle(4)
        aiai.call("UI_ACTION", {"action": "fit_view"})
        ca.settle(8)
        cv = aiai.call("GET_UI_STATE")["editor"]["canvas"]
        for k in range(6):
            pts = [(150 + 170 * k + 8 * i, 300 + 25 * k + 5 * i) for i in range(12)]
            evs = []
            for i, (x, y) in enumerate(pts):
                sx, sy = ca.to_screen(cv, x, y)
                evs.append({"kind": "pen", "x": round(sx, 2), "y": round(sy, 2), "pressure": 0.9, "phase": "down" if i == 0 else "move", "t_ms": i * 8.0})
            sx, sy = ca.to_screen(cv, *pts[-1])
            evs.append({"kind": "pen", "x": round(sx, 2), "y": round(sy, 2), "pressure": 0.0, "phase": "up"})
            aiai.call("INPUT", {"events": evs, "wait": True}, timeout=60)
            ca.settle(20)
        ca.settle(30)
        shot = os.path.join(ca.TMP, "dirty_rect_canvas.png")
        aiai.call("SCREENSHOT", {"out": shot, "panel": "canvas"})
        r = aiai.call("RENDER", {"cut": cut, "frame": 0, "camera": False}, timeout=60)
        rp = os.path.join(ca.TMP, "dirty_rect_render.png")
        with open(rp, "wb") as f:
            f.write(base64.b64decode(r["png_base64"]))
        st = aiai.call("GET_STATUS", {})
        print(json.dumps({"canvas": shot, "render": rp, "status_last_render": {k: v for k, v in st.items() if "render" in k}}, ensure_ascii=False, indent=1))
    finally:
        proc.kill()


if __name__ == "__main__":
    main()
