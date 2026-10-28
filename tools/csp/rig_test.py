"""Rigging 2D (auditoria 4.6d) numa instância de teste, pela bridge (sem mouse do sistema):

  braço (pivô no ombro, 300,200) e antebraço (pivô no cotovelo, 380,200; ponta do osso na mão, 450,200), o antebraço
  filho do braço. Na Composição, com Girar (W), arrasta a mão até (380, 300): o IK gira o braço e o antebraço e a mão
  fica no alvo; o desenho do antebraço acompanha o braço.

Grava calibracao/tmp/rig_test.png (tela) e rig_render.png (quadro). Uso: python tools/csp/rig_test.py [--exe ...] [--keep]
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

PROJECT = os.path.join(ca.TMP, "rig_test.aiai")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=os.path.join(ca.ROOT, "app", "target", "debug", "aiai-studio.exe"))
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    if os.path.exists(PROJECT):
        shutil.rmtree(PROJECT)
    proc = ca.launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--workspace", "composicao"])
    try:
        ca.wait_ready(proc)
        ca.settle(10)
        ca.click_widget_if("warn:ok")
        ca.PROJECT = PROJECT
        aiai.call("NEW_PROJECT", {"name": "rig", "settings": {"width": 600, "height": 400}, "path": PROJECT, "overwrite": True})
        ca.settle(6)
        ca.assert_test_project()
        ep = ca.made("EP", None, {"type": "episode", "number": 1})
        sc = ca.made("S", ep, {"type": "scene", "number": 1})
        cut = ca.made("C001", sc, {"type": "cut", "number": 1, "duration": 12})
        ink = {"type": "direct", "rgba": [0.1, 0.1, 0.1, 1.0]}

        def limb(name, x0, x1, anchor):
            layer = ca.made(name, cut, {"type": "layer", "kind": "vector", "anchor": anchor})
            ca.made("c1", layer, {"type": "cel", "number": 1})
            aiai.call("DRAW_CURVES", {"cut": cut, "frame": 0, "layer": layer,
                                      "curves": [{"points": [[x0, 200, 1.0], [x1, 200, 1.0]], "width": 14.0, "tip": "marker", "color": ink}]})
            return layer

        upper = limb("braço", 300, 380, [0.0, 0.0])
        lower = limb("antebraço", 380, 450, [80.0, 0.0])
        aiai.call("RIG", {"op": "parent", "layer": lower, "parent": upper})
        aiai.call("RIG", {"op": "bone_tip", "layer": lower, "tip": [450, 200]})
        aiai.call("UI_ACTION", {"action": "select_cut", "cut": cut})
        aiai.call("UI_ACTION", {"action": "select_layer", "layer": lower})
        aiai.call("UI_ACTION", {"action": "set_tool", "tool": "rotate"})
        ca.settle(4)
        aiai.call("UI_ACTION", {"action": "fit_view"})
        ca.settle(8)
        st = aiai.call("GET_UI_STATE")
        cv = st["editor"]["canvas"]
        x0, y0 = ca.to_screen(cv, 450, 200)
        x1, y1 = ca.to_screen(cv, 380, 300)
        evs = [{"kind": "mouse_move", "x": x0, "y": y0}, {"kind": "mouse_down", "x": x0, "y": y0}]
        for i in range(1, 11):
            f = i / 10
            evs.append({"kind": "mouse_move", "x": x0 + (x1 - x0) * f, "y": y0 + (y1 - y0) * f})
        evs.append({"kind": "mouse_up", "x": x1, "y": y1})
        aiai.call("INPUT", {"events": evs, "wait": True})
        ca.settle(8)
        node = lambda i: aiai.call("QUERY", {"query": {"kind": "node", "id": i}})
        def rot_keys(i):
            n = node(i)
            s = json.dumps(n)
            d = n.get("node", n).get("data", {}) if isinstance(n, dict) else {}
            keys = (d.get("keys") or {}).get("rotation") if isinstance(d, dict) else None
            return keys
        res = {"upper_rotation_keys": rot_keys(upper), "lower_rotation_keys": rot_keys(lower)}
        shot = os.path.join(ca.TMP, "rig_test.png")
        aiai.call("SCREENSHOT", {"out": shot})
        r = aiai.call("RENDER", {"cut": cut, "frame": 0, "background": "#ffffff"}, timeout=60)
        rp = os.path.join(ca.TMP, "rig_render.png")
        with open(rp, "wb") as f:
            f.write(base64.b64decode(r["png_base64"]))
        # a mão: pixel escuro perto de (380, 300) e o cotovelo saiu do lugar de repouso
        from PIL import Image
        im = Image.open(rp).convert("L")
        dark = lambda x, y: min(im.getpixel((x + dx, y + dy)) for dx in range(-4, 5) for dy in range(-4, 5)) < 100
        res["hand_at_target"] = dark(380, 296)
        res["rest_hand_gone"] = not dark(445, 200)
        res["screenshot"], res["render"] = shot, rp
        res["checks"] = {"chaves de rotação nos dois": bool(res["upper_rotation_keys"]) and bool(res["lower_rotation_keys"]),
                         "a mão foi para o alvo": res["hand_at_target"], "saiu da pose de repouso": res["rest_hand_gone"]}
        print(json.dumps(res, ensure_ascii=False, indent=1))
        if not all(res["checks"].values()):
            raise SystemExit("FALHOU")
        print("ok: rigging")
    finally:
        if not a.keep:
            proc.kill()


if __name__ == "__main__":
    main()
