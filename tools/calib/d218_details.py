"""Corte D, quadro 0: detalhes — fitas brancas no alto do cabelo e gota de suor (formas na raster `detalhe`, bordas medidas)
e brilhos dos olhos (traços brancos curtos na vetorial `brilho`, acima das linhas). Posições lidas nos recortes ampliados."""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node, REF0

WHITE = "#f6f3f6"
SHAPES = {
    "fita_esq": ("#f5e0c7", [(438, 0), (556, 0), (553, 20), (548, 45), (540, 66), (528, 58), (512, 46), (498, 42), (482, 42), (470, 50),
                             (462, 63), (442, 63)], True),
    "fita_dir": ("#f5e0c7", [(772, 0), (935, 0), (935, 22), (918, 25), (905, 36), (880, 30), (858, 22), (835, 25), (815, 30), (800, 40),
                             (790, 68), (780, 65)], True),
    "gota": ("#f6f2f6", [(682, 712), (692, 705), (700, 712), (702, 725), (715, 722), (728, 728), (733, 742), (725, 755), (708, 760),
                         (690, 755), (682, 742), (680, 725)], True),
    "gota_sombra": ("#d77882", [(690, 757), (702, 762), (718, 762), (722, 757), (722, 762), (712, 770), (698, 770), (690, 764)], False),
}
HIGHLIGHTS = [
    [(747, 351), (755, 349), (762, 347)],
    [(737, 537), (751, 539), (765, 540), (778, 539), (790, 537)],
    [(852, 382), (854, 385), (855, 388)],
    [(1290, 482), (1302, 480), (1315, 477)],
    [(1382, 365), (1384, 377), (1385, 390)],
    [(1320, 302), (1325, 304), (1330, 307)],
]


def run():
    det = node("detalhe", "Detalhes", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("detalhe_c1", "1", {"type": "cel", "number": 1}, det)
    bri = node("brilho", "Brilhos", {"type": "layer", "kind": "vector"}, ids["cut"])
    node("brilho_c1", "1", {"type": "cel", "number": 1}, bri)
    reorder()
    for k in ("detalhe_c1", "brilho_c1"):
        d.apply({"kind": "replace_data", "id": ids[k], "data": {"type": "cel", "number": 1}})
    for name, (color, poly, do_measure) in SHAPES.items():
        pts = meas.densify(poly, 4, closed=True)
        if do_measure:
            pts = meas.measure_edge(d, REF0, pts, passes=((5.0, 0.03), (3.0, 0.03)))
        c = [int(color[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": 0, "layer": det, "shapes": [{"points": pts, "geometry": "polyline"}],
                              "color": {"type": "direct", "rgba": c + [1.0]}})
    w = [int(WHITE[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    curves = [{"points": [list(p) for p in h], "width": 4.0, "tip": "marker", "geometry": "spline", "color": {"type": "direct", "rgba": w + [1.0]},
               "profile": [[0, 0.4], [0.5, 1.0], [1, 0.4]]} for h in HIGHLIGHTS]
    return d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": 0, "layer": bri, "curves": curves})


if __name__ == "__main__":
    print(run().get("description"))
