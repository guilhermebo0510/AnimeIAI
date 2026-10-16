"""Corte D, quadro 0: anéis grossos do rosto (olhos e boca) como formas com furo na camada raster `linha_massa`:
borda externa (pele) e interna (branco do olho / boca) medidas com MEASURE_EDGE a partir do contorno estimado."""
import json, math, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import d, ids, node, REF0

LINE = {"type": "direct", "rgba": [0x5c / 255, 0x2e / 255, 0x2e / 255, 1.0]}

MOUTH_OUT = [(988, 700), (1030, 688), (1080, 682), (1125, 680), (1138, 690), (1145, 715), (1140, 730), (1100, 738), (1050, 745), (1000, 752),
             (988, 740), (985, 720)]
MOUTH_OUT_SCAN = [(1014, 695), (1038, 691), (1071, 687), (1107, 683), (1121, 683), (1131, 687), (1136, 695), (1139, 704), (1141, 712),
                  (1142.5, 720), (1141.5, 728), (1136.5, 732), (1122.5, 736), (1102.5, 740), (1074.5, 744), (1047.5, 748), (1012, 752),
                  (1004, 752.5), (989, 748), (987, 740), (988, 736), (990, 728), (993, 720), (996, 712), (1001, 704), (1004, 700)]
MOUTH_IN_SCAN = [(1124, 695.5), (1100, 695.5), (1068, 699.5), (1034, 703.5), (1014, 707.5), (1010, 711.5), (1007, 715.5), (1006, 719.5),
                 (1004, 723.5), (1003, 727.5), (1002, 731.5), (1001, 735.5), (1042, 736.5), (1078, 732.5), (1104, 728.5), (1127, 724.5),
                 (1131, 720), (1131, 716), (1130, 712), (1129, 708), (1127, 704), (1126, 700)]
MOUTH_IN = [(1003, 707), (1040, 698), (1090, 693), (1122, 692), (1130, 705), (1128, 720), (1095, 727), (1050, 733), (1008, 738), (1002, 725)]


def run():
    massa = node("linha_massa", "Linha-massa", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("linha_massa_c1", "1", {"type": "cel", "number": 1}, massa)
    d.apply({"kind": "replace_data", "id": ids["linha_massa_c1"], "data": {"type": "cel", "number": 1}})
    out = []
    for cx, cy, ro, ri in [(772, 446, 117, 78), (1288, 389, 112, 77)]:
        outer = meas.measure_edge(d, REF0, meas.circle(cx, cy, ro, n=144))
        inner = meas.measure_edge(d, REF0, meas.circle(cx, cy, ri, n=120))
        mid = [[cx + (ro + ri) / 2 * math.cos(a), cy + (ro + ri) / 2 * math.sin(a)] for a in [k * math.pi / 18 for k in range(36)]]
        out.append(ring(massa, outer, inner, mid))
    # a boca muda por desenho: fica em d218_dyn.py (camada `boca`)
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
