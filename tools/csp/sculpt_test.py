"""Escultura da Modelagem (seção 4) numa instância de teste: uma esfera, modo Escultura pelo botão do cabeçalho,
traços pela ponte (desenhar, inflar, máscara, alisar tudo, remalhar) e um traço com o mouse de verdade; fotos em
calibracao/tmp/sculpt_<passo>.png e o histórico no fim.

Uso: python tools/csp/sculpt_test.py [--exe ...] [--keep]
"""
import argparse
import json
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(__file__))
import calib_app as ca  # noqa: E402
import aiai  # noqa: E402

PROJECT = os.path.join(ca.TMP, "sculpt_test.aiai")


def widgets():
    return aiai.call("GET_UI_STATE").get("widgets", {})


def click(name, frames=2):
    w = widgets().get(name)
    if not w:
        return False
    r = w["rect_logical"]
    x, y = r[0] + r[2] / 2, r[1] + r[3] / 2
    aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "primary"},
                                                {"kind": "mouse_up", "x": x, "y": y, "button": "primary"}, {"kind": "wait", "frames": frames}]})
    return True


def shot(name):
    p = os.path.join(ca.TMP, f"sculpt_{name}.png")
    aiai.call("SCREENSHOT", {"out": p})
    return p


def sculpt(**kw):
    kw["action"] = "sculpt"
    r = aiai.call("UI_ACTION", kw, timeout=120)
    ca.settle(6)
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=os.path.join(ca.ROOT, "app", "target", "debug", "aiai-studio.exe"))
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    if os.path.exists(PROJECT):
        shutil.rmtree(PROJECT)
    proc = ca.launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--workspace", "modelagem"])
    res = {}
    try:
        ca.wait_ready(proc)
        ca.settle(10)
        ca.click_widget_if("warn:ok")
        ca.PROJECT = PROJECT
        aiai.call("NEW_PROJECT", {"name": "escultura", "settings": {"width": 640, "height": 360}, "path": PROJECT, "overwrite": True})
        ca.settle(6)
        ca.assert_test_project()
        res["add"] = click("mdl:add.sphere")
        ca.settle(8)
        # vista de frente enquadrada na esfera
        aiai.call("UI_ACTION", {"action": "modeling_view", "view": "front"})
        ca.settle(4)
        res["mode_click"] = click("mdl:mode.sculpt")
        ca.settle(8)
        res["mode"] = aiai.call("GET_UI_STATE")["editor"]["modeling"]["mode"]
        res["remesh"] = sculpt(op="voxel_remesh")
        res["draw"] = sculpt(brush="draw", radius=60, paths=[[[0.40, 0.45], [0.50, 0.40], [0.60, 0.45]]])
        res["inflate"] = sculpt(brush="inflate", radius=40, paths=[[[0.45, 0.62], [0.55, 0.62]]])
        res["shot_strokes"] = shot("strokes")
        res["mask"] = sculpt(brush="mask", radius=70, paths=[[[0.40, 0.56], [0.47, 0.56]]])
        res["shot_mask"] = shot("mask")
        # traço com o mouse de verdade no meio do viewport (pincel desenhar)
        sculpt(brush="draw", radius=50)
        vp = widgets().get("mdl:viewport")
        if vp:
            r = vp["rect_logical"]
            pts = [(r[0] + r[2] * (0.42 + 0.16 * i / 20), r[1] + r[3] * 0.47) for i in range(21)]
            evs = [{"kind": "mouse_move", "x": pts[0][0], "y": pts[0][1]}, {"kind": "mouse_down", "x": pts[0][0], "y": pts[0][1], "button": "primary"}]
            evs += [{"kind": "mouse_move", "x": x, "y": y} for x, y in pts[1:]]
            evs += [{"kind": "mouse_up", "x": pts[-1][0], "y": pts[-1][1], "button": "primary"}, {"kind": "wait", "frames": 3}]
            aiai.call("INPUT", {"wait": True, "events": evs})
            ca.settle(6)
            # deixa o cursor sobre a esfera para o círculo do pincel aparecer
            aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": r[0] + r[2] * 0.62, "y": r[1] + r[3] * 0.5}, {"kind": "wait", "frames": 3}]})
            ca.settle(4)
        st = aiai.call("GET_UI_STATE")["editor"]["modeling"]
        res["after_mouse"] = {"history": st["history"], "sculpt": st["sculpt"]}
        res["shot_mouse"] = shot("mouse")
        res["smooth_all"] = sculpt(op="smooth_all")
        res["out"] = sculpt(mode=False)
        h = res["after_mouse"]["history"]
        res["checks"] = {
            "modo escultura pelo botão": res["mode"] == "sculpt",
            "remalhar": "Remalhar (voxel)" in res["remesh"]["history"],
            "traços da ponte esculpem": (res["draw"]["dabs"] or 0) > 0 and (res["inflate"]["dabs"] or 0) > 0,
            "traço do mouse entra no desfazer": sum(1 for x in h if x.startswith("Escultura")) >= 4,
            "círculo do pincel": res["after_mouse"]["sculpt"]["hover"],
            "sai da escultura": res["out"]["mode"] == "object",
        }
        print(json.dumps(res, ensure_ascii=False, indent=1))
        if not all(res["checks"].values()):
            raise SystemExit("FALHOU")
        print("ok: escultura")
    finally:
        if not a.keep:
            proc.kill()


if __name__ == "__main__":
    main()
