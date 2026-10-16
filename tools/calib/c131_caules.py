"""Corte C: caules roxos com espinhos (sem contorno preto): traço medido pelo paquímetro de faixa na cor do caule (centro e
largura), desenhado em duas passadas — borda roxo-escura um pouco mais larga e o corpo por cima. Camada vetorial `caules`
(abaixo da cor da moldura). uso: python c131_caules.py"""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from c131 import d, ids, F, REF, node, reorder

BODY = "#5c2850"
MODE = os.environ.get("C131_CAULE_MODO", "faixa")
TOL = float(os.environ.get("C131_CAULE_TOL", "0.14"))
SCALE = float(os.environ.get("C131_CAULE", "1.0"))
EDGE_EXTRA = float(os.environ.get("C131_CAULE_BORDA", "4.0"))
EDGE = [0x3a / 255, 0x16 / 255, 0x25 / 255, 1.0]
# centro estimado pelo agente nos recortes (grid_crop / mapa roxo), largura nominal
STEMS = {
    "s1": ([(522, 136), (540, 144), (560, 162), (580, 167), (600, 174), (620, 182), (640, 191), (660, 196), (680, 200), (700, 203), (720, 198),
            (740, 194), (760, 188), (780, 179), (800, 163), (809, 156), (818, 144), (826, 132), (832, 120), (833, 108), (832, 96), (836, 84),
            (836, 72), (832, 60), (829, 48), (824, 36), (815, 24), (802, 12), (797, 8)], 22),  # centros das varreduras (runs.py)
    "s2": ([(1000, 50), (970, 65), (950, 88), (935, 108), (920, 120), (915, 145), (920, 175), (935, 205), (955, 225), (970, 240)], 14),
    "s3": ([(1318, 118), (1330, 140), (1345, 165), (1365, 185), (1390, 200), (1420, 212), (1450, 213), (1480, 208), (1510, 202), (1545, 196)], 14),
    "s4": ([(165, 458), (185, 475), (200, 495), (212, 520), (222, 550), (230, 580), (235, 610), (236, 640), (232, 670), (225, 695)], 10),
    "s5a": ([(1758, 488), (1775, 510), (1792, 535), (1808, 555), (1825, 575), (1840, 592), (1855, 610), (1868, 635), (1880, 660), (1895, 685),
             (1910, 705)], 12),
    "s5b": ([(1840, 592), (1865, 588), (1890, 585), (1920, 583)], 12),
    "s5c": ([(1815, 600), (1800, 620), (1785, 645), (1770, 670), (1755, 690)], 9),
    "s7": ([(1130, 1080), (1150, 1050), (1170, 1025), (1190, 1015), (1210, 1005), (1230, 985), (1250, 965), (1280, 955), (1310, 957), (1335, 970),
            (1345, 995), (1340, 1020), (1325, 1035), (1315, 1040)], 14),
}


# larguras por ponto (mesma ordem dos pontos de STEMS) lidas nas varreduras por linha/coluna
RAW_W = {}  # s1 com larguras das varreduras piorou na caixa (0,770 → 0,748)


def run():
    lay = node("caules", "Caules", {"type": "layer", "kind": "vector"}, ids["cut"])
    node("caules_c1", "1", {"type": "cel", "number": 1}, lay)
    reorder()
    d.apply({"kind": "replace_data", "id": ids["caules_c1"], "data": {"type": "cel", "number": 1}})
    edge, body = [], []
    for name, (poly, w) in STEMS.items():
        if name in RAW_W:  # centros e larguras lidos nas varreduras (o paquímetro pega só o miolo claro)
            ws = RAW_W[name]
            n = len(poly)
            c = {"points": [list(p) for p in poly], "width": max(ws), "tip": "marker", "geometry": "spline",
                 "color": {"type": "direct", "rgba": [0x5a / 255, 0x24 / 255, 0x4a / 255, 1.0]},
                 "profile": [[i / (n - 1), v / max(ws)] for i, v in enumerate(ws)]}
            body.append(c)
            edge.append(dict(c, color={"type": "direct", "rgba": EDGE}, width=c["width"] + EDGE_EXTRA))
            continue
        if MODE == "meia":  # meia altura do contraste com o fundo creme: pega o caule inteiro (lado claro e lado sombreado)
            pts, ws = meas.measure_line(d, REF, meas.densify(poly, 3), passes=((16.0, 0.06), (10.0, 0.06)), smooth=1)
        else:
            pts, ws = meas.measure_band(d, REF, meas.densify(poly, 3), BODY, TOL, passes=(14.0, 10.0), smooth=1)
        probes = meas.first_list(d.call("PROBE_COLOR", {"reference": REF, "points": [[round(p[0]), round(p[1])] for p in pts[1:-1:3]], "radius": 1}))
        cs = sorted([p["rgb"] for p in probes], key=sum)
        mid = cs[len(cs) // 2] if cs else [0x5c / 255, 0x28 / 255, 0x50 / 255]
        c = meas.curve_from_measure(pts, ws, {"type": "direct", "rgba": list(mid) + [1.0]}, every=1)
        c["width"] = max(c["width"], 0.6 * w)
        c["profile"] = [[t, max(0.25, min(1.6, v))] for t, v in c["profile"]]
        c["width"] *= SCALE
        e = dict(c, color={"type": "direct", "rgba": EDGE}, width=c["width"] + EDGE_EXTRA)
        edge.append(e)
        body.append(c)
    return d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": F, "layer": lay, "curves": edge + body}).get("description")


if __name__ == "__main__":
    print(run())
