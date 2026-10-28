"""Editor de UV da Modelagem (seção 4) numa instância de teste: um cubo, modo Edição, costuras em cruz, desdobrar e
empacotar pela ponte (UI_ACTION uv); o painel Editor de UV aberto; um clique de verdade numa ilha do editor 2D (a
seleção volta para o 3D) e G para mover os UVs escolhidos. Fotos em calibracao/tmp/uv_<passo>.png.

Uso: python tools/csp/uv_test.py [--exe ...] [--keep]
"""
import argparse
import json
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(__file__))
import calib_app as ca  # noqa: E402
import aiai  # noqa: E402

PROJECT = os.path.join(ca.TMP, "uv_test.aiai")


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
    p = os.path.join(ca.TMP, f"uv_{name}.png")
    aiai.call("SCREENSHOT", {"out": p})
    return p


def uv(**params):
    r = aiai.call("UI_ACTION", {"action": "uv", **params}, timeout=120)
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
        aiai.call("NEW_PROJECT", {"name": "uv", "settings": {"width": 640, "height": 360}, "path": PROJECT, "overwrite": True})
        ca.settle(6)
        ca.assert_test_project()
        res["add"] = click("mdl:add.cube")
        ca.settle(6)
        res["seams"] = uv(edit=True, select="tree_seams", op="mark_seam")
        res["unwrap"] = uv(select="none", select_mode="face", op="unwrap")
        res["pack"] = uv(op="pack")
        aiai.call("UI_ACTION", {"action": "open_panel", "panel": "model_uv"})
        ca.settle(10)
        res["shot_unwrap"] = shot("unwrap")
        # clique de verdade no meio de uma ilha do editor 2D: escolhe a face (ilha com Alt) e a seleção 3D acompanha
        cv = widgets().get("mdl:uv_canvas")
        res["canvas"] = bool(cv)
        if cv:
            r = cv["rect_logical"]
            x, y = r[0] + r[2] * 0.5, r[1] + r[3] * 0.5
            aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "primary"},
                                                        {"kind": "mouse_up", "x": x, "y": y, "button": "primary"}, {"kind": "wait", "frames": 3}]})
            ca.settle(6)
            res["after_click"] = aiai.call("GET_UI_STATE")["editor"]["modeling"]["selection"]
            # G move os UVs escolhidos; arrasta, clica para confirmar
            aiai.call("INPUT", {"wait": True, "events": [{"kind": "key", "key": "G", "pressed": True}, {"kind": "key", "key": "G", "pressed": False}, {"kind": "wait", "frames": 2},
                                                        {"kind": "mouse_move", "x": x + 60, "y": y - 40}, {"kind": "wait", "frames": 2},
                                                        {"kind": "key", "key": "Enter", "pressed": True}, {"kind": "key", "key": "Enter", "pressed": False}, {"kind": "wait", "frames": 3}]})
            ca.settle(6)
        res["shot_moved"] = shot("moved")
        h = aiai.call("GET_UI_STATE")["editor"]["modeling"]["history"]
        res["history"] = h
        res["checks"] = {
            "costuras em cruz": res["seams"]["selection"]["edges"] > 0 and "Marcar costura" in res["seams"]["history"],
            "desdobrou numa ilha": res["unwrap"]["islands"] == 1 and res["unwrap"]["flipped"] == 0,
            "painel do editor de UV": res["canvas"],
            "clique escolhe no 3D": (res.get("after_click") or {}).get("faces", 0) >= 1,
            "G move os UVs (desfazer)": any(x.startswith("Mover UV") for x in h),
        }
        print(json.dumps(res, ensure_ascii=False, indent=1))
        if not all(res["checks"].values()):
            raise SystemExit("FALHOU")
        print("ok: uv")
    finally:
        if not a.keep:
            proc.kill()


if __name__ == "__main__":
    main()
