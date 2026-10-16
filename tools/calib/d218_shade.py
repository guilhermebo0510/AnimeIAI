"""Corte D, quadro 0: sombreamento do cabelo na camada `realce` (recortada na cor): escurecimento para baixo
(multiplicar) e degradê claro da franja; cores dos extremos medidas com PROBE_COLOR."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from d218 import reorder, d, ids, node, render, ck, CEL, F0


def layers():
    s = node("sombra", "Sombra (multiplicar)", {"type": "layer", "kind": "raster"}, ids["cut"])
    ck("sombra")
    d.apply({"kind": "set_props", "id": s, "data": {"blend": "multiply", "clip_to_below": True}})
    reorder()


def shade():
    layers()
    d.apply({"kind": "replace_data", "id": ids[ck("realce")], "data": {"type": "cel", "number": CEL}})
    d.apply({"kind": "replace_data", "id": ids[ck("sombra")], "data": {"type": "cel", "number": CEL}})
    out = []
    # base #a31f3c → embaixo #7f0a25 (y≈1000): fator por canal em sRGB ≈ (0,78; 0,32; 0,62)
    out.append(d.call("GRADIENT", {"cut": ids["cut"], "frame": F0, "layer": ids["sombra"], "kind": "linear", "from": [0, 0], "to": [0, 1080],
                                   # fatores por canal medidos (cabelo da referência ÷ base #9c1b38) por faixa de altura
                                   "stops": [(0.0, "#ffffff"), (0.5, "#ffffff"), (0.65, "#f7c4e3"), (0.93, "#cf62a3"), (1.0, "#c85a9a")]}))
    return out


if __name__ == "__main__":
    print([r.get("description") for r in shade()])
    print(render("q0_sombra"))
