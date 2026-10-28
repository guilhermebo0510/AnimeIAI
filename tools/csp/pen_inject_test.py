"""Teste da caneta nativa (auditoria 4.1c–f) com caneta SINTÉTICA do próprio Windows (InjectSyntheticPointerInput,
Windows 10 1809+): pressão que muda a cada amostra, inclinação e a ponta de borracha (caneta virada), entregues como
uma mesa digitalizadora de verdade entregaria, pelo Windows Ink, para uma instância de teste do app.

O que confere (na instância de teste, num projeto de teste em calibracao/tmp):
  1. traço de caneta (vetorial) com a pressão subindo de 0,1 a 1,0 e a inclinação indo de (0, 0) a (40, -20):
     os pontos gravados têm pressões diferentes entre si (não a mesma repetida) e a inclinação de cada um;
  2. a mesma caneta virada (PEN_FLAG_INVERTED) passando por cima do traço apaga com a borracha, e a ferramenta volta
     a ser a caneta depois de soltar.

Segura `calibracao/tmp/ui.lock` (a caneta sintética é entrada de verdade do sistema, no lugar do mouse), põe a janela
de teste na frente, injeta, e devolve tudo como estava. Não mexe no projeto nem no app do usuário.

Uso: python tools/csp/pen_inject_test.py [--exe app/target/debug/aiai-studio.exe] [--keep]
"""
import argparse
import ctypes
import math
import json
import os
import shutil
import sys
import time
from ctypes import wintypes

sys.path.insert(0, os.path.dirname(__file__))
import calib_app as ca  # noqa: E402  (define AIAI_BRIDGE_DISCOVERY da instância de teste)
import aiai  # noqa: E402

ROOT = ca.ROOT
LOCK = os.path.join(ca.TMP, "ui.lock")
PROJECT = os.path.join(ca.TMP, "pen_native_test.aiai")
u32 = ctypes.windll.user32
# coordenadas físicas da tela (com a tela a 125%, um processo sem DPI recebe as da janela escaladas e a caneta
# sintética cairia fora do lugar)
u32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # PER_MONITOR_AWARE_V2

PT_PEN = 3
POINTER_FLAG_INRANGE = 0x2
POINTER_FLAG_INCONTACT = 0x4
POINTER_FLAG_FIRSTBUTTON = 0x10
POINTER_FLAG_DOWN = 0x10000
POINTER_FLAG_UPDATE = 0x20000
POINTER_FLAG_UP = 0x40000
PEN_FLAG_INVERTED = 0x2
PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X, PEN_MASK_TILT_Y = 1, 2, 4, 8
FIRSTBUTTON_DOWN, FIRSTBUTTON_UP = 1, 2


class POINTER_INFO(ctypes.Structure):
    _fields_ = [("pointerType", ctypes.c_uint32), ("pointerId", ctypes.c_uint32), ("frameId", ctypes.c_uint32),
                ("pointerFlags", ctypes.c_uint32), ("sourceDevice", ctypes.c_void_p), ("hwndTarget", ctypes.c_void_p),
                ("ptPixelLocation", wintypes.POINT), ("ptHimetricLocation", wintypes.POINT),
                ("ptPixelLocationRaw", wintypes.POINT), ("ptHimetricLocationRaw", wintypes.POINT),
                ("dwTime", ctypes.c_uint32), ("historyCount", ctypes.c_uint32), ("InputData", ctypes.c_int32),
                ("dwKeyStates", ctypes.c_uint32), ("PerformanceCount", ctypes.c_uint64), ("ButtonChangeType", ctypes.c_int32)]


class POINTER_PEN_INFO(ctypes.Structure):
    _fields_ = [("pointerInfo", POINTER_INFO), ("penFlags", ctypes.c_uint32), ("penMask", ctypes.c_uint32),
                ("pressure", ctypes.c_uint32), ("rotation", ctypes.c_uint32), ("tiltX", ctypes.c_int32), ("tiltY", ctypes.c_int32)]


class POINTER_TOUCH_INFO(ctypes.Structure):
    _fields_ = [("pointerInfo", POINTER_INFO), ("touchFlags", ctypes.c_uint32), ("touchMask", ctypes.c_uint32),
                ("rcContact", wintypes.RECT), ("rcContactRaw", wintypes.RECT), ("orientation", ctypes.c_uint32), ("pressure", ctypes.c_uint32)]


class _U(ctypes.Union):
    _fields_ = [("touchInfo", POINTER_TOUCH_INFO), ("penInfo", POINTER_PEN_INFO)]


class POINTER_TYPE_INFO(ctypes.Structure):
    _anonymous_ = ("u",)
    _fields_ = [("type", ctypes.c_uint32), ("u", _U)]


u32.CreateSyntheticPointerDevice.restype = ctypes.c_void_p
u32.CreateSyntheticPointerDevice.argtypes = [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32]
u32.InjectSyntheticPointerInput.argtypes = [ctypes.c_void_p, ctypes.POINTER(POINTER_TYPE_INFO), ctypes.c_uint32]
u32.DestroySyntheticPointerDevice.argtypes = [ctypes.c_void_p]


class Pen:
    def __init__(self):
        self.dev = u32.CreateSyntheticPointerDevice(PT_PEN, 1, 1)
        if not self.dev:
            raise SystemExit(f"CreateSyntheticPointerDevice falhou ({ctypes.GetLastError()})")

    def send(self, x, y, flags, pressure=0.0, tilt=(0, 0), inverted=False, button=0):
        info = POINTER_TYPE_INFO()
        info.type = PT_PEN
        pi = info.penInfo
        pi.pointerInfo.pointerType = PT_PEN
        pi.pointerInfo.pointerId = 1
        pi.pointerInfo.pointerFlags = flags
        pi.pointerInfo.ptPixelLocation = wintypes.POINT(int(round(x)), int(round(y)))
        pi.pointerInfo.ButtonChangeType = button
        pi.penFlags = PEN_FLAG_INVERTED if inverted else 0
        pi.penMask = PEN_MASK_PRESSURE | PEN_MASK_TILT_X | PEN_MASK_TILT_Y
        pi.pressure = int(max(0.0, min(1.0, pressure)) * 1024)
        pi.tiltX, pi.tiltY = int(tilt[0]), int(tilt[1])
        if not u32.InjectSyntheticPointerInput(self.dev, ctypes.byref(info), 1):
            raise SystemExit(f"InjectSyntheticPointerInput falhou ({ctypes.GetLastError()})")

    def stroke(self, pts, inverted=False, dt=0.008):
        """pts: [(x, y, pressão, (tiltX, tiltY))] em px da tela."""
        x0, y0 = pts[0][0], pts[0][1]
        self.send(x0, y0, POINTER_FLAG_INRANGE | POINTER_FLAG_UPDATE, inverted=inverted)
        time.sleep(0.05)
        base = POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_FIRSTBUTTON
        for i, (x, y, p, t) in enumerate(pts):
            if i == 0:
                self.send(x, y, base | POINTER_FLAG_DOWN, p, t, inverted, FIRSTBUTTON_DOWN)
            else:
                self.send(x, y, base | POINTER_FLAG_UPDATE, p, t, inverted)
            time.sleep(dt)
        x, y = pts[-1][0], pts[-1][1]
        self.send(x, y, POINTER_FLAG_INRANGE | POINTER_FLAG_UP, 0.0, pts[-1][3], inverted, FIRSTBUTTON_UP)
        time.sleep(0.03)
        self.send(x, y, POINTER_FLAG_UPDATE, inverted=inverted)  # sai do alcance
        time.sleep(0.1)

    def close(self):
        u32.DestroySyntheticPointerDevice(self.dev)


def find_key(v, key):
    """Primeiro valor de `key` em qualquer nível do JSON."""
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


def cel_strokes(cel):
    n = aiai.call("QUERY", {"query": {"kind": "node", "id": cel}})
    return find_key(n, "strokes") or []


def window_of(pid):
    found = []
    cb_t = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def cb(hwnd, _):
        p = wintypes.DWORD()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        if p.value == pid and u32.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True
    u32.EnumWindows(cb_t(cb), 0)
    return found[0] if found else None


def client_origin(hwnd):
    pt = wintypes.POINT(0, 0)
    u32.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=os.path.join(ROOT, "app", "target", "debug", "aiai-studio.exe"))
    ap.add_argument("--keep", action="store_true", help="deixa a instância aberta no fim")
    a = ap.parse_args()
    os.makedirs(ca.TMP, exist_ok=True)
    try:
        fd = os.open(LOCK, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    except FileExistsError:
        raise SystemExit("ui.lock ocupado: outra checagem de interface está usando o mouse")
    os.write(fd, b"pen_inject_test")
    os.close(fd)
    proc = None
    pen = None
    try:
        if os.path.exists(PROJECT):
            shutil.rmtree(PROJECT)
        proc = ca.launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1400x900", "--workspace", "animacao"])
        ca.wait_ready(proc)
        ca.settle(10)
        ca.click_widget_if("warn:ok")
        ca.PROJECT = PROJECT
        aiai.call("NEW_PROJECT", {"name": "caneta_nativa", "settings": {"width": 900, "height": 600}, "path": PROJECT, "overwrite": True})
        ca.settle(6)
        ca.assert_test_project()
        ep = ca.made("EP", None, {"type": "episode", "number": 1})
        sc = ca.made("S", ep, {"type": "scene", "number": 1})
        cut = ca.made("C001", sc, {"type": "cut", "number": 1, "duration": 12})
        layer = ca.made("traços", cut, {"type": "layer", "kind": "vector"})
        cel = ca.made("c1", layer, {"type": "cel", "number": 1})
        aiai.call("UI_ACTION", {"action": "select_cut", "cut": cut})
        aiai.call("UI_ACTION", {"action": "select_layer", "layer": layer})
        aiai.call("UI_ACTION", {"action": "set_tool", "tool": "pen"})
        aiai.call("UI_ACTION", {"action": "set_color", "color": "#000000"})
        ca.settle(4)
        aiai.call("UI_ACTION", {"action": "fit_view"})
        ca.settle(6)
        # janela de teste na frente, dentro da tela, sem mudar o tamanho (mudar o tamanho desloca a vista)
        hwnd = window_of(proc.pid)
        u32.SetWindowPos(hwnd, -1, 60, 40, 0, 0, 0x0040 | 0x0001)  # HWND_TOPMOST, SWP_SHOWWINDOW | SWP_NOSIZE
        u32.SetForegroundWindow(hwnd)
        time.sleep(0.6)
        ca.settle(6)
        st = aiai.call("GET_UI_STATE")
        cv = st["editor"]["canvas"]
        ppp = find_key(st, "pixels_per_point") or 1.0
        ox, oy = client_origin(hwnd)

        def screen(x, y):
            lx, ly = ca.to_screen(cv, x, y)
            return ox + lx * ppp, oy + ly * ppp

        pen = Pen()
        n = 60
        pts = []
        for i in range(n):
            f = i / (n - 1)
            # onda: a caneta vetorial guarda mais pontos que numa reta (que vira 2 ao simplificar)
            sx, sy = screen(150 + 600 * f, 300 + 60 * math.sin(f * 2 * math.pi))
            pts.append((sx, sy, 0.1 + 0.9 * f, (round(40 * f), round(-20 * f))))
        rev0 = aiai.call("GET_UI_STATE")["project"]["revision"]
        pen.stroke(pts)
        ca.settle(10)
        st = aiai.call("GET_UI_STATE")
        result = {"ppp": ppp, "rev": [rev0, st["project"]["revision"]], "tool_after_pen": find_key(st, "active_tool")}
        strokes = cel_strokes(cel)
        result["strokes"] = len(strokes)
        if strokes:
            ptsq = strokes[0]["points"]
            pr = [round(p["pressure"], 3) for p in ptsq]
            tl = [p.get("tilt") for p in ptsq]
            result["points"] = len(ptsq)
            result["pressure_first_last"] = [pr[0], pr[-1]]
            result["distinct_pressures"] = len(set(pr))
            result["tilt_first_last"] = [tl[0], tl[-1]]
        # caneta virada por cima do traço: apaga, e a ferramenta volta a ser a caneta
        er = []
        for i in range(30):
            f = i / 29
            # passa pelo meio da onda (x = 450, y = 300) de cima a baixo
            sx, sy = screen(450, 200 + 200 * f)
            er.append((sx, sy, 0.8, (0, 0)))
        pen.stroke(er, inverted=True)
        ca.settle(10)
        st = aiai.call("GET_UI_STATE")
        result["strokes_after_eraser"] = len(cel_strokes(cel))
        result["tool_after_eraser"] = find_key(st, "active_tool")
        shot = os.path.join(ca.TMP, "pen_native_test.png")
        aiai.call("SCREENSHOT", {"out": shot})
        result["screenshot"] = shot
        # o meio da onda (x de 446 a 454) ficou sem traço
        spans = []
        for stv in cel_strokes(cel):
            xs = [q["x"] for q in stv["points"]]
            spans.append((min(xs), max(xs)))
        result["spans_after_eraser"] = [[round(a, 1), round(b, 1)] for a, b in spans]
        checks = {
            "traço gravado": result["strokes"] == 1,
            "pressão por amostra (sobe de ~0,1 a 1)": result.get("pressure_first_last", [1, 1])[0] < 0.3 and result["pressure_first_last"][1] > 0.9,
            "pressões diferentes ao longo do traço": result.get("distinct_pressures", 0) >= 4,
            "inclinação gravada (0,0 -> 40,-20)": result.get("tilt_first_last") == [[0.0, 0.0], [40.0, -20.0]],
            "a borracha da caneta apagou o meio": not any(a < 446 and b > 454 for a, b in spans) and len(spans) >= 2,
            "a ferramenta voltou à caneta": result["tool_after_eraser"] == "pen",
        }
        result["checks"] = checks
        print(json.dumps(result, ensure_ascii=False, indent=1))
        if not all(checks.values()):
            raise SystemExit("FALHOU: " + ", ".join(k for k, v in checks.items() if not v))
        print("ok: caneta nativa (pressão por amostra, inclinação, ponta de borracha)")
    finally:
        if pen:
            pen.close()
        if proc and not a.keep:
            proc.kill()
        try:
            os.remove(LOCK)
        except OSError:
            pass


if __name__ == "__main__":
    main()
