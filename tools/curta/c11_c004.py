"""C004 (CU na altura dos olhos): Ren, firme — 「僕だって、ばあちゃんに渡したいんだ。」. Fundo: cerca e roseiras do jardim
contra o céu, desfocados. Rosto do rig em 18 cels (olho × boca), timesheet do lip-sync da fala f4 + piscadas."""
import math, os, sys
from curta import d, ids, node, OUT, rgba
import corte

cut = ids["cuts"][3]
DUR = 84
START = 8


def C(h, a=1.0): return {"type": "direct", "rgba": rgba(h, a)}


# ---------- fundo ----------
bg = node("c4_bg", "bg", {"type": "layer", "kind": "raster"}, cut)
bgc = node("c4_bg_c1", "1", {"type": "cel", "number": 1}, bg)
d.apply({"kind": "replace_data", "id": bgc, "data": {"type": "cel", "number": 1}})
d.apply({"kind": "move_node", "id": bg, "parent": cut, "index": 1})
d.call("SKY", {"cut": cut, "frame": 0, "layer": bg, "sky": {"time_of_day": "sunset", "horizon_y": 760, "clouds": 0.35, "cloud_scale": 300, "seed": 41}})
d.call("GRADIENT", {"cut": cut, "frame": 0, "layer": bg, "kind": "radial", "from": [1650, 700], "to": [1650, 150],
                    "stops": [[0, "#ffe2a8cc"], [0.5, "#ffb06a44"], [1, "#ff905000"]], "blend": "screen"})
d.call("FILL_SHAPE", {"cut": cut, "frame": 0, "layer": bg, "color": C("#8f8a52"), "shapes": [{"geometry": "polyline", "points": [[0, 760], [1920, 760], [1920, 1080], [0, 1080]]}]})
pick = [[[x, 700], [x + 18, 680], [x + 36, 700], [x + 36, 1000], [x, 1000]] for x in range(-20, 1940, 110)]
d.call("FILL_SHAPE", {"cut": cut, "frame": 0, "layer": bg, "color": C("#d9a874"), "shapes": [{"geometry": "polyline", "points": p} for p in pick] +
                      [{"geometry": "polyline", "points": [[0, 740], [1920, 740], [1920, 768], [0, 768]]}, {"geometry": "polyline", "points": [[0, 880], [1920, 880], [1920, 908], [0, 908]]}]})
for i, (cx, cy, R) in enumerate(((120, 1060, 360), (980, 1100, 420), (1820, 1060, 380))):
    d.call("FILL_SHAPE", {"cut": cut, "frame": 0, "layer": bg, "color": C("#3c5a34"), "shapes": [{"geometry": "spline", "points": [
        [cx + R * (1 + 0.12 * math.sin(6 * t / 28 * math.tau + i)) * math.cos(t / 28 * math.tau), cy + R * 0.5 * (1 + 0.12 * math.sin(4 * t / 28 * math.tau)) * math.sin(t / 28 * math.tau)] for t in range(28)]}]})
    d.call("PAINT", {"cut": cut, "frame": 0, "layer": bg, "preset": "Folhagem", "preset_overrides": {"size": 90}, "color": C("#6f8f4a"),
                     "strokes": [{"points": [[cx - R * 0.8 + j * R * 0.4, cy - R * 0.38 + 20 * (j % 2), 1.0] for j in range(5)]}], "seed": 80 + i})
d.call("EFFECTS", {"op": "set", "layer": bg, "effects": [{"effect": {"type": "gaussian_blur", "radius": 16}, "enabled": True}]})

# ---------- Ren ----------
combos = [(e, m) for e in range(3) for m in corte.MOUTHS]
L = corte.face_cut(cut, "c4_ren", "ren", (960, 150, 600, 8.0, 0.0), "angry", combos, base_index=2, bottom=(1078 - 150) / 600, draw=os.environ.get("SKIP_DRAW") != "1")

# ---------- fala + lip-sync + piscadas ----------
pl = corte.place_dialogue(cut, "tts_f4", START, "char_ren")
cells, r = corte.lipsync_cells(cut, pl, L["linha"], DUR, blinks=(3, 76))
print("lip-sync:", r.get("used_moras"), "primeiro", r.get("first_frame"), "quadros", r.get("frames"))
print("exposições:", cells)
for k in corte.LAYER_ORDER:
    corte.set_track(cut, L[k], k, "drawing", cells)
for f in (0, 4, 14, 34, 56, 77):
    print(d.call("RENDER", {"cut": cut, "frame": f, "out": f"{OUT}/c004_q{f}.png"})["path"])
