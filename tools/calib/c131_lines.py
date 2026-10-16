"""Corte C: desenha as linhas da moldura (c131_rosas.LINES) medidas na camada `moldura_linha`. uso: python c131_lines.py"""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from c131 import d, ids, F, measured
from c131_rosas import LINES

if __name__ == "__main__":
    d.apply({"kind": "replace_data", "id": ids["moldura_linha_c1"], "data": {"type": "cel", "number": 1}})
    curves = []
    for name, (poly, w, closed, *opt) in LINES.items():
        ink, tol = (opt + [None, None])[:2]
        curves.append(measured(name, poly, w, closed=closed, ink=ink or "#1a0508", tol=tol or 0.12))
    print(d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": F, "layer": ids["moldura_linha"], "curves": curves}).get("description"))
