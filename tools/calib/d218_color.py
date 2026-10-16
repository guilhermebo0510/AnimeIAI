"""Corte D, quadro 0: cor. BG por degradê medido (coluna da esquerda/direita); balde na camada `cor` com a paleta
do design (cada região recebe a cor de paleta mais próxima da mediana medida no seu ponto mais interno)."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node, REF0, render, ck, CEL, F0, W

ROSTO = {"pele", "branco_olho", "boca", "laco", "bochecha"}  # não-cabelo: vão para `cor_rosto` (acima da sombra/realce do cabelo)

PALETTE = {
    "cabelo": "#9d1837", "cabelo_franja": "#e15a5a", "cabelo_franja2": "#d94a4f", "cabelo_topo": "#b23a4c",
    "cabelo_rim": "#f49089", "cabelo_sombra": "#7b0922", "cabelo_tras": "#660821", "pele": "#f8d3cb",
    "branco_olho": "#f6f3f6", "boca": "#ea7582", "laco": "#f4f5dc", "bochecha": "#ce6f74",
}
BG = {"bg_bege": "#c1a083", "bg_escuro": "#4d2418", "bg_madeira": "#5e3619", "bg_topo": "#5a3423"}


MANUAL = {(450, 400): "cabelo", (1500, 420): "cabelo", (1480, 640): "cabelo", (575, 750): "bochecha", (372, 800): "cabelo", (900, 600): "pele", (1136, 103): "cabelo_topo"}  # a roupa fica na camada `baixo`


# sementes do agente por desenho (coordenadas já do desenho, sem campo)
MANUAL_DES = {7: {(745, 70): "cabelo_franja2", (615, 240): "cabelo", (1100, 341): "pele", (1766, 829): "cabelo"},
              8: {(1100, 334): "pele"}}


def rgb(h):
    return [int(h[i:i + 2], 16) for i in (1, 3, 5)]


def nearest(hexc):
    c = rgb(hexc)
    best = min(list(PALETTE.items()) + list(BG.items()), key=lambda kv: sum((a - b) ** 2 for a, b in zip(rgb(kv[1]), c)))
    dist = sum((a - b) ** 2 for a, b in zip(rgb(best[1]), c)) ** 0.5
    return best[0], best[1], dist


def settle(x, y, name, rmax=14):
    """Ponto mais perto de (x, y), em anéis de 2 px, cuja cor medida fica a menos de 40 da cor da classe `name`."""
    cand = [(x, y)] + [(x + dx, y + dy) for r in range(2, rmax + 1, 2) for dx in range(-r, r + 1, 2) for dy in range(-r, r + 1, 2)
                       if max(abs(dx), abs(dy)) == r]
    pr = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": [list(p) for p in cand], "radius": 1}))
    target = rgb(PALETTE[name])
    for p, c in zip(cand, pr):
        if sum((a - b) ** 2 for a, b in zip(rgb(c["hex"]), target)) ** 0.5 < 40:
            return p
    return x, y


def background():
    d.apply({"kind": "replace_data", "id": ids[ck("bg")], "data": {"type": "cel", "number": CEL}})
    ys = list(range(30, 1080, 30))
    left = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": [[60, y] for y in ys], "radius": 6}))
    stops = [(round(y / 1080, 4), c["hex"]) for y, c in zip(ys, left)]
    stops = [(0.0, stops[0][1])] + stops + [(1.0, stops[-1][1])]
    return d.call("GRADIENT", {"cut": ids["cut"], "frame": F0, "layer": ids["bg"], "kind": "linear", "from": [0, 0], "to": [0, 1080],
                               "stops": stops})


def rosto_layer():
    r = node("cor_rosto", "Cor do rosto", {"type": "layer", "kind": "fill"}, ids["cut"])
    ck("cor_rosto")
    d.apply({"kind": "set_props", "id": r, "data": {"line_refs": [ids["linha"], ids["linha_massa"], ids["fecho"]]}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids[ck("cor_rosto")], "data": {"type": "cel", "number": CEL}})
    return r


def fills(min_area=150, gap=6.0):
    rosto = rosto_layer()
    d.apply({"kind": "replace_data", "id": ids[ck("cor")], "data": {"type": "cel", "number": CEL}})
    rep = d.call("LINT", {"cut": ids["cut"], "frame": F0, "layer": ids["cor"], "gap": gap, "min_area": min_area, "include_border": True})
    regs = rep["unpainted"]
    probes = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": [r["point"] for r in regs], "radius": 3}))
    seeds, face, log = [], [], []
    for r, p in zip(regs, probes):
        name, hexc, dist = nearest(p["hex"])
        if name == "cabelo_rim":  # realces ficam na camada `rim`; a região do balde é cabelo
            name, hexc, dist = "cabelo", PALETTE["cabelo"], 0
        log.append((r["point"], r["area"], p["hex"], name, round(dist)))
        if r["area"] > 300000:
            continue  # rosto inteiro / fundo: o ponto mais interno cai em qualquer coisa (boca etc.); sementes manuais decidem
        if name == "pele" and dist > 20:
            continue  # pele automática só quando a amostra é pele mesmo (a fita creme no alto cai perto da pele)
        if name in ROSTO and name != "pele" and r["area"] > 100000:
            continue  # região do rosto inteiro com o ponto mais interno caindo na boca/olho (camadas próprias): a semente manual da pele decide
        if name in BG or dist > 45 or (name in ROSTO and dist > (30 if r["area"] < 30000 else 15)):
            continue  # fundo, ou ambígua (fica para a leitura manual abaixo)
        seed = {"x": r["point"][0], "y": r["point"][1], "color": {"type": "direct", "rgba": [v / 255 for v in rgb(hexc)] + [1.0]}}
        (face if name in ROSTO else seeds).append(seed)
    # leitura do agente: regiões cujo ponto mais interno cai numa área não representativa
    for (x, y), name in MANUAL.items():
        x, y = (round(v) for v in W([(x, y)])[0])
        if CEL != 1:  # semente levada pelo campo pode cair na tinta: o ponto mais perto com a cor da classe
            x, y = settle(x, y, name)
        seed = {"x": x, "y": y, "color": {"type": "direct", "rgba": [v / 255 for v in rgb(PALETTE[name])] + [1.0]}}
        (face if name in ROSTO else seeds).append(seed)
    for (x, y), name in MANUAL_DES.get(CEL, {}).items():
        x, y = settle(x, y, name)
        seed = {"x": x, "y": y, "color": {"type": "direct", "rgba": [v / 255 for v in rgb(PALETTE[name])] + [1.0]}}
        (face if name in ROSTO else seeds).append(seed)
    res = d.call("FILL", {"cut": ids["cut"], "frame": F0, "layer": ids["cor"], "seeds": seeds, "gap": gap})
    # pele numa camada própria com degradê vertical medido (topo mais claro); olhos/boca/laço ficam em `cor_rosto`
    pele = node("pele", "Pele", {"type": "layer", "kind": "fill"}, ids["cut"])
    ck("pele")
    luz = node("pele_luz", "Pele degradê", {"type": "layer", "kind": "raster"}, ids["cut"])
    ck("pele_luz")
    d.apply({"kind": "set_props", "id": pele, "data": {"line_refs": [ids["linha"], ids["linha_massa"], ids["fecho"]]}})
    d.apply({"kind": "set_props", "id": luz, "data": {"clip_to_below": True}})
    reorder()
    for k in (ck("pele"), ck("pele_luz")):
        d.apply({"kind": "replace_data", "id": ids[k], "data": {"type": "cel", "number": CEL}})
    skin = [sd for sd in face if abs(sd["color"]["rgba"][0] - 0xf8 / 255) < 1e-3 and abs(sd["color"]["rgba"][1] - 0xd3 / 255) < 1e-3]
    rest = [sd for sd in face if sd not in skin]
    d.call("FILL", {"cut": ids["cut"], "frame": F0, "layer": pele, "seeds": skin, "gap": gap})
    if rest:
        d.call("FILL", {"cut": ids["cut"], "frame": F0, "layer": rosto, "seeds": rest, "gap": gap})
    d.call("GRADIENT", {"cut": ids["cut"], "frame": F0, "layer": luz, "kind": "linear", "from": [0, 120], "to": [0, 900],
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
