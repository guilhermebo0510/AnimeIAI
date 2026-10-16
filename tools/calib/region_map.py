"""Mapa de regiões do balde (leitura): pinta cada região com uma cor aleatória, renderiza e desfaz.
uso: python tools/calib/region_map.py <projeto> <ids.json> <saida.png> [frame] [gap]"""
import json, random, sys, os
sys.path.insert(0, os.path.dirname(__file__))
from drive import Drive

proj, idsf, out = sys.argv[1:4]
frame = int(sys.argv[4]) if len(sys.argv) > 4 else 0
gap = float(sys.argv[5]) if len(sys.argv) > 5 else 3.0
ids = json.load(open(idsf))
d = Drive(proj)
cel = ids["cor_c1"]
rep = d.call("LINT", {"cut": ids["cut"], "frame": frame, "layer": ids["cor"], "gap": gap, "min_area": 20, "include_border": True})
before = d.call("QUERY", {"what": "node", "id": cel}) if False else None
random.seed(1)
seeds = [{"x": r["point"][0], "y": r["point"][1], "color": {"type": "direct", "rgba": [random.random(), random.random(), random.random(), 1.0]}}
         for r in rep["unpainted"]]
d.apply({"kind": "replace_data", "id": cel, "data": {"type": "cel", "number": 1}})
d.call("FILL", {"cut": ids["cut"], "frame": frame, "layer": ids["cor"], "seeds": seeds, "gap": gap})
d.call("RENDER", {"cut": ids["cut"], "frame": frame, "out": out})
d.apply({"kind": "replace_data", "id": cel, "data": {"type": "cel", "number": 1}})
print(out, "regiões:", rep["regions_total"], "listadas:", len(seeds))
for r in sorted(rep["unpainted"], key=lambda r: -r["area"])[:25]:
    print(r["point"], r["area"])
