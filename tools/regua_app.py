"""Conferência das réguas S25 no app de verdade (instância de teste fora da tela, projeto próprio em calibracao/tmp, bridge
própria, sem sessão): cria pela ferramenta Régua (mouse injetado pela bridge) uma régua linear, uma curva por cliques, uma
figura elipse, um polígono, uma caneta de régua, círculos concêntricos e uma guia; desenha traços de caneta (camada
vetorial) e de pincel (camada raster) começando perto de cada uma e tira capturas da janela.

Uso:
    python tools/regua_app.py --out sprints/registro/S25-clip-studio/evidencias/funcionais [--keep]
"""
import argparse
import math
import os
import subprocess
import sys
import time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
sys.path.insert(0, os.path.join(ROOT, "tools", "csp"))
sys.path.insert(0, os.path.join(ROOT, "tools"))
TMP = os.path.join(ROOT, "calibracao", "tmp")
DISCOVERY = os.path.join(TMP, "s25_regua_bridge.json")
PROJECT = os.path.join(TMP, "s25_regua.aiai")
os.environ["AIAI_BRIDGE_DISCOVERY"] = DISCOVERY
import aiai  # noqa: E402
from calib_app import launch_hidden, wait_ready, settle, click_widget_if, made  # noqa: E402

# ordem de NewKind::ALL (sub-ferramentas da Régua)
KINDS = ["linear", "curve", "fig_rect", "fig_ellipse", "fig_polygon", "pen", "parallel", "radial", "concentric", "guide", "p1", "p2", "p3", "symmetry", "grid"]


def assert_test_project():
    path = (aiai.call("GET_UI_STATE")["project"]["path"] or "")
    if os.path.normcase(os.path.abspath(path)) != os.path.normcase(os.path.abspath(PROJECT)):
        raise SystemExit(f"a instância não está no projeto de teste ({path!r}); nada foi feito")


def cv():
    return aiai.call("GET_UI_STATE")["editor"]["canvas"]


def scr(x, y):
    c = cv()
    m, o = c["m"], c["origin_logical"]
    return round(o[0] + m[0] * x + m[1] * y, 2), round(o[1] + m[2] * x + m[3] * y, 2)


def mouse(path, button_up=True):
    """Arrasto do mouse pelos pontos do canvas (o primeiro aperta, o último solta)."""
    pts = [scr(x, y) for x, y in path]
    ev = [{"kind": "mouse_move", "x": pts[0][0], "y": pts[0][1]}, {"kind": "mouse_down", "x": pts[0][0], "y": pts[0][1]}]
    for x, y in pts[1:]:
        ev.append({"kind": "mouse_move", "x": x, "y": y})
    if button_up:
        ev.append({"kind": "mouse_up", "x": pts[-1][0], "y": pts[-1][1]})
    aiai.call("INPUT", {"events": ev, "wait": True}, timeout=60)
    settle(3)


def click(x, y):
    mouse([(x, y)])


def key(k):
    aiai.call("INPUT", {"events": [{"kind": "key", "key": k}], "wait": True})
    settle(2)


def line(a, b, n=24):
    return [(a[0] + (b[0] - a[0]) * i / n, a[1] + (b[1] - a[1]) * i / n) for i in range(n + 1)]


def pen_stroke(path):
    pts = [scr(x, y) for x, y in path]
    ev = []
    for i, (x, y) in enumerate(pts):
        e = {"kind": "pen", "x": x, "y": y, "pressure": 0.8, "phase": "down" if i == 0 else "move"}
        if i:
            e["t_ms"] = i * 8.0
        ev.append(e)
    ev.append({"kind": "pen", "x": pts[-1][0], "y": pts[-1][1], "pressure": 0.0, "phase": "up", "t_ms": len(pts) * 8.0})
    rev0 = aiai.call("GET_UI_STATE")["project"]["revision"]
    aiai.call("INPUT", {"events": ev, "wait": True}, timeout=60)
    t0 = time.time()
    while time.time() - t0 < 20:
        s = aiai.call("GET_UI_STATE")
        if s["project"]["revision"] > rev0 and not s["editor"]["gesture_active"]:
            return
        time.sleep(0.05)
    raise SystemExit("o traço não virou commit")


def pick_kind(name):
    aiai.call("UI_ACTION", {"action": "set_tool", "tool": "ruler"})
    settle(3)
    i = KINDS.index(name)
    for _ in range(12):
        if click_widget_if(f"subtool:item:{i}"):
            settle(2)
            return
        # a lista rola: roda o mouse em cima do primeiro item visível
        st = aiai.call("GET_UI_STATE")["widgets"]
        first = st.get("subtool:item:0") or next((v for k, v in st.items() if k.startswith("subtool:item:")), None)
        if not first:
            break
        r = first.get("rect_logical") or first.get("rect")
        aiai.call("INPUT", {"events": [{"kind": "mouse_move", "x": (r[0] + r[2]) / 2, "y": (r[1] + r[3]) / 2}, {"kind": "wheel", "dx": 0, "dy": -60}], "wait": True})
        settle(2)
    raise SystemExit(f"sub-ferramenta {name} não apareceu")


def shot(out, name):
    p = os.path.join(out, f"regua_{name}.png")
    aiai.screenshot(p)
    print(p)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--exe", default=os.path.join(ROOT, "app", "target", "release", "aiai-studio.exe"))
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    if os.path.exists(DISCOVERY):
        os.remove(DISCOVERY)
    proc = launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--workspace", "layout"])
    print(f"app de teste PID {proc.pid}")
    try:
        wait_ready(proc)
        time.sleep(1.5)
        click_widget_if("warn:ok")
        aiai.call("NEW_PROJECT", {"name": "s25_regua", "settings": {"width": 1920, "height": 1080}, "path": PROJECT, "overwrite": True})
        settle()
        assert_test_project()
        ep = made("EP", None, {"type": "episode", "number": 1})
        sc = made("S", ep, {"type": "scene", "number": 1})
        cut = made("C001", sc, {"type": "cut", "number": 1, "duration": 12})
        aiai.call("UI_ACTION", {"action": "select_cut", "cut": cut})
        vec = made("linhas", cut, {"type": "layer", "kind": "vector"})
        made("c1", vec, {"type": "cel", "number": 1})
        ras = made("pincel", cut, {"type": "layer", "kind": "raster"})
        made("c1", ras, {"type": "cel", "number": 1})
        aiai.call("UI_ACTION", {"action": "select_layer", "layer": vec})
        aiai.call("UI_ACTION", {"action": "set_color", "color": "#101010"})
        settle()
        aiai.call("UI_ACTION", {"action": "fit_view"})
        settle(6)

        # --- criar pela ferramenta Régua
        pick_kind("linear")
        mouse(line((150, 160), (900, 420)))
        pick_kind("curve")
        for p in [(150, 820), (400, 620), (650, 860), (900, 640), (1100, 780)]:
            click(*p)
        key("Enter")
        pick_kind("fig_ellipse")
        mouse(line((1150, 120), (1500, 380)))
        pick_kind("fig_polygon")
        mouse(line((1700, 300), (1700, 160)))
        pick_kind("pen")
        mouse([(1200 + 30 * i, 600 + 120 * math.sin(i / 3.0)) for i in range(22)])
        pick_kind("concentric")
        mouse(line((1450, 880), (1600, 960)))
        pick_kind("guide")
        mouse(line((100, 1000), (600, 1004)))
        settle(4)
        shot(a.out, "criadas")
        assert_test_project()

        # --- caneta (vetor): começa perto de cada régua e a mão desvia
        aiai.call("UI_ACTION", {"action": "set_tool", "tool": "pen"})
        aiai.call("UI_ACTION", {"action": "select_layer", "layer": vec})
        settle(3)
        pen_stroke([(260 + 12 * i, 240 + 6 * i + 10 * math.sin(i / 2.0)) for i in range(40)])          # linear (a ~25 px)
        pen_stroke([(170 + 25 * i, 840 - 6 * i + 18 * math.sin(i / 3)) for i in range(40)])             # curva
        pen_stroke([(1500 + 20, 250 + 6 * i) for i in range(30)])                                       # elipse (de pé, ao lado)
        pen_stroke([(1620 + 4 * i, 330 - 8 * i) for i in range(20)])                                    # polígono
        pen_stroke([(1600 + 6 * i, 960 + 3 * i) for i in range(30)])                                    # concêntricos (anel de fora)
        pen_stroke([(120 + 18 * i, 1020 + 6 * math.sin(i)) for i in range(25)])                         # guia
        pen_stroke([(700 + 5 * i, 520 + 4 * i) for i in range(30)])                                     # longe de tudo: livre
        settle(4)
        shot(a.out, "caneta")
        # --- pincel (raster)
        aiai.call("UI_ACTION", {"action": "set_tool", "tool": "brush"})
        aiai.call("UI_ACTION", {"action": "select_layer", "layer": ras})
        aiai.call("UI_ACTION", {"action": "set_color", "color": "#d03030"})
        settle(3)
        pen_stroke([(600 + 12 * i, 360 + 4 * i) for i in range(25)])                                    # linear, do outro lado
        pen_stroke([(640 + 20 * i, 880 - 10 * i) for i in range(30)])                                   # curva
        pen_stroke([(1300 + 10 * i, 360 + 5 * i * math.cos(i / 4)) for i in range(25)])                 # elipse (em baixo)
        pen_stroke([(1400 - 5 * i, 900 - 6 * i) for i in range(20)])                                    # concêntricos (anel de dentro)
        settle(4)
        shot(a.out, "pincel")
        aiai.call("SAVE_PROJECT", {})
    finally:
        if not a.keep:
            proc.terminate()
            try:
                proc.wait(15)
            except subprocess.TimeoutExpired:
                proc.kill()


if __name__ == "__main__":
    main()
