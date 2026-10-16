"""Corte D, quadro 0: realces salmão como TRAÇOS medidos (substitui a versão de polígonos, que errava a faixa em 3–5 px). O agente estima a
linha central de cada faixa; MEASURE_LINE pela cor do realce (busca larga: as faixas têm até ~20 px) dá centro e largura;
cor de cada faixa = mediana medida nos centros. Camada vetorial `rim`, recortada na cor do cabelo."""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node, REF0

CENTERS = {
    "esq1": [(300, 150), (285, 230), (262, 300), (247, 345)],
    "esq2": [(222, 345), (225, 385), (250, 440)],
    "esq3": [(262, 465), (240, 540), (225, 595), (250, 635)],
    "esq4": [(252, 668), (230, 740), (215, 795), (228, 828)],
    "esq5": [(245, 860), (235, 940), (225, 1010), (220, 1056)],
    "mecha_e": [(430, 0), (412, 60), (398, 150), (390, 250), (390, 400), (397, 520), (399, 640), (392, 720), (385, 770)],
    "mecha_e_topo": [(370, 0), (352, 50), (345, 75)],
    "dir1": [(1560, 0), (1585, 100), (1600, 200), (1610, 320), (1607, 420), (1600, 515)],
    "dir2": [(1625, 135), (1660, 150), (1700, 180), (1720, 212), (1712, 250), (1698, 298)],
    "dir3": [(1712, 308), (1752, 360), (1785, 392), (1765, 440), (1740, 490)],
    "dir4": [(1752, 503), (1800, 555), (1838, 582), (1828, 625), (1815, 648)],
    "dir5": [(1820, 670), (1860, 692), (1882, 708)],
    "dir6": [(1913, 722), (1914, 815)],
}
RIM = "#f4a097"


def run():
    lay = node("rim", "Realce de borda", {"type": "layer", "kind": "vector"}, ids["cut"])
    node("rim_c1", "1", {"type": "cel", "number": 1}, lay)
    d.apply({"kind": "set_props", "id": lay, "data": {"clip_to_below": True, "visible": True}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids["rim_c1"], "data": {"type": "cel", "number": 1}})
    curves, log = [], {}
    for name, poly in CENTERS.items():
        pts, ws = meas.measure_line(d, REF0, meas.densify(poly, 6), line_color=RIM, passes=((20.0, 0.08), (12.0, 0.08)), smooth=2)
        probes = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": [[round(p[0]), round(p[1])] for p in pts[1:-1:2]],
                                                        "radius": 1}))
        cs = sorted(probes, key=lambda c: sum(c["rgb"]))
        mid = cs[len(cs) // 2]["rgb"] if cs else [0xf4 / 255, 0xa0 / 255, 0x97 / 255]
        c = meas.curve_from_measure(pts, ws, {"type": "direct", "rgba": list(mid) + [1.0]}, tip="marker")
        c["profile"] = [[t, max(0.1, min(1.8, w))] for t, w in c["profile"]]
        curves.append(c)
        log[name] = (round(c["width"], 1), "#%02x%02x%02x" % tuple(round(v * 255) for v in mid))
    d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": 0, "layer": lay, "curves": curves})
    return log


if __name__ == "__main__":
    for k, v in run().items():
        print(k, v)
