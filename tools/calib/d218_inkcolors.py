"""Leitura: cor mediana da tinta medida no centro de cada linha (MEASURE_LINE + PROBE_COLOR). A tinta muda com a área
(mais escura sobre o cabelo de trás) e a luma dela decide se a linha aparece."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import d, REF0
from d218_hair import HAIR, DIRECT

out = {}
for n in list(HAIR) + list(DIRECT):
    poly = HAIR[n] if n in HAIR else DIRECT[n][0]
    pts = meas.densify(poly, 12)
    if len(pts) < 2:
        continue
    r = meas.first_list(d.call("MEASURE_LINE", {"reference": REF0, "points": pts, "search": 10, "min_contrast": 0.05, "line_color": "#5c2e2e"}))
    cs = [[round(m["center"][0]), round(m["center"][1])] for m in r[1:-1] if m["contrast"] > 0]
    if not cs:
        continue
    pr = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": cs, "radius": 1}))
    rgb = sorted([tuple(round(v * 255) for v in p["rgb"]) for p in pr], key=sum)
    out[n] = "#%02x%02x%02x" % rgb[len(rgb) // 2]
json.dump(out, open(os.path.join(os.path.dirname(__file__), "..", "..", "calibracao", "projetos", "d218_ink.json"), "w"), indent=1)
for k, v in out.items():
    print(k, v)
