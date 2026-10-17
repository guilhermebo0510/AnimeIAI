"""C005 (MS): choque cômico — 「…ばあちゃん？」. Os dois de busto, surpresos, sobre fundo de gag (faixas radiais) com linhas
de foco fervendo (semente por quadro em 2s) e tremor de câmera no impacto. Hana fala (lip-sync f5); Ren segura a boca."""
import os
from curta import d, ids, node, OUT
import corte

cut = ids["cuts"][4]
DUR = 60
START = 10
DRAW = os.environ.get("SKIP_DRAW") != "1"


def layer(key, kind, index):
    lid = node(f"c5_{key}", key, {"type": "layer", "kind": kind}, cut)
    cel = node(f"c5_{key}_c1", "1", {"type": "cel", "number": 1}, lid)
    d.apply({"kind": "replace_data", "id": cel, "data": {"type": "cel", "number": 1}})
    d.apply({"kind": "move_node", "id": lid, "parent": cut, "index": index})
    return lid


# ---------- fundo de gag + linhas de foco (fervendo: 3 desenhos alternados em 2s) ----------
bg = layer("bg_gag", "raster", 1)
d.call("GAG_BG", {"cut": cut, "frame": 0, "layer": bg, "pattern": {"type": "radial_stripes", "center": [960, 470], "count": 28, "colors": ["#ffe9a8", "#ffc76a"]}})
fl = layer("linhas_foco", "vector", 2)
fl_cels = {1: ids["c5_linhas_foco_c1"]}
for n in (2, 3):
    fl_cels[n] = node(f"c5_linhas_foco_c{n}", str(n), {"type": "cel", "number": n}, fl)
    d.apply({"kind": "replace_data", "id": fl_cels[n], "data": {"type": "cel", "number": n}})
corte.set_track(cut, fl, "linhas", "effect", {0: 1, 1: 2, 2: 3})
for i in range(3):
    d.call("LINES", {"cut": cut, "frame": i, "layer": fl, "kind": {"type": "focus", "center": [960, 470], "clear": [780, 400]}, "count": 110,
                     "length": [220, 520], "width": [3, 12], "color": {"type": "direct", "rgba": [0.35, 0.18, 0.1, 0.85]}, "seed": 11 + i})
corte.set_track(cut, fl, "linhas", "effect", {f: 1 + (f // 2) % 3 for f in range(0, DUR, 2)})

# ---------- personagens ----------
H, TOP = 360, 190
BOT = (1078 - TOP) / H
Lh = corte.face_cut(cut, "c5_hana", "hana", (560, TOP, H, 10.0, 0.0), "surprised", [(e, m) for e in range(3) for m in corte.MOUTHS], base_index=3, bottom=BOT, draw=DRAW)
Lr = corte.face_cut(cut, "c5_ren", "ren", (1370, TOP - 30, H * 1.02, -10.0, 0.0), "surprised", [(e, "u") for e in range(3)], base_index=8, bottom=(1078 - TOP + 30) / (H * 1.02), draw=DRAW)

pl = corte.place_dialogue(cut, "tts_f5", START, "char_hana")
cells_h, r = corte.lipsync_cells(cut, pl, Lh["linha"], DUR, blinks=(44,))
for k in corte.LAYER_ORDER:
    corte.set_track(cut, Lh[k], k, "drawing", cells_h)
cells_r = {0: 4, 40: 14, 41: 24, 42: 14, 43: 4}  # Ren pisca um pouco antes dela
for k in corte.LAYER_ORDER:
    corte.set_track(cut, Lr[k], k, "drawing", cells_r)
print("Hana:", cells_h)

# ---------- tremor de impacto ----------
d.call("CAMERA", {"op": "clear_keys", "cut": cut})
d.call("CAMERA", {"op": "set", "cut": cut, "zoom": 1.06})
d.call("CAMERA", {"op": "shake", "cut": cut, "amplitude": 14, "start": 0, "end": 14, "decay": True, "seed": 5})
for f in (0, 5, 14, 30, 42):
    print(d.call("RENDER", {"cut": cut, "frame": f, "out": f"{OUT}/c005_q{f}.png"})["path"])
