"""Marcadores da Edição (seção 5) numa cópia do projeto do usuário: com o ponteiro na linha do tempo, M no quadro 0,
clique na régua em ~8 s e M de novo, Shift+M volta... (Ctrl+Shift+M vai ao anterior), e a foto mostra as bandeirinhas.
Foto em calibracao/tmp/edit_markers.png.

Uso: python tools/csp/edit_markers_test.py [--keep]
"""
import argparse
import json
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(__file__))
import calib_app as ca  # noqa: E402
import aiai  # noqa: E402


def widgets():
    return aiai.call("GET_UI_STATE").get("widgets", {})


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=os.path.join(ca.ROOT, "app", "target", "debug", "aiai-studio.exe"))
    ap.add_argument("--src", default=os.path.join(ca.ROOT, "producoes", "micro-anime", "ultima-rosa.aiai"))
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    dst = os.path.join(ca.TMP, "edit_markers.aiai")
    if os.path.exists(dst):
        shutil.rmtree(dst)
    shutil.copytree(a.src, dst, ignore=shutil.ignore_patterns("*.lock", "lock"))
    proc = ca.launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--project", dst, "--workspace", "edicao"])
    res = {}
    try:
        ca.wait_ready(proc)
        ca.settle(30)
        ca.click_widget_if("warn:ok")
        ca.PROJECT = dst
        ws = widgets()
        ruler = ws["edit:ruler"]["rect_logical"]
        rx, ry, rw = ruler[0], ruler[1] + ruler[3] / 2, ruler[2]
        hover_y = ruler[1] + ruler[3] + 60

        def at(frac):
            x = rx + rw * frac
            aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": x, "y": ry}, {"kind": "mouse_down", "x": x, "y": ry, "button": "primary"},
                                                        {"kind": "mouse_up", "x": x, "y": ry, "button": "primary"}, {"kind": "wait", "frames": 2},
                                                        {"kind": "mouse_move", "x": x, "y": hover_y}, {"kind": "wait", "frames": 2}]})

        def key(k, mods=None):
            ev = {"kind": "key", "key": k}
            if mods:
                ev["modifiers"] = mods
            aiai.call("INPUT", {"wait": True, "events": [ev, {"kind": "wait", "frames": 3}]})
            ca.settle(4)

        for frac in (0.15, 0.45, 0.70):
            at(frac)
            key("M")
        frames = sorted(int(k.split(":")[-1]) for k in widgets() if k.startswith("edit:marker:"))
        res["markers"] = frames
        at(0.30)
        key("M", ["shift"])
        res["after_next"] = aiai.call("GET_UI_STATE")["editor"].get("edit_frame")
        p = os.path.join(ca.TMP, "edit_markers.png")
        aiai.call("SCREENSHOT", {"out": p})
        res["shot"] = p
        res["checks"] = {"três marcadores": len(frames) == 3}
        print(json.dumps(res, ensure_ascii=False, indent=1))
        if not all(res["checks"].values()):
            raise SystemExit("FALHOU")
        print("ok: marcadores")
    finally:
        if not a.keep:
            proc.kill()


if __name__ == "__main__":
    main()
