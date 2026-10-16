"""Corte D: escurecimento de 1 px na borda do quadro (acabamento da fonte: a primeira/última linha e coluna da referência
saem ~4–10 % mais escuras, fator medido por canal em f_0001 e f_0064, igual nos desenhos). Camada raster `borda_quadro`
em multiplicar, um cel só para o corte inteiro (sem trilha)."""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from d218 import reorder, d, ids, node

# fator por canal (sRGB) medido: mediana de borda ÷ 3 px para dentro, só onde não é escuro
EDGES = {"topo": ("#f4eded", [[0, 0], [1920, 0], [1920, 1], [0, 1]]),
         "base": ("#f1f0e4", [[0, 1079], [1920, 1079], [1920, 1080], [0, 1080]]),
         "esq": ("#efebe6", [[0, 0], [1, 0], [1, 1080], [0, 1080]]),
         "dir": ("#efeae7", [[1919, 0], [1920, 0], [1920, 1080], [1919, 1080]])}


def run():
    lay = node("borda_quadro", "Borda do quadro (multiplicar)", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("borda_quadro_c1", "1", {"type": "cel", "number": 1}, lay)
    d.apply({"kind": "set_props", "id": lay, "data": {"blend": "multiply"}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids["borda_quadro_c1"], "data": {"type": "cel", "number": 1}})
    for name, (hexc, poly) in EDGES.items():
        c = [int(hexc[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": 0, "layer": lay, "shapes": [{"points": poly, "geometry": "polyline"}],
                              "color": {"type": "direct", "rgba": c + [1.0]}})
    return lay


if __name__ == "__main__":
    print(run())
