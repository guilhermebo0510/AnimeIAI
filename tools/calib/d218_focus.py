"""Corte D, quadro 0: profundidade de campo do satsuei — o alto (y < ~120) e a base do quadro estão desfocados na
referência. Camada de efeito `foco` (desfoque gaussiano) com máscara `foco_mascara` (degradê vertical, oculta).
uso: python d218_focus.py [raio] [y_topo_nitido] [y_base_nitido]"""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from d218 import reorder, d, ids, node


def run(radius=8.0, top=130, bottom=2000):
    m = node("foco_mascara", "Máscara do foco", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("foco_mascara_c1", "1", {"type": "cel", "number": 1}, m)
    f = node("foco", "Profundidade de campo", {"type": "layer", "kind": "effect"}, ids["cut"])
    d.apply({"kind": "set_props", "id": m, "data": {"visible": False}})
    d.apply({"kind": "set_props", "id": f, "data": {"mask": m, "params": {"effects": [{"effect": {"type": "gaussian_blur", "radius": radius}}]}}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids["foco_mascara_c1"], "data": {"type": "cel", "number": 1}})
    t0, b0 = top / 1080, bottom / 1080
    stops = [(0.0, "#ffffff"), (max(0.0, t0 - 0.09), "#ffffff"), (t0, "#000000")]
    if b0 < 1.0:  # base desfocada só se pedida (no quadro 0 a roupa embaixo está nítida)
        stops += [(b0, "#000000"), (min(1.0, b0 + 0.15), "#ffffff"), (1.0, "#ffffff")]
    else:
        stops += [(1.0, "#000000")]
    return d.call("GRADIENT", {"cut": ids["cut"], "frame": 0, "layer": m, "kind": "linear", "from": [0, 0], "to": [0, 1080], "stops": stops})


if __name__ == "__main__":
    a = [float(x) for x in sys.argv[1:4]]
    print(run(*a).get("description"))
