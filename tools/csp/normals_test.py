"""Normais personalizadas da Modelagem (seção 4) numa instância de teste: dois cubos lado a lado na prévia Toon; no da
direita, as normais viram as de uma esfera (UI_ACTION normals from_center): ele passa a sombrear como uma bola, sem
mudar a forma. Foto em calibracao/tmp/normals_cubes.png.

Uso: python tools/csp/normals_test.py [--exe ...] [--keep]
"""
import argparse
import json
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(__file__))
import calib_app as ca  # noqa: E402
import aiai  # noqa: E402

PROJECT = os.path.join(ca.TMP, "normals_test.aiai")


def click(name, frames=2):
    w = aiai.call("GET_UI_STATE").get("widgets", {}).get(name)
    if not w:
        return False
    r = w["rect_logical"]
    x, y = r[0] + r[2] / 2, r[1] + r[3] / 2
    aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "primary"},
                                                {"kind": "mouse_up", "x": x, "y": y, "button": "primary"}, {"kind": "wait", "frames": frames}]})
    return True


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
        aiai.call("NEW_PROJECT", {"name": "normais", "settings": {"width": 640, "height": 360}, "path": PROJECT, "overwrite": True})
        ca.settle(6)
        ca.assert_test_project()
        click("mdl:add.cube")
        ca.settle(4)
        click("mdl:duplicate")
        ca.settle(4)
        # o segundo cubo vai para a direita pela transformação do painel (MODELING é do documento do projeto; aqui é a UI)
        st = aiai.call("GET_UI_STATE")["editor"]["modeling"]
        res["objects"] = st["objects"]
        aiai.call("UI_ACTION", {"action": "uv", "edit": True})
        ca.settle(4)
        res["normals"] = aiai.call("UI_ACTION", {"action": "normals", "op": "from_center"})
        aiai.call("UI_ACTION", {"action": "uv", "edit": False})
        ca.settle(10)
        p = os.path.join(ca.TMP, "normals_cubes.png")
        aiai.call("SCREENSHOT", {"out": p})
        res["shot"] = p
        res["checks"] = {"normais trocadas": res["normals"]["corners"] >= 24, "um passo de desfazer": res["normals"]["history"][-1].startswith("Da esfera") or res["normals"]["history"][-1].startswith("From sphere")}
        print(json.dumps(res, ensure_ascii=False, indent=1))
        if not all(res["checks"].values()):
            raise SystemExit("FALHOU")
        print("ok: normais")
    finally:
        if not a.keep:
            proc.kill()


if __name__ == "__main__":
    main()
