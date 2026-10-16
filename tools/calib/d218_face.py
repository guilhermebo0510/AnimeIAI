"""Corte D, quadro 0: anéis grossos do rosto (olhos e boca) como formas com furo na camada raster `linha_massa`:
borda externa (pele) e interna (branco do olho / boca) medidas com MEASURE_EDGE a partir do contorno estimado."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import d, ids, node, REF0

LINE = {"type": "direct", "rgba": [0x5c / 255, 0x2e / 255, 0x2e / 255, 1.0]}

MOUTH_OUT = [(988, 700), (1030, 688), (1080, 682), (1125, 680), (1138, 690), (1145, 715), (1140, 730), (1100, 738), (1050, 745), (1000, 752),
             (988, 740), (985, 720)]
MOUTH_IN = [(1003, 707), (1040, 698), (1090, 693), (1122, 692), (1130, 705), (1128, 720), (1095, 727), (1050, 733), (1008, 738), (1002, 725)]


def run():
    massa = node("linha_massa", "Linha-massa", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("linha_massa_c1", "1", {"type": "cel", "number": 1}, massa)
    d.apply({"kind": "replace_data", "id": ids["linha_massa_c1"], "data": {"type": "cel", "number": 1}})
    shapes = []
    for cx, cy, ro, ri in [(772, 446, 117, 78), (1288, 389, 112, 77)]:
        outer = meas.measure_edge(d, REF0, meas.circle(cx, cy, ro, n=72))
        inner = meas.measure_edge(d, REF0, meas.circle(cx, cy, ri, n=60))
        shapes += [{"points": outer, "geometry": "polyline"}, {"points": list(reversed(inner)), "geometry": "polyline"}]
    outer = meas.measure_edge(d, REF0, meas.densify(MOUTH_OUT, 5, closed=True), passes=((5.0, 0.03), (2.5, 0.03)))
    inner = meas.measure_edge(d, REF0, meas.densify(MOUTH_IN, 5, closed=True), passes=((5.0, 0.03), (2.5, 0.03)))
    shapes += [{"points": outer, "geometry": "polyline"}, {"points": list(reversed(inner)), "geometry": "polyline"}]
    return d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": 0, "layer": massa, "shapes": shapes, "color": LINE})


if __name__ == "__main__":
    print(run().get("description"))
