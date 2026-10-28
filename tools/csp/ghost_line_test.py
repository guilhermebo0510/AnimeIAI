"""Linha fantasma da caneta (auditoria 4.8e) numa instância de teste, pela bridge (sem mouse do sistema):

  1. uma linha vertical em x = 500;
  2. um traço horizontal que para em x = 440 (60 px antes dela), a caneta parada ~0,4 s, Tab, e solta:
     o traço tem que terminar EXATAMENTE na vertical (fechamento de junção), sem voltar para onde a caneta soltou;
  3. um traço tremido; depois de soltar, Tab troca pela versão sem tremido (menos pontos, mesma ponta).

Grava a tela em calibracao/tmp/ghost_line_test.png. Uso: python tools/csp/ghost_line_test.py [--exe ...] [--keep]
"""
import argparse
import json
import math
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(__file__))
import calib_app as ca  # noqa: E402
import aiai  # noqa: E402

PROJECT = os.path.join(ca.TMP, "ghost_line_test.aiai")


def find_key(v, key):
    if isinstance(v, dict):
        if key in v:
            return v[key]
        for x in v.values():
            r = find_key(x, key)
            if r is not None:
                return r
    elif isinstance(v, list):
        for x in v:
            r = find_key(x, key)
            if r is not None:
                return r
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=os.path.join(ca.ROOT, "app", "target", "debug", "aiai-studio.exe"))
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    if os.path.exists(PROJECT):
        shutil.rmtree(PROJECT)
    proc = ca.launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--workspace", "animacao"])
    try:
        ca.wait_ready(proc)
        ca.settle(10)
        ca.click_widget_if("warn:ok")
        ca.PROJECT = PROJECT
        aiai.call("NEW_PROJECT", {"name": "fantasma", "settings": {"width": 900, "height": 600}, "path": PROJECT, "overwrite": True})
        ca.settle(6)
        ca.assert_test_project()
        ep = ca.made("EP", None, {"type": "episode", "number": 1})
        sc = ca.made("S", ep, {"type": "scene", "number": 1})
        cut = ca.made("C001", sc, {"type": "cut", "number": 1, "duration": 12})
        layer = ca.made("linhas", cut, {"type": "layer", "kind": "vector"})
        cel = ca.made("c1", layer, {"type": "cel", "number": 1})
        aiai.call("UI_ACTION", {"action": "select_cut", "cut": cut})
        aiai.call("UI_ACTION", {"action": "select_layer", "layer": layer})
        aiai.call("UI_ACTION", {"action": "set_tool", "tool": "pen"})
        aiai.call("UI_ACTION", {"action": "set_color", "color": "#000000"})
        ca.settle(4)
        aiai.call("UI_ACTION", {"action": "fit_view"})
        ca.settle(6)
        aiai.call("UI_ACTION", {"action": "set_tool_option", "key": "pen_ghost", "value": True})
        ca.settle(4)
        if not find_key(aiai.call("GET_UI_STATE"), "pen_ghost"):
            raise SystemExit("a opção Linha fantasma não ligou")
        st = aiai.call("GET_UI_STATE")
        cv = st["editor"]["canvas"]

        def pen(points, tail=None):
            evs = []
            for i, (x, y) in enumerate(points):
                sx, sy = ca.to_screen(cv, x, y)
                evs.append({"kind": "pen", "x": round(sx, 3), "y": round(sy, 3), "pressure": 0.8, "phase": "down" if i == 0 else "move", "t_ms": i * 8.0})
            evs += tail or []
            sx, sy = ca.to_screen(cv, *points[-1])
            evs.append({"kind": "pen", "x": round(sx, 3), "y": round(sy, 3), "pressure": 0.0, "phase": "up"})
            aiai.call("INPUT", {"events": evs, "wait": True}, timeout=60)
            ca.settle(8)

        def strokes():
            return find_key(aiai.call("QUERY", {"query": {"kind": "node", "id": cel}}), "strokes") or []

        # 1. vertical
        pen([(500, 100 + i * 10) for i in range(31)])
        # 2. horizontal que para antes, parada, Tab, solta
        pen([(200 + i * 6, 250) for i in range(41)], tail=[{"kind": "wait", "frames": 30}, {"kind": "key", "key": "Tab"}, {"kind": "wait", "frames": 3}])
        ss = strokes()
        h = ss[-1]["points"]
        end = (h[-1]["x"], h[-1]["y"])
        xs = [q["x"] for q in h]
        res = {"strokes": len(ss), "end_of_horizontal": [round(end[0], 2), round(end[1], 2)], "max_x": round(max(xs), 2)}
        # 3. tremido + Tab depois de soltar
        shaky = [(150 + i * 4, 450 + 3.0 * math.sin(i * 1.9)) for i in range(120)]
        pen(shaky)
        before = strokes()[-1]
        aiai.call("INPUT", {"events": [{"kind": "key", "key": "Tab"}, {"kind": "wait", "frames": 3}], "wait": True})
        ca.settle(6)
        ss_after = strokes()
        # o traço tremido é o que começa em x = 150, y ~ 450 (a troca pode mudar a ordem)
        after = next((x for x in ss_after if abs(x["points"][0]["x"] - 150) < 3 and abs(x["points"][0]["y"] - 450) < 6), ss_after[-1])
        res["strokes_after_tab"] = [(len(x["points"]), round(x["points"][0]["x"]), round(x["points"][0]["y"])) for x in ss_after]

        def wobble(pts):
            # desvio da reta y = 450 (os pontos do meio; as pontas ficam onde a mão começou e terminou)
            mid = pts[1:-1] or pts
            return max(abs(q["y"] - 450.0) for q in mid)
        res["shaky_points"] = [len(before["points"]), len(after["points"])]
        res["shaky_wobble_px"] = [round(wobble(before["points"]), 2), round(wobble(after["points"]), 2)]
        shot = os.path.join(ca.TMP, "ghost_line_test.png")
        aiai.call("SCREENSHOT", {"out": shot})
        checks = {
            "fecha na vertical (x = 500)": abs(end[0] - 500) <= 2.0 and abs(end[1] - 250) <= 3.0,
            "não volta para onde a caneta soltou": abs(end[0] - max(xs)) < 0.5,
            "embelezar trocou o traço": res["shaky_points"][1] != res["shaky_points"][0],
            "sem o tremido": res["shaky_wobble_px"][1] < 0.6 and res["shaky_wobble_px"][1] < 0.5 * res["shaky_wobble_px"][0],
        }
        res["checks"] = checks
        res["screenshot"] = shot
        print(json.dumps(res, ensure_ascii=False, indent=1))
        if not all(checks.values()):
            raise SystemExit("FALHOU: " + ", ".join(k for k, v in checks.items() if not v))
        print("ok: linha fantasma")
    finally:
        if not a.keep:
            proc.kill()


if __name__ == "__main__":
    main()
