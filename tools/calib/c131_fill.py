"""Corte C: cor da moldura por balde na camada `moldura_cor` (line_refs = linhas). Cada região recebe a cor MEDIDA no seu ponto
mais interno (mediana raio 3); regiões com cor de fundo ficam vazias (o BG aparece). uso: python c131_fill.py [gap]"""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from c131 import d, ids, F, REF

BG = ["#f0ebc2", "#eed9b7", "#e1c0a8", "#f2f5ca", "#ecccae", "#e6bba5"]
MANUAL = {}  # (x, y): "#rrggbb" — leituras do agente onde o ponto mais interno cai mal


def rgb(h):
    return [int(h[i:i + 2], 16) for i in (1, 3, 5)]


def run(gap=4.0, min_area=40):
    d.apply({"kind": "replace_data", "id": ids["moldura_cor_c1"], "data": {"type": "cel", "number": 1}})
    rep = d.call("LINT", {"cut": ids["cut"], "frame": F, "layer": ids["moldura_cor"], "gap": gap, "min_area": min_area, "include_border": True})
    regs = rep["unpainted"]
    probes = meas.first_list(d.call("PROBE_COLOR", {"reference": REF, "points": [r["point"] for r in regs], "radius": 3}))
    seeds, skipped = [], 0
    for r, p in zip(regs, probes):
        c = rgb(p["hex"])
        if min(sum((a - b) ** 2 for a, b in zip(c, rgb(h))) ** 0.5 for h in BG) < 30:
            skipped += 1
            continue
        seeds.append({"x": r["point"][0], "y": r["point"][1], "color": {"type": "direct", "rgba": [v / 255 for v in c] + [1.0]}})
    for (x, y), h in MANUAL.items():
        seeds.append({"x": x, "y": y, "color": {"type": "direct", "rgba": [v / 255 for v in rgb(h)] + [1.0]}})
    res = d.call("FILL", {"cut": ids["cut"], "frame": F, "layer": ids["moldura_cor"], "seeds": seeds, "gap": gap})
    return rep["regions_total"], len(seeds), skipped, res.get("description")


if __name__ == "__main__":
    print(run(*(float(a) for a in sys.argv[1:2])))
