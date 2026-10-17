"""Cenário 1 (C001): jardim ao pôr do sol, casa de madeira à direita, cerca e roseiras; a última rosa em (1200, 800).
Tudo pintado com as ferramentas do app (SKY, GRADIENT, FILL_SHAPE, PATTERN_FILL, PAINT), em camadas de multiplano
(céu, morros, casa, cerca, roseiras). Reexecutável: cada camada tem o cel zerado antes de pintar."""
import math, random, sys
from curta import d, ids, node, OUT, rgba

cut = ids["cuts"][0]
LAYERS = ["bg_ceu", "bg_morros", "bg_casa", "bg_cerca", "bg_roseira"]


def layer(key):
    lid = node(f"c1_{key}", key, {"type": "layer", "kind": "raster"}, cut)
    cel = node(f"c1_{key}_c1", "1", {"type": "cel", "number": 1}, lid)
    d.apply({"kind": "replace_data", "id": cel, "data": {"type": "cel", "number": 1}})
    return lid


_call = d.call
d.call = lambda a, p=None, **kw: _call(a, {"frame": 0, **(p or {})} if a != "RENDER" else p, **kw)
L = {k: layer(k) for k in LAYERS}
for i, k in enumerate(LAYERS):
    d.apply({"kind": "move_node", "id": L[k], "parent": cut, "index": i + 1})


def C(h, a=1.0):
    return {"type": "direct", "rgba": rgba(h, a)}


def shape(lay, pts, color, **kw):
    if max(p[0] for p in pts) < 0 or min(p[0] for p in pts) > 1920:
        print("forma fora:", lay, [round(v) for v in pts[0]])
        return
    d.call("FILL_SHAPE", {"cut": cut, "layer": L[lay], "shapes": [{"points": pts, "geometry": kw.pop("geometry", "polyline")}], "color": C(color, kw.pop("alpha", 1.0)), **kw})


def shapes(lay, list_pts, color, geometry="polyline", **kw):
    d.call("FILL_SHAPE", {"cut": cut, "layer": L[lay], "shapes": [{"points": p, "geometry": geometry} for p in list_pts], "color": C(color, kw.pop("alpha", 1.0)), **kw})


def paint(lay, preset, color, strokes, alpha=1.0, seed=1, **over):
    d.call("PAINT", {"cut": cut, "layer": L[lay], "preset": preset, "preset_overrides": over, "color": C(color, alpha),
                     "strokes": [{"points": s} for s in strokes], "seed": seed})


rnd = random.Random(7)
HY = 600  # horizonte

# ---------- céu ----------
d.call("SKY", {"cut": cut, "layer": L["bg_ceu"], "sky": {"time_of_day": "sunset", "horizon_y": HY, "clouds": 0.35, "cloud_scale": 260, "seed": 11}})
d.call("GRADIENT", {"cut": cut, "layer": L["bg_ceu"], "kind": "radial", "from": [640, 548], "to": [640, 148],
                    "stops": [[0, "#fff1c8ff"], [0.12, "#ffd38acc"], [0.45, "#ff9a5a40"], [1, "#ff8a5000"]], "blend": "screen"})
shapes("bg_ceu", [[[640 + 46 * math.cos(a / 20 * math.tau), 548 + 46 * math.sin(a / 20 * math.tau)] for a in range(20)]], "#fff6dc", geometry="spline")

# ---------- morros ----------
def ridge(y0, amp, step, seed):
    r = random.Random(seed)
    pts, y = [], y0
    for x in range(-40, 1981, step):
        y = y0 + amp * math.sin(x / 310 + seed) + r.uniform(-amp, amp) * 0.35
        pts.append([x, y])
    return pts + [[1960, 1080], [-40, 1080]]

shape("bg_morros", ridge(520, 26, 60, 3), "#a77a8e", geometry="spline")
shape("bg_morros", ridge(560, 18, 50, 5), "#8a6a80", geometry="spline")
# bosque distante (copas em fila)
for x in range(-20, 1960, 34):
    y = 585 + 8 * math.sin(x / 140)
    r = rnd.uniform(18, 34)
    shape("bg_morros", [[x + r * math.cos(t / 10 * math.tau), y - r * 0.8 * (1 + math.sin(t / 10 * math.tau)) * 0.6] for t in range(10)], "#6d5a6e", geometry="spline")
# campo: degradê dourado → verde
shape("bg_morros", [[-40, HY], [1960, HY], [1960, 1080], [-40, 1080]], "#7d8a4a")
d.call("GRADIENT", {"cut": cut, "layer": L["bg_morros"], "kind": "linear", "from": [0, 0], "to": [0, 1080],
                    "stops": [[0, "#c8a86000"], [HY / 1080 - 0.001, "#c8a86000"], [HY / 1080, "#c8a860ff"], [0.68, "#99a052ff"], [0.9, "#5d7a3cff"], [1, "#3f5a2cff"]], "blend": "normal",
                    })
d.call("PATTERN_FILL", {"cut": cut, "layer": L["bg_morros"], "texture": {"spec": {"type": "grass", "scale": 3, "density": 0.7},
                        "dark": [0.30, 0.40, 0.20], "light": [0.75, 0.70, 0.40], "seed": 4, "contrast": 0.8, "opacity": 0.35},
                        "mapping": {"type": "perspective", "quad": [[-600, HY], [2520, HY], [2520, 1080], [-600, 1080]], "size": [1600, 900]}})

# ---------- casa (dois pontos de fuga: (-1400, 600) e (3300, 600)) ----------
VL, VR = (-1400.0, HY), (3300.0, HY)

def toward(p, v, t):
    return [p[0] + (v[0] - p[0]) * t, p[1] + (v[1] - p[1]) * t]

def on_line(p, v, x):  # ponto da reta p→v na abscissa x
    t = (x - p[0]) / (v[0] - p[0])
    return [x, p[1] + (v[1] - p[1]) * t]

K_top, K_bot = [1500, 360], [1500, 640]  # quina da frente
FL_t, FL_b = on_line(K_top, VL, 1290), on_line(K_bot, VL, 1290)  # fachada (esquerda, para VL)
SD_t, SD_b = on_line(K_top, VR, 1760), on_line(K_bot, VR, 1760)  # lateral (direita, para VR)
front = [FL_t, K_top, K_bot, FL_b]
side = [K_top, SD_t, SD_b, K_bot]
wood = {"spec": {"type": "wood", "scale": 6, "rings": 0.25}, "seed": 2, "contrast": 0.6}
d.call("PATTERN_FILL", {"cut": cut, "layer": L["bg_casa"], "texture": {**wood, "dark": [0.55, 0.36, 0.24], "light": [0.80, 0.58, 0.40]},
                        "mapping": {"type": "perspective", "quad": front, "size": [900, 540]}})
d.call("PATTERN_FILL", {"cut": cut, "layer": L["bg_casa"], "texture": {**wood, "dark": [0.36, 0.22, 0.20], "light": [0.52, 0.34, 0.30]},
                        "mapping": {"type": "perspective", "quad": side, "size": [700, 540]}})
# empena na lateral (frontão triangular voltado para a direita) + telhado
mid_t = toward(K_top, VR, 0.5 * (SD_t[0] - K_top[0]) / (VR[0] - K_top[0]) * 2) if False else [(K_top[0] + SD_t[0]) / 2, (K_top[1] + SD_t[1]) / 2]
apex = [mid_t[0], mid_t[1] - 150]
shape("bg_casa", [K_top, apex, SD_t], "#7a4a3e")
ridge_back = on_line(apex, VL, 1250)
roof = [apex, ridge_back, [FL_t[0] - 40, FL_t[1] + 6], [K_top[0] + 14, K_top[1] + 10]]
shape("bg_casa", roof, "#5a3a4a")
d.call("PATTERN_FILL", {"cut": cut, "layer": L["bg_casa"], "texture": {"spec": {"type": "roof_tiles", "tile": [28, 16], "variation": 0.3},
                        "dark": [0.30, 0.20, 0.28], "light": [0.55, 0.36, 0.40], "seed": 5, "contrast": 0.7},
                        "mapping": {"type": "perspective", "quad": [ridge_back, apex, roof[3], roof[2]], "size": [900, 300]}})
shape("bg_casa", [apex, [apex[0] + 6, apex[1] + 4], [SD_t[0] + 12, SD_t[1] + 8], [SD_t[0] + 4, SD_t[1] + 2]], "#3a2630")
shape("bg_casa", [[apex[0] - 3, apex[1] - 4], ridge_back, [ridge_back[0], ridge_back[1] + 6], [apex[0], apex[1] + 3]], "#ffcf9a", alpha=0.8)
# janelas acesas na fachada e porta
def quad_front(u0, u1, v0, v1):
    """retângulo na fachada: u ao longo (0 = quina, 1 = ponta esquerda), v na altura (0 = topo, 1 = base)"""
    def P(u, v):
        top = toward(K_top, VL, u * (K_top[0] - FL_t[0]) / (K_top[0] - VL[0]))
        bot = toward(K_bot, VL, u * (K_bot[0] - FL_b[0]) / (K_bot[0] - VL[0]))
        return [top[0] + (bot[0] - top[0]) * v, top[1] + (bot[1] - top[1]) * v]
    return [P(u0, v0), P(u1, v0), P(u1, v1), P(u0, v1)]

for u0 in (0.12, 0.62):
    shape("bg_casa", quad_front(u0, u0 + 0.22, 0.25, 0.55), "#3b2a28")
    shape("bg_casa", quad_front(u0 + 0.015, u0 + 0.205, 0.27, 0.53), "#ffd98a")
    shape("bg_casa", quad_front(u0 + 0.105, u0 + 0.12, 0.27, 0.53), "#3b2a28")
shape("bg_casa", quad_front(0.40, 0.52, 0.35, 1.0), "#4a3028")
# varanda: deck e pilares à frente da fachada
deck = [toward(K_bot, VL, 0.0), [FL_b[0] - 30, FL_b[1] + 22], [FL_b[0] - 30, FL_b[1] + 40], [K_bot[0] + 30, K_bot[1] + 30]]

# árvores atrás da casa e à esquerda
for (cx, cy, R, s) in ((1240, 420, 120, 1), (1820, 470, 110, 2), (250, 470, 150, 3)):
    blob = [[cx + R * (1 + 0.15 * math.sin(7 * t / 24 * math.tau)) * math.cos(t / 24 * math.tau),
             cy + R * 0.85 * (1 + 0.15 * math.sin(5 * t / 24 * math.tau)) * math.sin(t / 24 * math.tau)] for t in range(24)]
    shape("bg_morros" if cx < 1300 and cx > 1000 else "bg_morros", blob, "#3f5a3a", geometry="spline")
    for row in range(4):
        yy = cy - R * 0.55 + row * R * 0.32
        half = R * (0.75 - 0.12 * abs(row - 1.3))
        paint("bg_morros", "Folhagem", ("#4f7040", "#5d7f45", "#47663a", "#3a5634")[row], [[[cx - half + 36 * i, yy + 14 * (i % 2), 1.0] for i in range(int(2 * half / 36) + 1)]], seed=s * 10 + row, size=44)
    paint("bg_morros", "Folhagem", "#c9a95a", [[[cx - R * 0.2 + 30 * i, cy - R * 0.65 + 12 * (i % 2), 0.8] for i in range(int(R / 40))]], alpha=0.7, seed=s + 9, size=30)
shape("bg_morros", [[245, 560], [262, 560], [266, 650], [240, 650]], "#4a3328")
# o tronco da árvore da direita fica atrás da casa

# ---------- cerca ----------
def fence_y(x):
    return 660 + 14 * math.sin(x / 520)

shapes("bg_cerca", [[[x, fence_y(x) - 6], [x + 160, fence_y(x + 160) - 6], [x + 160, fence_y(x + 160) + 4], [x, fence_y(x) + 4]] for x in range(-60, 1960, 160)] +
       [[[x, fence_y(x) + 44], [x + 160, fence_y(x + 160) + 44], [x + 160, fence_y(x + 160) + 54], [x, fence_y(x) + 54]] for x in range(-60, 1960, 160)], "#b78456")
pick = []
for x in range(-20, 1960, 46):
    y = fence_y(x)
    pick.append([[x, y - 40], [x + 8, y - 52], [x + 16, y - 40], [x + 16, y + 74], [x, y + 74]])
shapes("bg_cerca", pick, "#d9a874")
shapes("bg_cerca", [[[p[0][0] + 11, p[0][1]], [p[2][0], p[2][1]], [p[3][0], p[3][1]], [p[0][0] + 11, p[3][1]]] for p in pick], "#9a6a48")

# ---------- roseiras (sem flores, exceto a última) ----------
bushes = [(110, 840, 170), (420, 870, 175), (760, 850, 170), (1200, 860, 185), (1600, 870, 175), (1900, 850, 160)]
for i, (cx, cy, R) in enumerate(bushes):  # sombra projetada (sol à esquerda)
    shape("bg_roseira", [[cx + 70 + R * 1.15 * math.cos(t / 24 * math.tau), cy + R * 0.42 + R * 0.22 * math.sin(t / 24 * math.tau)] for t in range(24)], "#33502a", alpha=0.55, geometry="spline")
for i, (cx, cy, R) in enumerate(bushes):
    blob = [[cx + R * (1 + 0.12 * math.sin(6 * t / 28 * math.tau + i)) * math.cos(t / 28 * math.tau),
             cy + R * 0.55 * (1 + 0.12 * math.sin(4 * t / 28 * math.tau + i)) * math.sin(t / 28 * math.tau)] for t in range(28)]
    shape("bg_roseira", blob, "#2f4a2c", geometry="spline")
    for k, (col, a, dy, sz) in enumerate((("#2a4226", 1.0, 0.55, 54), ("#3a5a32", 1.0, 0.3, 54), ("#476b3a", 1.0, 0.0, 52), ("#5c8040", 1.0, -0.2, 46), ("#6f8f4a", 0.9, -0.38, 40), ("#d8b46a", 0.55, -0.48, 26))):
        n = max(3, int(R / 45))
        pts = [[cx - R * 0.75 + (1.5 * R / (n - 1)) * j + rnd.uniform(-12, 12), cy + R * 0.55 * dy * 2 + rnd.uniform(-14, 14), 1.0] for j in range(n)]
        paint("bg_roseira", "Folhagem", col, [pts], alpha=a, seed=10 * i + k, size=sz)
    # rosas murchas / botões secos
    for j in range(3):
        x, y = cx + rnd.uniform(-R * 0.7, R * 0.7), cy + rnd.uniform(-R * 0.35, R * 0.1)
        shape("bg_roseira", [[x + 7 * math.cos(t / 8 * math.tau), y + 6 * math.sin(t / 8 * math.tau)] for t in range(8)], "#7a4a38", geometry="spline")
# sombra do chão sob as roseiras


# a última rosa (haste + folhas + flor)
RX, RY = 1200, 760
d.call("PAINT", {"cut": cut, "layer": L["bg_roseira"], "preset": "Redondo duro", "preset_overrides": {"size": 7}, "color": C("#355a2c"),
                 "strokes": [{"points": [[RX + 4, RY + 120, 1], [RX + 2, RY + 70, 1], [RX, RY + 18, 1]]}], "seed": 1})
shapes("bg_roseira", [[[RX + 2, RY + 70], [RX + 34, RY + 52], [RX + 50, RY + 60], [RX + 30, RY + 76]],
                      [[RX + 2, RY + 92], [RX - 30, RY + 78], [RX - 46, RY + 86], [RX - 24, RY + 100]]], "#4f7a3a", geometry="spline")
petal = lambda r, a0, n, sq: [[RX + r * math.cos(a0 + t / n * math.tau), RY + sq * r * math.sin(a0 + t / n * math.tau)] for t in range(n)]
shape("bg_roseira", petal(30, 0, 16, 0.85), "#9c1f2e", geometry="spline")
shape("bg_roseira", petal(22, 0.4, 14, 0.8), "#c8303e", geometry="spline")
shape("bg_roseira", petal(13, 1.0, 12, 0.8), "#e2525a", geometry="spline")
shape("bg_roseira", [[RX - 6, RY - 4], [RX + 4, RY - 10], [RX + 8, RY], [RX, RY + 4]], "#7a1422", geometry="spline")
shape("bg_roseira", petal(30, 0, 16, 0.85)[11:15] + [[RX + 10, RY - 14]], "#ffb08a", alpha=0.45, geometry="spline")

for f in (0, 107):
    print(d.call("RENDER", {"cut": cut, "frame": f, "out": f"{OUT}/c001_bg_q{f}.png"})["path"])
print(d.call("RENDER", {"cut": cut, "frame": 0, "camera": False, "out": f"{OUT}/c001_bg_full.png"})["path"])

# ---------- câmera e multiplano ----------
d.call("CAMERA", {"op": "clear_keys", "cut": cut})
d.call("CAMERA", {"op": "move", "cut": cut, "from": {"center": [740, 520], "zoom": 1.3}, "to": {"center": [1180, 600], "zoom": 1.3},
                  "start": 0, "end": 107, "ease": "ease_in_out"})
for k, dep in (("bg_ceu", 4.0), ("bg_morros", 1.2), ("bg_casa", 0.5), ("bg_cerca", 0.25), ("bg_roseira", 0.0)):
    # plano distante dá menos zoom: escala a camada (pivô no centro do canvas) para cobrir o quadro em todo o pan
    kp = 1 / (1 + dep)
    z = 1 + 0.3 * kp
    need = 1.0
    for cx, cy in ((740, 520), (1180, 600)):
        lx, ly = 740 + (cx - 740) * kp, 520 + (cy - 520) * kp
        need = max(need, (960 / z + abs(lx - 960)) / 960, (540 / z + abs(ly - 540)) / 540)
    d.call("LAYER_MOTION", {"op": "set", "layer": L[k], "depth": dep})
    d.call("LAYER_MOTION", {"op": "keys", "layer": L[k], "property": "scale", "keys": [{"frame": 0, "value": round(need * 1.005, 4), "ease": {"type": "linear"}}] if need > 1.0001 else []})
    print(k, "escala", round(need, 3))
for f in (0, 54, 107):
    print(_call("RENDER", {"cut": cut, "frame": f, "out": f"{OUT}/c001_bg_q{f}.png"})["path"])
