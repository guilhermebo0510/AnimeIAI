"""Corte D, quadro 0: realces salmão como TRAÇOS medidos (substitui a versão de polígonos, que errava a faixa em 3–5 px). O agente estima a
linha central de cada faixa; MEASURE_LINE pela cor do realce (busca larga: as faixas têm até ~20 px) dá centro e largura;
cor de cada faixa = mediana medida nos centros. Camada vetorial `rim`, recortada na cor do cabelo."""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node, REF0, ck, CEL, F0, W, candidates, color_score

CENTERS = {
    "esq1": [(300, 150), (285, 230), (262, 300), (247, 345)],
    "esq2": [(222, 345), (225, 385), (250, 440)],
    "esq3": [(262, 465), (240, 540), (225, 595), (250, 635)],
    "esq4": [(252, 668), (230, 740), (215, 795), (228, 828)],
    "esq5": [(242, 864), (238, 888), (233, 912), (229, 936), (228, 956)],
    "esq5b": [(228, 960), (226, 976), (223, 992), (220, 1008), (220, 1016), (219, 1032), (219, 1040), (220, 1048), (225, 1056)],
    "mecha_e": [(430, 0), (412, 60), (398, 150), (390, 250), (390, 400), (397, 520), (399, 640), (397, 680), (395, 706)],
    "mecha_e_topo": [(370, 0), (352, 50), (345, 75)],
    "dir1": [(1560, 0), (1585, 100), (1600, 200), (1610, 320), (1607, 420), (1600, 515)],
    "dir2": [(1625, 135), (1660, 150), (1700, 180), (1720, 212), (1712, 250), (1698, 298)],
    "dir3": [(1712, 308), (1752, 360), (1785, 392), (1765, 440), (1740, 490)],
    "dir4": [(1752, 503), (1800, 555), (1838, 582), (1828, 625), (1815, 648)],
    "dir5": [(1820, 670), (1860, 692), (1882, 708)],
    "dir6": [(1913, 722), (1914, 815)],
}
RIM = "#f4a097"
TAPER_END = set()
NO_TRIM = set()
# caudas finas (2–3 px) que a medição de faixa não acompanha: traço fixo lido por varredura (runs.py)
TAILS = {"mecha_e_cauda": ([(394, 706), (392, 718), (390, 730), (388, 742), (386, 754), (384, 766)], 2.5, "#e47a74"),
         "dir3_cauda": ([(1745, 456), (1742, 464), (1738, 472), (1735, 480), (1732, 488)], 4.0, "#e98a82"),
         "mecha_e_meio": ([(398, 660), (397.5, 672), (396.5, 684), (395.5, 696), (394, 708)], 8.0, "#f18682")}  # pontas finas e mais escuras: a cor da faixa muda e o corte comeria a ponta real
# redesenhos: quantos pontos de cada cauda existem (cor lida nos pontos levados pelo campo); 0 = não existe
TAIL_KEEP = {7: {"mecha_e_cauda": 3, "dir3_cauda": 0}, 8: {"mecha_e_cauda": 0, "dir3_cauda": 0, "mecha_e_meio": 0}}
# faixas relidas no próprio desenho (centros das varreduras por linha, runs.py)
CENTERS_DES = {8: {"dir2": [(1615, 62), (1622, 70), (1627, 80), (1634, 90), (1642, 100), (1650, 110), (1657, 120), (1664, 130), (1672, 140), (1678, 150),
                            (1685, 160), (1692, 170), (1699, 180), (1706, 190), (1712, 200), (1715, 210), (1714, 225), (1712, 240), (1709, 255),
                            (1708, 265)]}}
# caudas relidas por varredura no próprio desenho (runs.py): pontos já no desenho
TAILS_DES = {7: {"mecha_e_cauda": ([(389, 690), (388, 705), (387, 718), (386, 730), (385, 740), (383, 750)], 3.5, "#e47a74")}}
AS_SHAPE = "--formas" in sys.argv  # polígonos pelas bordas da faixa pioraram o F1 (0,904 × 0,9075)


def run():
    lay = node("rim", "Realce de borda", {"type": "layer", "kind": "vector"}, ids["cut"])
    ck("rim")
    d.apply({"kind": "set_props", "id": lay, "data": {"clip_to_below": True, "visible": True}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids[ck("rim")], "data": {"type": "cel", "number": CEL}})
    curves, shapes, log = [], [], {}
    raw = CENTERS_DES.get(CEL, {})
    for name, poly in {**CENTERS, **raw}.items():
        # paquímetro de faixa: a cor do realce varia por dentro (borda salmão, miolo mais claro)
        best = None
        for cand in ([[tuple(p) for p in poly]] if name in raw else candidates(poly)):  # redesenho: do desenho 1 ou do campo, a que cai mais no realce
            cp, cw = meas.measure_band(d, REF0, meas.densify(cand, 3), RIM, 0.09, smooth=1)
            sc = color_score(cp, RIM, 60) if CEL != 1 else 0
            if best is None or sc > best[0]:
                best = (sc, cp, cw)
        _, pts, ws = best
        # cor própria da faixa (varia de faixa para faixa) e nova medição com ela
        own = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": [[round(p[0]), round(p[1])] for p in pts[2:-2:4]],
                                                     "radius": 1}))
        cs0 = sorted(own, key=lambda c: sum(c["rgb"]))
        if len(cs0) >= 3:
            hexc = cs0[len(cs0) // 2]["hex"]
            pts, ws = meas.measure_band(d, REF0, pts, hexc, 0.08, passes=(16.0, 14.0), smooth=1, trim=name not in NO_TRIM)
        probes = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": [[round(p[0]), round(p[1])] for p in pts[1:-1:2]],
                                                        "radius": 1}))
        cs = sorted(probes, key=lambda c: sum(c["rgb"]))
        mid = cs[len(cs) // 2]["rgb"] if cs else [0xf4 / 255, 0xa0 / 255, 0x97 / 255]
        c = meas.curve_from_measure(pts, ws, {"type": "direct", "rgba": list(mid) + [1.0]}, tip="marker", every=1)
        c["profile"] = [[t, max(0.1, min(1.8, w))] for t, w in c["profile"]]
        if name in TAPER_END:  # faixa que termina em ponta dentro do cabelo
            n = len(c["profile"])
            k = max(2, n // 6)
            for i in range(n - k, n):
                c["profile"][i][1] = max(0.08, c["profile"][i][1] * (n - i - 0.5) / k)
        if AS_SHAPE:
            # polígono pelas bordas medidas da faixa (pontas retas/agudas como na referência, sem tampa redonda)
            ns = meas._normals(pts)
            wl = [w or c["width"] for w in ws]
            left = [[p[0] + n[0] * w / 2, p[1] + n[1] * w / 2] for p, n, w in zip(pts, ns, wl)]
            right = [[p[0] - n[0] * w / 2, p[1] - n[1] * w / 2] for p, n, w in zip(pts, ns, wl)]
            shapes.append(({"points": left + right[::-1], "geometry": "polyline"}, list(mid)))
        else:
            curves.append(c)
        log[name] = (round(c["width"], 1), "#%02x%02x%02x" % tuple(round(v * 255) for v in mid))
    for name, (poly, w, hexc) in {**TAILS, **TAILS_DES.get(CEL, {})}.items():
        if name in TAILS_DES.get(CEL, {}):  # relida no desenho (sem campo)
            col = [int(hexc[i:i + 2], 16) / 255 for i in (1, 3, 5)]
            curves.append({"points": [list(p) for p in poly], "width": w, "tip": "marker", "geometry": "spline",
                           "color": {"type": "direct", "rgba": col + [1.0]}, "profile": [[0, 1.2], [1, 0.4]]})
            continue
        keep = TAIL_KEEP.get(CEL, {}).get(name)
        if keep is not None:
            if keep < 2:
                continue
            poly = poly[:keep]
        col = [int(hexc[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        curves.append({"points": [list(p) for p in W(poly)], "width": w, "tip": "marker", "geometry": "spline",
                       "color": {"type": "direct", "rgba": col + [1.0]}, "profile": [[0, 1.2], [1, 0.4]]})
    if curves:
        d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": F0, "layer": lay, "curves": curves})
    if shapes:
        rl = node("rim_r", "Realce de borda (formas)", {"type": "layer", "kind": "raster"}, ids["cut"])
        ck("rim_r")
        d.apply({"kind": "set_props", "id": rl, "data": {"clip_to_below": True}})
        reorder()
        d.apply({"kind": "replace_data", "id": ids[ck("rim_r")], "data": {"type": "cel", "number": CEL}})
        for sh, col in shapes:
            d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": F0, "layer": rl, "shapes": [sh], "color": {"type": "direct", "rgba": col + [1.0]}})
    return log


if __name__ == "__main__":
    for k, v in run().items():
        print(k, v)
