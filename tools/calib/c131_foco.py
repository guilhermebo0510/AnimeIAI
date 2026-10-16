"""Corte C: foco suave na periferia — camada de efeito com desfoque, mascarada por um degradê radial elíptico (preto no centro
→ branco nas bordas, onde estão as rosas). Raio e início do degradê ajustados na régua. uso: python c131_foco.py [raio] [t0]"""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from c131 import d, ids, F, node, reorder

CX, CY, RATIO, R = 960, 560, 1.8, 640.0


def run(radius=3.0, t0=0.55):
    m = node("foco_mascara", "Máscara do foco", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("foco_mascara_c1", "1", {"type": "cel", "number": 1}, m)
    f = node("foco", "Foco suave da periferia", {"type": "layer", "kind": "effect"}, ids["cut"])
    d.apply({"kind": "set_props", "id": m, "data": {"visible": False}})
    d.apply({"kind": "set_props", "id": f, "data": {"mask": m, "params": {"effects": [{"effect": {"type": "gaussian_blur", "radius": radius}}]}}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids["foco_mascara_c1"], "data": {"type": "cel", "number": 1}})
    d.call("GRADIENT", {"cut": ids["cut"], "frame": F, "layer": m, "kind": "radial", "from": [CX, CY], "to": [CX, CY + R],
                        "stops": [[0.0, "#000000ff"], [t0, "#000000ff"], [1.0, "#ffffffff"]]})
    return d.call("TRANSFORM", {"cut": ids["cut"], "frame": F, "layer": m, "deform": {"type": "affine", "scale": [RATIO, 1.0], "pivot": [CX, CY]}}).get("description")


if __name__ == "__main__":
    print(run(*(float(a) for a in sys.argv[1:3])))
