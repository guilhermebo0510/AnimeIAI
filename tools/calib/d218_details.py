"""Corte D, quadro 0: detalhes — fitas brancas no alto do cabelo e gota de suor (formas na raster `detalhe`, bordas medidas)
e brilhos dos olhos (traços brancos curtos na vetorial `brilho`, acima das linhas). Posições lidas nos recortes ampliados."""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node, REF0

WHITE = "#f6f3f6"
SHAPES = {
    "fita_esq": ("#f5e0c7", [(468, 0), (556, 0), (552, 20), (546, 40), (520, 42), (495, 40), (485, 45), (480, 60), (470, 60)], True),
    "fita_dir": ("#f5e0c7", [(772, 0), (858, 0), (857, 20), (852, 36), (840, 38), (820, 36), (800, 38), (788, 42), (781, 57), (775, 30)], True),
    "fita_dir2": ("#f5e0c7", [(883, 0), (935, 0), (935, 22), (918, 25), (905, 36), (895, 38), (885, 30)], True),
    # respiro rosado ao lado do queixo: bordas medidas (rosa × tinta é nítido); cobre a fresta que o balde deixa
    "bochecha": ("#ce6f74", [(558, 668), (565, 690), (580, 715), (597, 745), (612, 770), (628, 797), (600, 800), (570, 795), (552, 788),
                             (551, 750), (553, 700)], True),
    "sombra_l2": ("#872637", [(1330, 92), (1355, 104), (1380, 116), (1392, 122), (1395, 145), (1375, 140), (1350, 125)], True),
    "gota_sombra": ("#d77882", [(690, 752), (725, 752), (726, 760), (720, 768), (708, 771), (696, 769), (690, 762)], False),
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
        pts = meas.densify(poly, 3, closed=True)
        if do_measure:
            pts = meas.measure_edge(d, REF0, pts, passes=((5.0, 0.03), (3.0, 0.03)))
        c = [int(color[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": 0, "layer": det, "shapes": [{"points": pts, "geometry": "polyline"}],
                              "color": {"type": "direct", "rgba": c + [1.0]}})
    w = [int(WHITE[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    curves = [{"points": [list(p) for p in h], "width": 4.0, "tip": "marker", "geometry": "spline", "color": {"type": "direct", "rgba": w + [1.0]},
               "profile": [[0, 0.4], [0.5, 1.0], [1, 0.4]]} for h in HIGHLIGHTS]
    # gota de suor: traço branco grosso enrolado (centro cor de pele), lido no recorte ampliado 4×
    gw = {"type": "direct", "rgba": [0xf6 / 255, 0xf2 / 255, 0xf6 / 255, 1.0]}
    # gota: "U" grosso (braço esquerdo mais alto), lido no recorte 4× e conferido nas bordas
    curves.append({"points": [[691, 707], [688, 725], [692, 744], [705, 755], [720, 755], [731, 746], [735, 734], [733, 722]],
                   "width": 17.0, "tip": "marker", "geometry": "spline", "color": gw})
    curves.append({"points": [[705, 729], [707, 735]], "width": 9.0, "tip": "marker", "geometry": "polyline", "color": gw})
    return d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": 0, "layer": bri, "curves": curves})


if __name__ == "__main__":
    print(run().get("description"))
