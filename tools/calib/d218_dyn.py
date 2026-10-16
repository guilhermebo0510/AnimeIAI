"""Corte D: partes que mudam de um desenho para outro (8 desenhos) — sobrancelhas (h3 e começo de l1, desenhadas por cima
da franja), boca e gotas de suor. Cada camada dinâmica ganha um cel por desenho e uma trilha no timesheet; o resto do
quadro fica no cel 1 de sempre. Estimativas por desenho lidas pelo agente (recortes e varreduras), refinadas pelas mesmas
medições do desenho 1. uso: python d218_dyn.py [desenhos...]  (padrão: todos)"""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node
import d218_hair as H

FRAMES = {1: 0, 2: 6, 3: 9, 4: 14, 5: 54, 6: 57, 7: 60, 8: 63}  # desenho → primeiro quadro (0-based)
# mudanças de exposição: quadro → desenho (ciclo 6/7/8 em 3s até o fim)
EXPOSURE = {0: 1, 6: 2, 9: 3, 14: 4, 54: 5, 57: 6, 60: 7, 63: 8, 66: 6, 69: 7, 72: 8, 75: 6, 78: 7, 81: 8, 84: 6}


def ref_of(k):
    return rf"C:\AnimeIAI\calibracao\ref\cut_218\frames\f_{FRAMES[k] + 1:04d}.png"


# ---------------------------------------------------------------------------------------------- dados por desenho
D1 = {
    # nomes iguais aos da tabela do cabelo: herdam a cor de tinta medida (d218_ink.json) e a largura fixa
    "brows": {"franja_h3": [(700, 170), (750, 147), (780, 132), (825, 136)],
              "franja_l1": [(1130, 99), (1175, 85), (1225, 95), (1262, 117)]},
    "mouth_out": [(1014, 695), (1038, 691), (1071, 687), (1107, 683), (1121, 683), (1131, 687), (1136, 695), (1139, 704), (1141, 712),
                  (1142.5, 720), (1141.5, 728), (1136.5, 732), (1122.5, 736), (1102.5, 740), (1074.5, 744), (1047.5, 748), (1012, 752),
                  (1004, 752.5), (989, 748), (987, 740), (988, 736), (990, 728), (993, 720), (996, 712), (1001, 704), (1004, 700)],
    "mouth_in": [(1124, 695.5), (1100, 695.5), (1068, 699.5), (1034, 703.5), (1014, 707.5), (1010, 711.5), (1007, 715.5), (1006, 719.5),
                 (1004, 723.5), (1003, 727.5), (1002, 731.5), (1001, 735.5), (1042, 736.5), (1078, 732.5), (1104, 728.5), (1127, 724.5),
                 (1131, 720), (1131, 716), (1130, 712), (1129, 708), (1127, 704), (1126, 700)],
    "mouth_extra": [],  # [(cor, polígono)] por cima do rosa (garganta escura da boca aberta etc.)
    "drops": [  # gota em "U": (pontos, largura) + bolinha interna + sombra rosa embaixo
        {"u": [(692, 712), (689, 718), (687, 728), (688, 740), (695, 750), (705, 753), (715, 751), (722, 744), (724, 734), (723, 730)],
         "dot": [(702, 731), (702, 737)], "shadow": [(690, 752), (725, 752), (726, 760), (720, 768), (708, 771), (696, 769), (690, 762)]},
    ],
}
DATA = {k: D1 for k in FRAMES}  # placeholder: desenhos 2–8 ainda com os dados do 1 até serem relidos

WHITE = [0xf6 / 255, 0xf2 / 255, 0xf6 / 255, 1.0]
PINK_SHADOW = [0xd7 / 255, 0x78 / 255, 0x82 / 255, 1.0]
MOUTH_PINK = [0xea / 255, 0x75 / 255, 0x82 / 255, 1.0]


def area(poly):
    return sum(poly[i][0] * poly[(i + 1) % len(poly)][1] - poly[(i + 1) % len(poly)][0] * poly[i][1] for i in range(len(poly))) / 2


def layers():
    out = {}
    for key, name, kind in [("sobrancelha", "Sobrancelhas", "vector"), ("boca", "Boca", "raster"), ("gota", "Gotas", "vector"),
                            ("gota_r", "Gotas (sombra)", "raster")]:
        lay = node(key, name, {"type": "layer", "kind": kind}, ids["cut"])
        for k in FRAMES:
            node(f"{key}_c{k}", str(k), {"type": "cel", "number": k}, lay)
        out[key] = lay
    reorder()
    return out


def set_track(key, layer):
    """Trilha do timesheet da camada: remove as que já expõem essa camada e cria a nova com o mapa de exposição."""
    cut = d.query({"kind": "node", "id": ids["cut"]})
    for t in cut["data"].get("xsheet", {}).get("tracks", []):
        if t.get("layer") == layer:
            d.apply({"kind": "remove_track", "cut": ids["cut"], "track": t["id"]})
    cells = {str(f): {"type": "cel", "number": k} for f, k in EXPOSURE.items()}
    return d.apply({"kind": "add_track", "cut": ids["cut"], "track": {"id": "", "name": key.upper()[:6], "kind": "drawing", "layer": layer,
                                                                      "cells": cells}})


def draw_drawing(k, lay):
    ref, data, fr = ref_of(k), DATA[k], FRAMES[k]
    for key in lay:
        d.apply({"kind": "replace_data", "id": ids[f"{key}_c{k}"], "data": {"type": "cel", "number": k}})
    # sobrancelhas: mesma medição das linhas do cabelo, na referência deste desenho
    curves, _ = H.measured_curves(data["brows"], ref=ref)
    d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": fr, "layer": lay["sobrancelha"], "curves": curves})
    # boca: anel de tinta com furo + rosa por dentro (+ extras da boca aberta)
    outer, inner = [list(p) for p in data["mouth_out"]], [list(p) for p in data["mouth_in"]]
    pr = meas.first_list(d.call("PROBE_COLOR", {"reference": ref, "points": [[round((a[0] + b[0]) / 2), round((a[1] + b[1]) / 2)]
                                                                            for a, b in zip(meas.densify(data["mouth_out"], 20, closed=True),
                                                                                            meas.densify(data["mouth_in"], 20, closed=True))],
                                                 "radius": 1}))
    rgb = sorted([p["rgb"] for p in pr], key=sum)
    ink = list(rgb[len(rgb) // 2]) + [1.0]
    d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": fr, "layer": lay["boca"], "shapes": [{"points": inner, "geometry": "polyline"}],
                          "color": {"type": "direct", "rgba": MOUTH_PINK}})
    for hexc, poly in data["mouth_extra"]:
        d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": fr, "layer": lay["boca"], "shapes": [{"points": [list(p) for p in poly], "geometry": "polyline"}],
                              "color": {"type": "direct", "rgba": [int(hexc[i:i + 2], 16) / 255 for i in (1, 3, 5)] + [1.0]}})
    # furo: o contorno interno precisa girar ao contrário do externo (área com sinal oposto)
    hole = inner if area(inner) * area(outer) < 0 else list(reversed(inner))
    d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": fr, "layer": lay["boca"],
                          "shapes": [{"points": outer, "geometry": "polyline"}, {"points": hole, "geometry": "polyline"}],
                          "color": {"type": "direct", "rgba": ink}})
    # gotas
    gc = []
    for drop in data["drops"]:
        d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": fr, "layer": lay["gota_r"], "shapes": [{"points": [list(p) for p in drop["shadow"]],
                                                                                                    "geometry": "polyline"}],
                              "color": {"type": "direct", "rgba": PINK_SHADOW}})
        gc.append({"points": [list(p) for p in drop["u"]], "width": drop.get("w", 17.0), "tip": "marker", "geometry": "spline",
                   "color": {"type": "direct", "rgba": WHITE}})
        if drop.get("dot"):
            gc.append({"points": [list(p) for p in drop["dot"]], "width": drop.get("dot_w", 8.0), "tip": "marker", "geometry": "polyline",
                       "color": {"type": "direct", "rgba": WHITE}})
    if gc:
        d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": fr, "layer": lay["gota"], "curves": gc})


def run(which=None):
    lay = layers()
    for key, l in lay.items():
        set_track(key, l)
    for k in (which or list(FRAMES)):
        draw_drawing(k, lay)
    return lay


if __name__ == "__main__":
    ks = [int(a) for a in sys.argv[1:] if a.isdigit()]
    run(ks or None)
    print("ok")
