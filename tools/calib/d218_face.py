"""Corte D, quadro 0: olhos (anéis grossos = forma com furo, bordas medidas), boca e queixo (linhas medidas)."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import d, ids, node, REF0, draw, render

LINE = {"type": "direct", "rgba": [0x5c / 255, 0x2e / 255, 0x2e / 255, 1.0]}

# camada raster para linhas-massa (anéis dos olhos etc.), também fronteira do balde
massa = node("linha_massa", "Linha-massa", {"type": "layer", "kind": "raster"}, ids["cut"])
node("linha_massa_c1", "1", {"type": "cel", "number": 1}, massa)
d.apply({"kind": "set_props", "id": ids["cor"], "data": {"line_refs": [ids["linha"], massa]}})
# recomeça os cels de linha deste quadro
d.apply({"kind": "replace_data", "id": ids["linha_c1"], "data": {"type": "cel", "number": 1}})
d.apply({"kind": "replace_data", "id": ids["linha_massa_c1"], "data": {"type": "cel", "number": 1}})

log = {}
shapes = []
for name, (cx, cy, ro, ri) in {"olho_esq": (772, 446, 117, 78), "olho_dir": (1288, 389, 112, 77)}.items():
    outer = meas.measure_edge(d, REF0, meas.circle(cx, cy, ro, n=72))
    inner = meas.measure_edge(d, REF0, meas.circle(cx, cy, ri, n=60))
    shapes.append({"points": outer, "geometry": "polyline"})
    shapes.append({"points": list(reversed(inner)), "geometry": "polyline"})
    log[name] = "ok"
print(d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": 0, "layer": massa, "shapes": shapes, "color": LINE}).get("description"))

curves = []
mouth = [(992, 703), (1030, 694), (1080, 689), (1130, 688), (1140, 700), (1142, 722), (1110, 733), (1060, 740), (1000, 748), (990, 726)]
pts, ws = meas.measure_line(d, REF0, meas.densify(mouth, 8, closed=True), closed=True)
curves.append(meas.curve_from_measure(pts, ws, LINE, closed=True))
log["boca"] = curves[-1]["width"]
chin = [(560, 600), (575, 680), (610, 750), (665, 820), (740, 885), (840, 935), (960, 962), (1080, 955), (1190, 925), (1280, 870), (1345, 800), (1390, 720)]
pts, ws = meas.measure_line(d, REF0, meas.densify(chin, 8))
curves.append(meas.curve_from_measure(pts, ws, LINE))
log["queixo"] = curves[-1]["width"]
print(json.dumps(log))
print(draw(curves).get("description"))
print(render("q0_rosto"))
