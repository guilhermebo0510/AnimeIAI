"""Multivista da Modelagem (seção 4) numa instância de teste: um toro, depois

  1. Vista > Quatro vistas (pelo menu, clique de verdade): perspectiva, frente, direita e topo;
  2. a vista 3 (topo) vira "Costas" pelo menu do nome no canto dela;
  3. pela ponte (UI_ACTION modeling_view): duas lado a lado, e de volta a uma.

Confere o estado (GET_UI_STATE modeling.views) e grava calibracao/tmp/mdl_views_<passo>.png.
Uso: python tools/csp/modeling_views_test.py [--exe ...] [--keep]
"""
import argparse
import json
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(__file__))
import calib_app as ca  # noqa: E402
import aiai  # noqa: E402

PROJECT = os.path.join(ca.TMP, "modeling_views_test.aiai")


def click(name, frames=2):
    st = aiai.call("GET_UI_STATE")
    w = st.get("widgets", {}).get(name)
    if not w:
        return False
    r = w["rect_logical"]
    x, y = r[0] + r[2] / 2, r[1] + r[3] / 2
    aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "primary"},
                                                {"kind": "mouse_up", "x": x, "y": y, "button": "primary"}, {"kind": "wait", "frames": frames}]})
    return True


def views():
    st = aiai.call("GET_UI_STATE")
    m = st.get("modeling") or st.get("editor", {}).get("modeling") or {}
    return m.get("views")


def shot(name):
    p = os.path.join(ca.TMP, f"mdl_views_{name}.png")
    aiai.call("SCREENSHOT", {"out": p})
    return p


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
        aiai.call("NEW_PROJECT", {"name": "vistas", "settings": {"width": 640, "height": 360}, "path": PROJECT, "overwrite": True})
        ca.settle(6)
        ca.assert_test_project()
        res["add"] = click("mdl:add.torus")
        ca.settle(10)
        res["single"] = views()
        res["mdl_widgets"] = sorted(k for k in aiai.call("GET_UI_STATE").get("widgets", {}) if k.startswith("mdl:v") or k.startswith("mdl:l"))
        # 1. menu Vista > Quatro vistas
        # com o cabeçalho estreito, Vista fica dentro de "Mais"
        if "mdl:view" not in aiai.call("GET_UI_STATE").get("widgets", {}):
            click("mdl:more")
        res["menu_quad"] = click("mdl:view") and click("mdl:layout.quad")
        ca.settle(12)
        res["quad"] = views()
        res["shot_quad"] = shot("quad")
        # 2. vista 3: Costas pelo nome no canto
        res["kind_menu"] = click("mdl:view3.kind") and click("mdl:view3.kind.back")
        ca.settle(10)
        res["after_kind"] = views()
        res["shot_back"] = shot("back")
        # 3. ponte: lado a lado e uma vista
        res["split_v"] = aiai.call("UI_ACTION", {"action": "modeling_view", "layout": "split_v"})
        ca.settle(10)
        res["shot_split"] = shot("split_v")
        res["single_again"] = aiai.call("UI_ACTION", {"action": "modeling_view", "layout": "single"})
        ca.settle(6)
        q = res["quad"] or {}
        k = res["after_kind"] or {}
        res["checks"] = {
            "quatro vistas": q.get("layout") == "quad" and q.get("kinds", [])[1:] == ["front", "right", "top"],
            "vista 3 = costas": (list(k.get("kinds") or []) + [None] * 4)[3] == "back",
            "lado a lado": res["split_v"].get("layout") == "split_v",
            "volta a uma": res["single_again"].get("layout") == "single",
        }
        print(json.dumps(res, ensure_ascii=False, indent=1))
        if not all(res["checks"].values()):
            raise SystemExit("FALHOU")
        print("ok: multivista")
    finally:
        if not a.keep:
            proc.kill()


if __name__ == "__main__":
    main()
