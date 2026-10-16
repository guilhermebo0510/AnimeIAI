"""Corte C: satsuei — desfoque gaussiano leve no quadro inteiro (a referência tem foco suave). uso: python c131_fx.py [raio]"""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from c131 import d, ids

RADIUS = 3.0


def run(radius=RADIUS):
    return d.apply({"kind": "set_props", "id": ids["fx"], "data": {"params": {"effects": [{"effect": {"type": "gaussian_blur", "radius": radius}}]}}})


if __name__ == "__main__":
    print(run(float(sys.argv[1]) if len(sys.argv) > 1 else RADIUS))
