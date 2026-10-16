"""Corte D, quadro 0: franja clara. Camada de cor `franja` (balde nas regiões da franja, fechadas no topo por uma linha
oculta) + `franja_luz` recortada com degradê radial medido; e clareamento do topo do cabelo na `realce`."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node, REF0, render

BOX = (740, 40, 1235, 352)
LIGHT = (0xda, 0x56, 0x5a)


def layers():
    f = node("franja", "Franja", {"type": "layer", "kind": "fill"}, ids["cut"])
    node("franja_c1", "1", {"type": "cel", "number": 1}, f)
    fl = node("franja_luz", "Franja luz", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("franja_luz_c1", "1", {"type": "cel", "number": 1}, fl)
    d.apply({"kind": "set_props", "id": f, "data": {"line_refs": [ids["linha"], ids["linha_massa"], ids["fecho"]]}})
    d.apply({"kind": "set_props", "id": fl, "data": {"clip_to_below": True}})
    # a franja clareia aos poucos de cima para baixo (sem borda dura no topo): máscara vertical nas duas camadas
    m = node("franja_mascara", "Máscara da franja", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("franja_mascara_c1", "1", {"type": "cel", "number": 1}, m)
    d.apply({"kind": "set_props", "id": m, "data": {"visible": False}})
    for lay in (f, fl):
        d.apply({"kind": "set_props", "id": lay, "data": {"mask": m}})
    reorder()
    for k in ("franja_c1", "franja_luz_c1", "realce_c1", "franja_mascara_c1"):
        d.apply({"kind": "replace_data", "id": ids[k], "data": {"type": "cel", "number": 1}})
    return f, fl


def run():
    f, fl = layers()
    rep = d.call("LINT", {"cut": ids["cut"], "frame": 0, "layer": f, "gap": 6, "min_area": 150, "include_border": True})
    regs = [r for r in rep["unpainted"] if BOX[0] <= r["point"][0] <= BOX[2] and BOX[1] <= r["point"][1] <= BOX[3]]
    probes = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": [r["point"] for r in regs], "radius": 3}))
    seeds, log = [], []
    for r, p in zip(regs, probes):
        c = [int(p["hex"][i:i + 2], 16) for i in (1, 3, 5)]
        dist = sum((a - b) ** 2 for a, b in zip(c, LIGHT)) ** 0.5
        log.append((r["point"], r["area"], p["hex"], round(dist)))
        if dist < 60:
            seeds.append({"x": r["point"][0], "y": r["point"][1], "color": {"type": "direct", "rgba": [0xd0 / 255, 0x46 / 255, 0x50 / 255, 1.0]}})
    d.call("FILL", {"cut": ids["cut"], "frame": 0, "layer": f, "seeds": seeds, "gap": 6})
    d.call("GRADIENT", {"cut": ids["cut"], "frame": 0, "layer": ids["franja_mascara"], "kind": "linear", "from": [0, 15], "to": [0, 140],
                        "stops": [(0.0, "#000000"), (1.0, "#ffffff")]})
    d.call("GRADIENT", {"cut": ids["cut"], "frame": 0, "layer": fl, "kind": "radial", "from": [1000, 225], "to": [1290, 225],
                        "stops": [(0.0, "#e25a5a"), (0.5, "#d94a50"), (1.0, "#b5374f")]})
    # topo do cabelo mais claro que a base (#b4344d em y≈0 → base em y≈380)
    d.call("GRADIENT", {"cut": ids["cut"], "frame": 0, "layer": ids["realce"], "kind": "linear", "from": [0, 0], "to": [0, 380],
                        "stops": [(0.0, "#b5364eff"), (1.0, "#b5364e00")]})
    return log


if __name__ == "__main__":
    for row in run():
        print(row)
    r = d.call("RENDER", {"cut": ids["cut"], "frame": 0, "out": r"C:\AnimeIAI\sprints\registro\calibracao\D218\q0_franja.png"})
    print(r.get("warnings"))
