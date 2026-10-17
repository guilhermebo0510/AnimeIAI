"""Cenário 3 (C007): plano aberto em contra-plongée; a câmera inclina do jardim para o céu do entardecer (tilt), onde as
pétalas vão subir. Céu (SKY + sol + brilho), morros e silhueta do jardim (cerca, roseiras, casa) contra a luz.
Multiplano: céu em depth 2 (escala calculada para cobrir o quadro), chão em 0. Reexecutável."""
import math, random
from curta import d, ids, node, OUT, rgba

cut = ids["cuts"][6]
W, H = 1920, 1080
Z = 1.6
CAM_FROM, CAM_TO = (960, 740), (960, 340)   # meia altura visível = 540/1.6 = 337,5 → 402,5..1077,5 e 2,5..677,5
_call = d.call
d.call = lambda a, p=None, **kw: _call(a, {"frame": 0, **(p or {})} if a not in ("RENDER", "CAMERA", "LAYER_MOTION") else p, **kw)


def layer(key):
    lid = node(f"c7_{key}", key, {"type": "layer", "kind": "raster"}, cut)
    cel = node(f"c7_{key}_c1", "1", {"type": "cel", "number": 1}, lid)
    d.apply({"kind": "replace_data", "id": cel, "data": {"type": "cel", "number": 1}})
    return lid


LAYERS = ["bg_ceu", "bg_chao"]
L = {k: layer(k) for k in LAYERS}
for i, k in enumerate(LAYERS):
    d.apply({"kind": "move_node", "id": L[k], "parent": cut, "index": i + 1})


def C(h, a=1.0): return {"type": "direct", "rgba": rgba(h, a)}


def fill(lay, pts_list, color, alpha=1.0, geometry="polyline"):
    pts_list = [p for p in pts_list if max(q[0] for q in p) > 0 and min(q[0] for q in p) < W and max(q[1] for q in p) > 0 and min(q[1] for q in p) < H]
    if pts_list:
        d.call("FILL_SHAPE", {"cut": cut, "layer": L[lay], "shapes": [{"points": p, "geometry": geometry} for p in pts_list], "color": C(color, alpha)})


rnd = random.Random(21)
HY = 860
SUN = (1180, 250)
# ---------- céu ----------
d.call("SKY", {"cut": cut, "layer": L["bg_ceu"], "sky": {"time_of_day": "sunset", "horizon_y": HY, "clouds": 0.45, "cloud_scale": 300, "seed": 23}})
d.call("GRADIENT", {"cut": cut, "layer": L["bg_ceu"], "kind": "radial", "from": list(SUN), "to": [SUN[0], SUN[1] + 520],
                    "stops": [[0, "#fff4d0ff"], [0.1, "#ffe0a0dd"], [0.4, "#ffb06a55"], [1, "#ff905000"]], "blend": "screen"})
fill("bg_ceu", [[[SUN[0] + 58 * math.cos(t / 32 * math.tau), SUN[1] + 58 * math.sin(t / 32 * math.tau)] for t in range(32)]], "#fff8e4", geometry="spline")
# raios de luz suaves saindo do sol
for k in range(9):
    a = -math.pi / 2 + (k - 4) * 0.33 + rnd.uniform(-0.08, 0.08)
    w = rnd.uniform(0.03, 0.06)
    fill("bg_ceu", [[list(SUN), [SUN[0] + 1400 * math.cos(a - w), SUN[1] + 1400 * math.sin(a - w)], [SUN[0] + 1400 * math.cos(a + w), SUN[1] + 1400 * math.sin(a + w)]]], "#fff0c8", 0.10)
for k in range(9):
    a = math.pi / 2 + (k - 4) * 0.3 + rnd.uniform(-0.08, 0.08)
    w = rnd.uniform(0.03, 0.05)
    fill("bg_ceu", [[list(SUN), [SUN[0] + 1400 * math.cos(a - w), SUN[1] + 1400 * math.sin(a - w)], [SUN[0] + 1400 * math.cos(a + w), SUN[1] + 1400 * math.sin(a + w)]]], "#fff0c8", 0.07)


# ---------- chão: morros, jardim em contraluz ----------
def ridge(y0, amp, step, seed):
    r = random.Random(seed)
    return [[x, y0 + amp * math.sin(x / 290 + seed) + r.uniform(-amp, amp) * 0.3] for x in range(-40, W + 41, step)] + [[W + 40, H + 10], [-40, H + 10]]


fill("bg_chao", [ridge(HY - 50, 22, 60, 2)], "#9a6f86", geometry="spline")
fill("bg_chao", [ridge(HY - 10, 14, 48, 4)], "#6e5470", geometry="spline")
# casa em silhueta à direita (telhado de duas águas) e árvore
hx, hy = 1460, HY + 10
fill("bg_chao", [[[hx, hy], [hx, hy - 110], [hx + 150, hy - 200], [hx + 300, hy - 110], [hx + 300, hy]]], "#3c2c3e")
fill("bg_chao", [[[hx + 60, hy - 70], [hx + 100, hy - 70], [hx + 100, hy - 30], [hx + 60, hy - 30]],
                 [[hx + 200, hy - 70], [hx + 240, hy - 70], [hx + 240, hy - 30], [hx + 200, hy - 30]]], "#ffcf7a", 0.9)
for (cx, cy, R) in ((300, HY - 90, 110), (1830, HY - 60, 90)):
    fill("bg_chao", [[[cx + R * (1 + 0.14 * math.sin(7 * t / 30 * math.tau)) * math.cos(t / 30 * math.tau),
                       cy + R * 0.9 * (1 + 0.14 * math.sin(5 * t / 30 * math.tau)) * math.sin(t / 30 * math.tau)] for t in range(30)]], "#33283a", geometry="spline")
    fill("bg_chao", [[[cx - 9, cy + R * 0.7], [cx + 9, cy + R * 0.7], [cx + 11, HY + 30], [cx - 11, HY + 30]]], "#2c2230")
# gramado em primeiro plano
fill("bg_chao", [[[-40, HY + 20]] + [[x, HY + 20 + 6 * math.sin(x / 200)] for x in range(0, W + 1, 80)] + [[W + 40, HY + 20], [W + 40, H + 10], [-40, H + 10]]], "#3a3a2e")
d.call("GRADIENT", {"cut": cut, "layer": L["bg_chao"], "kind": "linear", "from": [0, 0], "to": [0, H],
                    "stops": [[0, "#00000000"], [(HY + 18) / H, "#00000000"], [(HY + 20) / H, "#5a5a36ff"], [1, "#2a2a22ff"]], "blend": "normal"})
# cerca em contraluz
pick = []
for x in range(-20, W + 20, 40):
    y = HY + 30
    pick.append([[x, y - 46], [x + 7, y - 56], [x + 14, y - 46], [x + 14, y + 30], [x, y + 30]])
fill("bg_chao", pick + [[[-40, HY + 2], [W + 40, HY + 2], [W + 40, HY + 10], [-40, HY + 10]], [[-40, HY + 40], [W + 40, HY + 40], [W + 40, HY + 48], [-40, HY + 48]]], "#2e2428")
# borda de luz do sol no topo das estacas
fill("bg_chao", [[[p[0][0], p[0][1]], [p[1][0], p[1][1]], [p[2][0], p[2][1]], [p[2][0], p[2][1] + 4], [p[0][0], p[0][1] + 4]] for p in pick], "#ffb070", 0.7)
# roseiras em primeiro plano (massas escuras com folhas), cortadas pela borda de baixo
for i, (cx, cy, R) in enumerate(((150, 1040, 230), (620, 1060, 200), (1100, 1050, 240), (1560, 1060, 210), (1920, 1040, 220))):
    fill("bg_chao", [[[cx + R * (1 + 0.12 * math.sin(6 * t / 28 * math.tau + i)) * math.cos(t / 28 * math.tau),
                       cy + R * 0.5 * (1 + 0.12 * math.sin(4 * t / 28 * math.tau + i)) * math.sin(t / 28 * math.tau)] for t in range(28)]], "#22301e", geometry="spline")
    pts = [[cx - R * 0.7 + j * R * 0.35 + rnd.uniform(-10, 10), cy - R * 0.32 + rnd.uniform(-10, 10), 1.0] for j in range(5)]
    d.call("PAINT", {"cut": cut, "layer": L["bg_chao"], "preset": "Folhagem", "preset_overrides": {"size": 44}, "color": C("#3c5232"), "strokes": [{"points": pts}], "seed": 30 + i})
    pts = [[cx - R * 0.5 + j * R * 0.3 + rnd.uniform(-10, 10), cy - R * 0.42 + rnd.uniform(-8, 8), 0.8] for j in range(4)]
    d.call("PAINT", {"cut": cut, "layer": L["bg_chao"], "preset": "Folhagem", "preset_overrides": {"size": 30}, "color": C("#c88a52", 0.6), "strokes": [{"points": pts}], "seed": 50 + i})

# ---------- câmera (tilt) e multiplano ----------
d.call("CAMERA", {"op": "clear_keys", "cut": cut})
d.call("CAMERA", {"op": "move", "cut": cut, "from": {"center": list(CAM_FROM), "zoom": Z}, "to": {"center": list(CAM_TO), "zoom": Z}, "start": 0, "end": 131, "ease": "ease_in_out"})
for k, dep in (("bg_ceu", 2.0), ("bg_chao", 0.0)):
    kp = 1 / (1 + dep)
    z = 1 + (Z - 1) * kp
    need = 1.0
    for cx, cy in (CAM_FROM, CAM_TO):
        lx, ly = CAM_FROM[0] + (cx - CAM_FROM[0]) * kp, CAM_FROM[1] + (cy - CAM_FROM[1]) * kp
        need = max(need, (W / 2 / z + abs(lx - W / 2)) / (W / 2), (H / 2 / z + abs(ly - H / 2)) / (H / 2))
    d.call("LAYER_MOTION", {"op": "set", "layer": L[k], "depth": dep})
    d.call("LAYER_MOTION", {"op": "keys", "layer": L[k], "property": "scale", "keys": [{"frame": 0, "value": round(need * 1.005, 4), "ease": "linear"}] if need > 1.0001 else []})
    print(k, "escala", round(need, 3))
for f in (0, 66, 131):
    print(_call("RENDER", {"cut": cut, "frame": f, "out": f"{OUT}/c007_bg_q{f}.png"})["path"])
print(_call("RENDER", {"cut": cut, "frame": 0, "camera": False, "out": f"{OUT}/c007_bg_full.png"})["path"])
