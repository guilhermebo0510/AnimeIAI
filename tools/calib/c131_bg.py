"""Corte C: fundo creme — degradê radial ELÍPTICO medido (centro 960,560; elipse 1,8× mais larga; cor mediana por anel de raio
nos pixels creme da referência, com o leve anel de aquarela). Radial em círculo + escala horizontal (TRANSFORM)."""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from c131 import d, ids, F

CX, CY, RATIO, RMAX = 960, 560, 1.8, 800.0
RINGS = [(0, "#f2f4c8"), (60, "#f1f3c7"), (100, "#f1f0c5"), (140, "#f1ebc2"), (180, "#f0e3be"), (220, "#f0e2be"), (260, "#f0e5bf"),
         (300, "#f0e8c0"), (340, "#f0e6bf"), (380, "#f0e1bc"), (420, "#efd5b6"), (460, "#eac7ad"), (500, "#ddc2ab"), (540, "#d0beac"),
         (800, "#a8968a")]


def run():
    d.apply({"kind": "replace_data", "id": ids["bg_c1"], "data": {"type": "cel", "number": 1}})
    stops = [[r / RMAX, h + "ff"] for r, h in RINGS]
    d.call("GRADIENT", {"cut": ids["cut"], "frame": F, "layer": ids["bg"], "kind": "radial", "from": [CX, CY], "to": [CX, CY + RMAX],
                        "stops": stops})
    return d.call("TRANSFORM", {"cut": ids["cut"], "frame": F, "layer": ids["bg"],
                                "deform": {"type": "affine", "scale": [RATIO, 1.0], "pivot": [CX, CY]}}).get("description")


if __name__ == "__main__":
    print(run())
