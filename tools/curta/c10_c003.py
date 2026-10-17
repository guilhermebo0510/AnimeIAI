"""C003 (CU contra-plongée): Hana, brava — 「それ、私が先に見つけたの！」. Fundo: céu do entardecer e topos das roseiras,
desfocados. Rosto do rig em 18 cels (olho × boca), timesheet do lip-sync da fala f3 + piscadas."""
import math, os, sys
from curta import d, ids, node, OUT, rgba
import corte

cut = ids["cuts"][2]
DUR = 72
START = 6


def C(h, a=1.0): return {"type": "direct", "rgba": rgba(h, a)}


# ---------- fundo ----------
bg = node("c3_bg", "bg", {"type": "layer", "kind": "raster"}, cut)
bgc = node("c3_bg_c1", "1", {"type": "cel", "number": 1}, bg)
d.apply({"kind": "replace_data", "id": bgc, "data": {"type": "cel", "number": 1}})
d.apply({"kind": "move_node", "id": bg, "parent": cut, "index": 1})
d.call("SKY", {"cut": cut, "frame": 0, "layer": bg, "sky": {"time_of_day": "sunset", "horizon_y": 1250, "clouds": 0.4, "cloud_scale": 320, "seed": 31}})
d.call("GRADIENT", {"cut": cut, "frame": 0, "layer": bg, "kind": "radial", "from": [300, 900], "to": [300, 100],
                    "stops": [[0, "#ffe2a8cc"], [0.5, "#ffb06a44"], [1, "#ff905000"]], "blend": "screen"})
for i, (cx, cy, R) in enumerate(((180, 1100, 330), (1750, 1120, 360))):
    d.call("FILL_SHAPE", {"cut": cut, "frame": 0, "layer": bg, "color": C("#3c5a34"), "shapes": [{"geometry": "spline", "points": [
        [cx + R * (1 + 0.12 * math.sin(6 * t / 28 * math.tau + i)) * math.cos(t / 28 * math.tau), cy + R * 0.6 * (1 + 0.12 * math.sin(4 * t / 28 * math.tau)) * math.sin(t / 28 * math.tau)] for t in range(28)]}]})
    d.call("PAINT", {"cut": cut, "frame": 0, "layer": bg, "preset": "Folhagem", "preset_overrides": {"size": 90}, "color": C("#6f8f4a"),
                     "strokes": [{"points": [[cx - R * 0.8 + j * R * 0.4, cy - R * 0.45 + 20 * (j % 2), 1.0] for j in range(5)]}], "seed": 70 + i})
d.call("EFFECTS", {"op": "set", "layer": bg, "effects": [{"effect": {"type": "gaussian_blur", "radius": 14}, "enabled": True}]})

# ---------- Hana ----------
combos = [(e, m) for e in range(3) for m in corte.MOUTHS]
L = corte.face_cut(cut, "c3_hana", "hana", (960, 120, 640, -6.0, 0.0), "angry", combos, base_index=2, bottom=(1078 - 120) / 640, draw=os.environ.get("SKIP_DRAW") != "1")

# ---------- fala + lip-sync + piscadas ----------
pl = corte.place_dialogue(cut, "tts_f3", START, "char_hana")
cells, r = corte.lipsync_cells(cut, pl, L["linha"], DUR, blinks=(2, 66))
print("lip-sync:", r.get("used_moras"), "primeiro", r.get("first_frame"), "quadros", r.get("frames"))
print("exposições:", cells)
for k in corte.LAYER_ORDER:
    corte.set_track(cut, L[k], k, "drawing", cells)
for f in (0, 3, 12, 30, 50, 67):
    print(d.call("RENDER", {"cut": cut, "frame": f, "out": f"{OUT}/c003_q{f}.png"})["path"])
