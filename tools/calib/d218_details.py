"""Corte D, quadro 0: detalhes — fitas brancas no alto do cabelo e gota de suor (formas na raster `detalhe`, bordas medidas)
e brilhos dos olhos (traços brancos curtos na vetorial `brilho`, acima das linhas). Posições lidas nos recortes ampliados."""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node, REF0, ck, CEL, F0, W, edge_score, candidates

WHITE = "#f6f3f6"
SHAPES = {
    "fita_esq": ("#f5e0c7", [(476, 0), (556, 0), (552, 20), (548, 48), (544, 58), (536, 57), (528, 44), (520, 38), (504, 38), (488, 41),
                             (480, 52), (472, 59), (470, 20)], False),
    "fita_dir": ("#f5e0c7", [(776, 0), (862, 0), (862, 20), (858, 30), (850, 28), (842, 23), (830, 26), (818, 33), (806, 37), (794, 45),
                             (782, 48), (777, 40)], False),
    "fita_dir2": ("#f5e0c7", [(884, 0), (922, 0), (921, 22), (914, 25), (902, 27), (890, 36), (885, 30)], False),
    # respiro rosado ao lado do queixo: bordas medidas (rosa × tinta é nítido); cobre a fresta que o balde deixa
    "bochecha": ("#ce6f74", [(558, 668), (565, 690), (580, 715), (597, 745), (612, 770), (628, 797), (600, 800), (570, 795), (552, 788),
                             (551, 750), (553, 700)], True),
    # cunha de tinta entre o braço de k3 e l1 (as duas linhas se fundem numa massa)
    "cunha_k3_l1": ("#6f413c", [(1222, 80), (1240, 86), (1250, 100), (1232, 101), (1224, 97)], False),
    "sombra_l2": ("#872637", [(1330, 92), (1355, 104), (1380, 116), (1392, 122), (1395, 145), (1375, 140), (1350, 125)], True),
}
# redesenhos: formas relidas no próprio desenho (varredura por colunas, runs.py), sem campo
SHAPES_DES = {7: {
    "fita_esq": ("#f5e0c7", [(472, 0), (551, 0), (548, 25), (544, 40), (540, 58), (533, 59), (528, 48), (522, 42), (504, 41), (492, 44),
                             (484, 50), (474, 60), (466, 67), (460, 58), (466, 30), (470, 10)], False),
    "fita_dir": ("#f5e0c7", [(768, 0), (864, 0), (862, 20), (862, 38), (856, 37), (850, 28), (842, 27), (830, 30), (818, 37), (806, 42),
                             (794, 40), (789, 57), (782, 57), (776, 44), (770, 25)], False),
    "fita_dir2": ("#f5e0c7", [(886, 0), (919, 0), (917, 22), (908, 22), (900, 24), (896, 36), (890, 26), (886, 6)], False)}}
# fresta rosada (cor da pele) entre as duas voltas do anel de baixo do olho esquerdo, quando ela cresce no redesenho
GAP_DES = {7: [([(708, 523), (718, 529), (730, 534), (744, 538), (758, 540), (770, 541)], 7.0)]}
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
    ck("detalhe")
    bri = node("brilho", "Brilhos", {"type": "layer", "kind": "vector"}, ids["cut"])
    ck("brilho")
    reorder()
    for k in (ck("detalhe"), ck("brilho")):
        d.apply({"kind": "replace_data", "id": ids[k], "data": {"type": "cel", "number": CEL}})
    raw = SHAPES_DES.get(CEL, {})
    for name, (color, poly, do_measure) in {**SHAPES, **raw}.items():
        best = None
        for cand in ([[tuple(p) for p in poly]] if name in raw else candidates(poly)):  # redesenho: a estimativa do desenho 1 ou a do campo, a que encaixar melhor depois de medida
            pts = meas.densify(cand, 3, closed=True)
            if do_measure:
                pts = meas.measure_edge(d, REF0, pts, passes=((5.0, 0.03), (3.0, 0.03)))
            sc = edge_score(pts) if CEL != 1 else 0
            if best is None or sc > best[0]:
                best = (sc, pts)
        pts = best[1]
        c = [int(color[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": F0, "layer": det, "shapes": [{"points": pts, "geometry": "polyline"}],
                              "color": {"type": "direct", "rgba": c + [1.0]}})
    w = [int(WHITE[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    curves = [{"points": [list(p) for p in W(h)], "width": 4.0, "tip": "marker", "geometry": "spline", "color": {"type": "direct", "rgba": w + [1.0]},
               "profile": [[0, 0.4], [0.5, 1.0], [1, 0.4]]} for h in HIGHLIGHTS]
    pele = [0xf8 / 255, 0xd3 / 255, 0xcb / 255, 1.0]
    for pts, wd in GAP_DES.get(CEL, []):
        curves.append({"points": [list(p) for p in pts], "width": wd, "tip": "marker", "geometry": "spline", "color": {"type": "direct", "rgba": pele},
                       "profile": [[0, 0.3], [0.45, 1.0], [1, 0.3]]})
    # a gota de suor muda por desenho: d218_dyn.py (camadas `gota`/`gota_r`)
    return d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": F0, "layer": bri, "curves": curves})


if __name__ == "__main__":
    print(run().get("description"))
