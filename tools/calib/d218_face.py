"""Corte D, quadro 0: anéis grossos do rosto (olhos e boca) como formas com furo na camada raster `linha_massa`:
borda externa (pele) e interna (branco do olho / boca) medidas com MEASURE_EDGE a partir do contorno estimado."""
import json, math, os, sys
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
    out = []
    for cx, cy, ro, ri in [(772, 446, 117, 78), (1288, 389, 112, 77)]:
        outer = meas.measure_edge(d, REF0, meas.circle(cx, cy, ro, n=72))
        inner = meas.measure_edge(d, REF0, meas.circle(cx, cy, ri, n=60))
        mid = [[cx + (ro + ri) / 2 * math.cos(a), cy + (ro + ri) / 2 * math.sin(a)] for a in [k * math.pi / 18 for k in range(36)]]
        out.append(ring(massa, outer, inner, mid))
    outer = meas.measure_edge(d, REF0, meas.densify(MOUTH_OUT, 5, closed=True), passes=((5.0, 0.03), (2.5, 0.03)))
    inner = meas.measure_edge(d, REF0, meas.densify(MOUTH_IN, 5, closed=True), passes=((5.0, 0.03), (2.5, 0.03)))
    mid = [[(a[0] + b[0]) / 2, (a[1] + b[1]) / 2] for a, b in zip(meas.densify(MOUTH_OUT, 20, closed=True), meas.densify(MOUTH_IN, 20, closed=True))]
    out.append(ring(massa, outer, inner, mid))
    return out


def ring(layer, outer, inner, mid):
    """Anel com a cor da tinta medida no meio dele (mediana)."""
    pr = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": [[round(p[0]), round(p[1])] for p in mid], "radius": 1}))
    rgb = sorted([p["rgb"] for p in pr], key=sum)
    col = {"type": "direct", "rgba": list(rgb[len(rgb) // 2]) + [1.0]}
    shapes = [{"points": outer, "geometry": "polyline"}, {"points": list(reversed(inner)), "geometry": "polyline"}]
    return d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": 0, "layer": layer, "shapes": shapes, "color": col}).get("description")


if __name__ == "__main__":
    print(run())
