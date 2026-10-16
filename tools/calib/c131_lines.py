"""Corte C: desenha as linhas da moldura (c131_rosas.LINES) medidas na camada `moldura_linha`. uso: python c131_lines.py"""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from c131 import d, ids, F, measured
from c131_rosas import LINES, FECHO
from c131 import node, reorder

if __name__ == "__main__":
    d.apply({"kind": "replace_data", "id": ids["moldura_linha_c1"], "data": {"type": "cel", "number": 1}})
    curves = []
    for name, (poly, w, closed, *opt) in LINES.items():
        ink, tol = (opt + [None, None])[:2]
        curves.append(measured(name, poly, w, closed=closed, ink=ink or "#1a0508", tol=tol or 0.12))
    print(d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": F, "layer": ids["moldura_linha"], "curves": curves}).get("description"))
    # linhas de fechamento do balde (camada oculta, só nas line_refs da cor)
    f = node("fecho", "Fecho do balde", {"type": "layer", "kind": "vector"}, ids["cut"])
    node("fecho_c1", "1", {"type": "cel", "number": 1}, f)
    d.apply({"kind": "set_props", "id": f, "data": {"visible": False}})
    d.apply({"kind": "set_props", "id": ids["moldura_cor"], "data": {"line_refs": [ids["moldura_linha"], f]}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids["fecho_c1"], "data": {"type": "cel", "number": 1}})
    if FECHO:
        fc = [{"points": [list(p) for p in c], "width": 3.0, "tip": "marker", "geometry": "polyline",
               "color": {"type": "direct", "rgba": [0, 0, 0, 1]}} for c in FECHO]
        print(d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": F, "layer": f, "curves": fc}).get("description"))
