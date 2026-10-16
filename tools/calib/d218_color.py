"""Corte D, quadro 0: cor. BG por degradê medido (coluna da esquerda/direita); balde na camada `cor` com a paleta
do design (cada região recebe a cor de paleta mais próxima da mediana medida no seu ponto mais interno)."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node, REF0, render

ROSTO = {"pele", "branco_olho", "boca", "laco"}  # não-cabelo: vão para `cor_rosto` (acima da sombra/realce do cabelo)

PALETTE = {
    "cabelo": "#9d1837", "cabelo_franja": "#e15a5a", "cabelo_franja2": "#d94a4f", "cabelo_topo": "#b23a4c",
    "cabelo_rim": "#f49089", "cabelo_sombra": "#7b0922", "cabelo_tras": "#660821", "pele": "#f8d3cb",
    "branco_olho": "#f6f3f6", "boca": "#ea7582", "laco": "#f4f5dc",
}
BG = {"bg_bege": "#c1a083", "bg_escuro": "#4d2418", "bg_madeira": "#5e3619", "bg_topo": "#5a3423"}


MANUAL = {(450, 400): "cabelo", (1500, 420): "cabelo", (1480, 640): "cabelo", (562, 650): "cabelo"}  # a roupa fica na camada `baixo`


def rgb(h):
    return [int(h[i:i + 2], 16) for i in (1, 3, 5)]


def nearest(hexc):
    c = rgb(hexc)
    best = min(list(PALETTE.items()) + list(BG.items()), key=lambda kv: sum((a - b) ** 2 for a, b in zip(rgb(kv[1]), c)))
    dist = sum((a - b) ** 2 for a, b in zip(rgb(best[1]), c)) ** 0.5
    return best[0], best[1], dist


def background():
    d.apply({"kind": "replace_data", "id": ids["bg_c1"], "data": {"type": "cel", "number": 1}})
    ys = list(range(30, 1080, 30))
    left = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": [[60, y] for y in ys], "radius": 6}))
    stops = [(round(y / 1080, 4), c["hex"]) for y, c in zip(ys, left)]
    stops = [(0.0, stops[0][1])] + stops + [(1.0, stops[-1][1])]
    return d.call("GRADIENT", {"cut": ids["cut"], "frame": 0, "layer": ids["bg"], "kind": "linear", "from": [0, 0], "to": [0, 1080],
                               "stops": stops})


def rosto_layer():
    r = node("cor_rosto", "Cor do rosto", {"type": "layer", "kind": "fill"}, ids["cut"])
    node("cor_rosto_c1", "1", {"type": "cel", "number": 1}, r)
    d.apply({"kind": "set_props", "id": r, "data": {"line_refs": [ids["linha"], ids["linha_massa"], ids["fecho"]]}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids["cor_rosto_c1"], "data": {"type": "cel", "number": 1}})
    return r


def fills(min_area=150, gap=6.0):
    rosto = rosto_layer()
    d.apply({"kind": "replace_data", "id": ids["cor_c1"], "data": {"type": "cel", "number": 1}})
    rep = d.call("LINT", {"cut": ids["cut"], "frame": 0, "layer": ids["cor"], "gap": gap, "min_area": min_area, "include_border": True})
    regs = rep["unpainted"]
    probes = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": [r["point"] for r in regs], "radius": 3}))
    seeds, face, log = [], [], []
    for r, p in zip(regs, probes):
        name, hexc, dist = nearest(p["hex"])
        log.append((r["point"], r["area"], p["hex"], name, round(dist)))
        if name in BG or dist > 45 or (name in ROSTO and dist > 15):
            continue  # fundo, ou ambígua (fica para a leitura manual abaixo)
        seed = {"x": r["point"][0], "y": r["point"][1], "color": {"type": "direct", "rgba": [v / 255 for v in rgb(hexc)] + [1.0]}}
        (face if name in ROSTO else seeds).append(seed)
    # leitura do agente: regiões cujo ponto mais interno cai numa área não representativa
    for (x, y), name in MANUAL.items():
        seeds.append({"x": x, "y": y, "color": {"type": "direct", "rgba": [v / 255 for v in rgb(PALETTE[name])] + [1.0]}})
    res = d.call("FILL", {"cut": ids["cut"], "frame": 0, "layer": ids["cor"], "seeds": seeds, "gap": gap})
    # pele numa camada própria com degradê vertical medido (topo mais claro); olhos/boca/laço ficam em `cor_rosto`
    pele = node("pele", "Pele", {"type": "layer", "kind": "fill"}, ids["cut"])
    node("pele_c1", "1", {"type": "cel", "number": 1}, pele)
    luz = node("pele_luz", "Pele degradê", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("pele_luz_c1", "1", {"type": "cel", "number": 1}, luz)
    d.apply({"kind": "set_props", "id": pele, "data": {"line_refs": [ids["linha"], ids["linha_massa"], ids["fecho"]]}})
    d.apply({"kind": "set_props", "id": luz, "data": {"clip_to_below": True}})
    reorder()
    for k in ("pele_c1", "pele_luz_c1"):
        d.apply({"kind": "replace_data", "id": ids[k], "data": {"type": "cel", "number": 1}})
    skin = [sd for sd in face if abs(sd["color"]["rgba"][0] - 0xf8 / 255) < 1e-3 and abs(sd["color"]["rgba"][1] - 0xd3 / 255) < 1e-3]
    rest = [sd for sd in face if sd not in skin]
    d.call("FILL", {"cut": ids["cut"], "frame": 0, "layer": pele, "seeds": skin, "gap": gap})
    if rest:
        d.call("FILL", {"cut": ids["cut"], "frame": 0, "layer": rosto, "seeds": rest, "gap": gap})
    d.call("GRADIENT", {"cut": ids["cut"], "frame": 0, "layer": luz, "kind": "linear", "from": [0, 120], "to": [0, 900],
                        "stops": [(0.0, "#fad9d0"), (0.45, "#f8d2ca"), (1.0, "#f6cbc3")]})
    return rep["regions_total"], log, res


if __name__ == "__main__":
    pass  # fundo: d218_bg.py
    total, log, res = fills()
    print("regiões", total, "listadas", len(log))
    for row in log:
        print(row)
    print(res.get("description"))
    print(render("q0_cor"))
