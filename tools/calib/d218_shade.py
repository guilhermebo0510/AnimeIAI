"""Corte D, quadro 0: sombreamento do cabelo na camada `realce` (recortada na cor): escurecimento para baixo
(multiplicar) e degradê claro da franja; cores dos extremos medidas com PROBE_COLOR."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from d218 import reorder, d, ids, node, render


def layers():
    s = node("sombra", "Sombra (multiplicar)", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("sombra_c1", "1", {"type": "cel", "number": 1}, s)
    d.apply({"kind": "set_props", "id": s, "data": {"blend": "multiply", "clip_to_below": True}})
    reorder()


def shade():
    layers()
    d.apply({"kind": "replace_data", "id": ids["realce_c1"], "data": {"type": "cel", "number": 1}})
    d.apply({"kind": "replace_data", "id": ids["sombra_c1"], "data": {"type": "cel", "number": 1}})
    out = []
    # base #a31f3c → embaixo #7f0a25 (y≈1000): fator por canal em sRGB ≈ (0,78; 0,32; 0,62)
    out.append(d.call("GRADIENT", {"cut": ids["cut"], "frame": 0, "layer": ids["sombra"], "kind": "linear", "from": [0, 640], "to": [0, 1060],
                                   "stops": [(0.0, "#ffffff"), (0.5, "#e6a3c4"), (1.0, "#c7529e")]}))
    return out


if __name__ == "__main__":
    print([r.get("description") for r in shade()])
    print(render("q0_sombra"))
