"""Conferência no app de verdade: abre uma instância de teste do AnimeIAI Studio (projeto novo em calibracao/tmp, arquivo de
descoberta da bridge próprio, sem sessão), e desenha a folha de teste com a caneta INJETADA pela bridge (`INPUT` pen, um
ponto a cada 8 ms com `t_ms`, pelo mesmo caminho de uma caneta de verdade: estabilizador, amostras, prévia ao vivo e
commit). Cada folha vai numa camada raster nova; o PNG da camada (papel branco) sai em `--out` para o
`calib_compare.py`. Não mexe no mouse do sistema nem no projeto do usuário.

Uso:
    python tools/csp/calib_app.py --out calibracao/csp/ours/app [--pens g-pen,felt-pen] [--sizes 3,10,30] [--quant 0]
"""
import argparse
import base64
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from csp_sheet import sheet  # noqa: E402
from calib_render import FACTORY, PENS  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TMP = os.path.join(ROOT, "calibracao", "tmp")
DISCOVERY = os.path.join(TMP, "s25_canetas_bridge.json")
PROJECT = os.path.join(TMP, "s25_canetas.aiai")
os.environ["AIAI_BRIDGE_DISCOVERY"] = DISCOVERY
import aiai  # noqa: E402  (lê AIAI_BRIDGE_DISCOVERY a cada chamada)


def launch_hidden(args):
    """Abre o app de teste sem tomar o foco e o põe fora da área das telas, atrás das outras janelas: não aparece na
    frente de quem está usando outro programa (nem atrapalha capturas dele), e o mouse do sistema nunca passa por ele."""
    si = None
    if os.name == "nt":
        si = subprocess.STARTUPINFO()
        si.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        si.wShowWindow = 4  # SW_SHOWNOACTIVATE
    proc = subprocess.Popen(args, cwd=ROOT, env=dict(os.environ), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, startupinfo=si)
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes
        u32 = ctypes.windll.user32
        vx, vw = u32.GetSystemMetrics(76), u32.GetSystemMetrics(78)
        found = []
        cb_t = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

        def cb(hwnd, _):
            pid = wintypes.DWORD()
            u32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            if pid.value == proc.pid and u32.IsWindowVisible(hwnd):
                found.append(hwnd)
            return True

        t0 = time.time()
        while time.time() - t0 < 30 and not found:
            u32.EnumWindows(cb_t(cb), 0)
            time.sleep(0.05)
        for hwnd in found:
            # HWND_BOTTOM, sem ativar; à direita da última tela
            u32.SetWindowPos(hwnd, 1, vx + vw + 200, 40, 1500, 980, 0x0010)
    return proc


def wait_ready(proc, timeout=90):
    t0 = time.time()
    while time.time() - t0 < timeout:
        if proc.poll() is not None:
            raise SystemExit(f"o app fechou (código {proc.returncode})")
        try:
            aiai.call("PING")
            return
        except Exception:
            time.sleep(0.5)
    raise SystemExit("o app não respondeu à bridge")


def settle(frames=4):
    aiai.call("INPUT", {"events": [{"kind": "wait", "frames": frames}], "wait": True})


def click_widget_if(name):
    st = aiai.call("GET_UI_STATE")
    w = st.get("widgets", {}).get(name)
    if not w:
        return False
    r = w.get("rect_logical") or w.get("rect")
    x, y = (r[0] + r[2]) / 2, (r[1] + r[3]) / 2
    aiai.call("INPUT", {"events": [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y}, {"kind": "mouse_up", "x": x, "y": y}], "wait": True})
    return True


def made(name, parent, data):
    c = {"kind": "create_node", "name": name, "data": data}
    if parent:
        c["parent"] = parent
    return aiai.call("APPLY_COMMAND", {"command": c})["created"][0]


def assert_test_project():
    path = (aiai.call("GET_UI_STATE")["project"]["path"] or "")
    if os.path.normcase(os.path.abspath(path)) != os.path.normcase(os.path.abspath(PROJECT)):
        raise SystemExit(f"a instância não está no projeto de teste ({path!r}); nada foi feito")


def to_screen(cv, x, y):
    m, o = cv["m"], cv["origin_logical"]
    return o[0] + m[0] * x + m[1] * y, o[1] + m[2] * x + m[3] * y


def draw_sheet(cut, layer, size, quant, rev_wait=20.0):
    st = aiai.call("GET_UI_STATE")
    cv = st["editor"]["canvas"]
    for stroke in sheet(size):
        pts = stroke["points"]
        evs = []
        for i, (x, y, p) in enumerate(pts):
            if quant is not None:
                x, y = round(x - quant) + quant, round(y - quant) + quant
            sx, sy = to_screen(cv, x, y)
            e = {"kind": "pen", "x": round(sx, 3), "y": round(sy, 3), "pressure": p, "phase": "down" if i == 0 else "move"}
            if i > 0:
                e["t_ms"] = i * 8.0
            evs.append(e)
        last = evs[-1]
        evs.append({"kind": "pen", "x": last["x"], "y": last["y"], "pressure": 0.0, "phase": "up", "t_ms": len(pts) * 8.0})
        rev0 = aiai.call("GET_UI_STATE")["project"]["revision"]
        aiai.call("INPUT", {"events": evs, "wait": True}, timeout=120)
        t0 = time.time()
        while time.time() - t0 < rev_wait:
            s = aiai.call("GET_UI_STATE")
            if s["project"]["revision"] > rev0 and not s["editor"]["gesture_active"]:
                break
            time.sleep(0.05)
        else:
            raise SystemExit(f"o traço {stroke['name']} não virou commit (revisão {rev0})")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--pens", default=",".join(PENS))
    ap.add_argument("--sizes", default="3,10,30")
    ap.add_argument("--quant", type=float, default=0.0, help="grade inteira da entrada (mouse do Clip Studio); -1 = posições exatas")
    ap.add_argument("--exe", default=os.path.join(ROOT, "app", "target", "release", "aiai-studio.exe"))
    ap.add_argument("--keep", action="store_true", help="deixa o app aberto no fim")
    a = ap.parse_args()
    quant = None if a.quant < 0 else a.quant
    os.makedirs(a.out, exist_ok=True)
    os.makedirs(TMP, exist_ok=True)
    if os.path.exists(DISCOVERY):
        os.remove(DISCOVERY)
    proc = launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--workspace", "pintura"])
    print(f"app de teste PID {proc.pid}")
    try:
        wait_ready(proc)
        time.sleep(1.5)
        click_widget_if("warn:ok")
        aiai.call("NEW_PROJECT", {"name": "s25_canetas", "settings": {"width": 900, "height": 700}, "path": PROJECT, "overwrite": True})
        settle()
        assert_test_project()
        ep = made("EP", None, {"type": "episode", "number": 1})
        sc = made("S", ep, {"type": "scene", "number": 1})
        cut = made("C001", sc, {"type": "cut", "number": 1, "duration": 12})
        aiai.call("UI_ACTION", {"action": "select_cut", "cut": cut})
        aiai.call("UI_ACTION", {"action": "set_tool", "tool": "brush"})
        aiai.call("UI_ACTION", {"action": "set_color", "color": "#000000"})
        settle()
        aiai.call("UI_ACTION", {"action": "fit_view"})
        settle(6)
        done = 0
        for pen in [p for p in a.pens.split(",") if p]:
            for s in [int(x) for x in a.sizes.split(",")]:
                assert_test_project()
                layer = made(f"{pen}_s{s}", cut, {"type": "layer", "kind": "raster"})
                made("c1", layer, {"type": "cel", "number": 1})
                aiai.call("UI_ACTION", {"action": "select_layer", "layer": layer})
                size = 1 if pen == "dot-pen" else s
                aiai.call("UI_ACTION", {"action": "set_brush", "preset": FACTORY[pen], "size": size})
                settle()
                draw_sheet(cut, layer, s, quant)
                r = aiai.call("RENDER", {"cut": cut, "frame": 0, "layers": [layer], "background": "#ffffff"}, timeout=60)
                with open(os.path.join(a.out, f"{pen}_s{s}.png"), "wb") as f:
                    f.write(base64.b64decode(r["png_base64"]))
                done += 1
                print(f"{pen} s{s} ok")
        aiai.call("SAVE_PROJECT", {})
        print(f"{done} folhas em {a.out}")
    finally:
        if not a.keep:
            proc.terminate()
            try:
                proc.wait(15)
            except subprocess.TimeoutExpired:
                proc.kill()


if __name__ == "__main__":
    main()
