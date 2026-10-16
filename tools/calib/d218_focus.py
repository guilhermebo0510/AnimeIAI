"""Corte D, quadro 0: profundidade de campo do satsuei — o alto (y < ~120) e a base do quadro estão desfocados na
referência. Camada de efeito `foco` (desfoque gaussiano) com máscara `foco_mascara` (degradê vertical, oculta).
uso: python d218_focus.py [raio] [y_topo_nitido] [y_base_nitido]"""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from d218 import reorder, d, ids, node


def run(radius=4.0, top=110, bottom=2000):
    m = node("foco_mascara", "Máscara do foco", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("foco_mascara_c1", "1", {"type": "cel", "number": 1}, m)
    f = node("foco", "Profundidade de campo", {"type": "layer", "kind": "effect"}, ids["cut"])
    d.apply({"kind": "set_props", "id": m, "data": {"visible": False}})
    d.apply({"kind": "set_props", "id": f, "data": {"mask": m, "params": {"effects": [{"effect": {"type": "gaussian_blur", "radius": radius}}]}}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids["foco_mascara_c1"], "data": {"type": "cel", "number": 1}})
    # o plano de foco é inclinado: o desfoque do alto desce até y≈190 na esquerda e só até y≈70 na direita (leitura das bordas
    # que sobram/faltam). Degradê ao longo da normal dessa reta: branco 80 px acima dela, preto sobre ela.
    return d.call("GRADIENT", {"cut": ids["cut"], "frame": 0, "layer": m, "kind": "linear", "from": [960, 50], "to": [965, 130],
                               "stops": [(0.0, "#ffffff"), (1.0, "#000000")]})


# cabelo de trás, sob o queixo à esquerda: camada de trás desfocada na referência (as linhas não dão borda nítida)
TRAS = [(450, 790), (545, 800), (600, 815), (650, 850), (700, 880), (760, 912), (840, 950), (930, 975), (942, 1082), (440, 1082)]


def tras(radius=12.0):
    m = node("foco_tras_mascara", "Máscara do foco de trás", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("foco_tras_mascara_c1", "1", {"type": "cel", "number": 1}, m)
    f = node("foco_tras", "Desfoque do cabelo de trás", {"type": "layer", "kind": "effect"}, ids["cut"])
    d.apply({"kind": "set_props", "id": m, "data": {"visible": False}})
    d.apply({"kind": "set_props", "id": f, "data": {"mask": m, "params": {"effects": [{"effect": {"type": "gaussian_blur", "radius": radius}}]}}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids["foco_tras_mascara_c1"], "data": {"type": "cel", "number": 1}})
    return d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": 0, "layer": m, "shapes": [{"points": [list(p) for p in TRAS], "geometry": "polyline"}],
                                 "color": {"type": "direct", "rgba": [1, 1, 1, 1]}})


if __name__ == "__main__":
    a = [float(x) for x in sys.argv[1:4]]
    print(run(*a).get("description"))
    print(tras().get("description"))
