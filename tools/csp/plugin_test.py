"""Plugins .aiaiplugin (auditoria 4.9g) numa instância de teste, com os exemplos de plugins/exemplos:

  1. PLUGIN_LIST acha os três;
  2. cada um aplicado (PLUGIN_FILTER) numa cópia de uma pintura (céu + círculo), render salvo para conferir a olho;
  3. pelo menu do app: Filtro > Plugins > Posterizar, janela, Aplicar, e a camada muda (um passo de desfazer).

Grava calibracao/tmp/plugin_<id>.png e plugin_test.png. Uso: python tools/csp/plugin_test.py [--exe ...] [--keep]
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

PROJECT = os.path.join(ca.TMP, "plugin_test.aiai")
EXAMPLES = os.path.join(ca.ROOT, "plugins", "exemplos")


def click(name):
    """Clique como o da suíte do app: mover, apertar, soltar e 2 quadros (o menu abre e fica aberto)."""
    st = aiai.call("GET_UI_STATE")
    w = st.get("widgets", {}).get(name)
    if not w:
        return False
    r = w["rect_logical"]
    x, y = r[0] + r[2] / 2, r[1] + r[3] / 2
    aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "primary"},
                                                {"kind": "mouse_up", "x": x, "y": y, "button": "primary"}, {"kind": "wait", "frames": 2}]})
    return True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=os.path.join(ca.ROOT, "app", "target", "debug", "aiai-studio.exe"))
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    if os.path.exists(PROJECT):
        shutil.rmtree(PROJECT)
    os.environ["AIAI_PLUGINS_DIR"] = EXAMPLES
    proc = ca.launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--workspace", "pintura"])
    res = {}
    try:
        ca.wait_ready(proc)
        ca.settle(10)
        ca.click_widget_if("warn:ok")
        ca.PROJECT = PROJECT
        aiai.call("NEW_PROJECT", {"name": "plugins", "settings": {"width": 480, "height": 270}, "path": PROJECT, "overwrite": True})
        ca.settle(6)
        ca.assert_test_project()
        ep = ca.made("EP", None, {"type": "episode", "number": 1})
        sc = ca.made("S", ep, {"type": "scene", "number": 1})
        cut = ca.made("C001", sc, {"type": "cut", "number": 1, "duration": 12})
        lst = aiai.call("PLUGIN_LIST", {})
        res["listed"] = [p["id"] for p in lst["plugins"]]
        res["problems"] = lst["problems"]

        def painted(name):
            layer = ca.made(name, cut, {"type": "layer", "kind": "raster"})
            ca.made("c1", layer, {"type": "cel", "number": 1})
            aiai.call("SKY", {"cut": cut, "frame": 0, "layer": layer, "sky": {"time_of_day": "sunset", "horizon_y": 200, "clouds": 0.5, "cloud_scale": 60}})
            aiai.call("FILL_SHAPE", {"cut": cut, "frame": 0, "layer": layer, "shapes": [{"points": [[240 + 70 * __import__("math").cos(t / 20 * 6.283), 140 + 70 * __import__("math").sin(t / 20 * 6.283)] for t in range(20)]}],
                                     "color": {"type": "direct", "rgba": [0.2, 0.35, 0.8, 1.0]}})
            return layer

        for pid in ["aiai.posterizar", "aiai.contorno", "aiai.reticula"]:
            layer = painted(pid)
            aiai.call("PLUGIN_FILTER", {"cut": cut, "frame": 0, "layer": layer, "plugin": pid, "params": {}})
            r = aiai.call("RENDER", {"cut": cut, "frame": 0, "layers": [layer], "background": "#ffffff"}, timeout=60)
            out = os.path.join(ca.TMP, f"plugin_{pid.split('.')[-1]}.png")
            with open(out, "wb") as f:
                f.write(base64.b64decode(r["png_base64"]))
            res[pid] = out
        # pelo menu
        layer = painted("menu")
        aiai.call("UI_ACTION", {"action": "select_cut", "cut": cut})
        aiai.call("UI_ACTION", {"action": "select_layer", "layer": layer})
        ca.settle(6)
        rev0 = aiai.call("GET_UI_STATE")["project"]["revision"]
        ok = click("menu:filter")
        ok = ok and click("menu_item:filter.group.plugins")
        ok = ok and click("menu_item:plugin:aiai.posterizar")
        ca.settle(4)
        ok = ok and click("plugin:apply")
        ca.settle(8)
        st = aiai.call("GET_UI_STATE")
        res["menu_clicks"] = bool(ok)
        res["menu_revision"] = [rev0, st["project"]["revision"]]
        shot = os.path.join(ca.TMP, "plugin_test.png")
        aiai.call("SCREENSHOT", {"out": shot})
        res["screenshot"] = shot
        res["checks"] = {
            "lista os 3": sorted(res["listed"]) == ["aiai.contorno", "aiai.posterizar", "aiai.reticula"],
            "menu aplicou (1 passo)": res["menu_clicks"] and res["menu_revision"][1] == rev0 + 1,
        }
        print(json.dumps(res, ensure_ascii=False, indent=1))
        if not all(res["checks"].values()):
            raise SystemExit("FALHOU")
        print("ok: plugins")
    finally:
        if not a.keep:
            proc.kill()


if __name__ == "__main__":
    main()
