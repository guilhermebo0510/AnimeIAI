"""Corte D: partes que mudam de um desenho para outro (8 desenhos) — sobrancelhas (h3 e começo de l1, desenhadas por cima
da franja), boca e gotas de suor. Cada camada dinâmica ganha um cel por desenho e uma trilha no timesheet; o resto do
quadro fica no cel 1 de sempre. Estimativas por desenho lidas pelo agente (recortes e varreduras), refinadas pelas mesmas
medições do desenho 1. uso: python d218_dyn.py [desenhos...]  (padrão: todos)"""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node
import d218_hair as H

if "franja_h3" in H.INK:
    H.INK.setdefault("franja_h3_r", H.INK["franja_h3"])

FRAMES = {1: 0, 2: 6, 3: 9, 4: 14, 5: 54, 6: 57, 7: 60, 8: 63}  # desenho → primeiro quadro (0-based)
# mudanças de exposição: quadro → desenho (ciclo 6/7/8 em 3s até o fim)
EXPOSURE = {0: 1, 6: 2, 9: 3, 14: 4, 54: 5, 57: 6, 60: 7, 63: 8, 66: 6, 69: 7, 72: 8, 75: 6, 78: 7, 81: 8, 84: 6}


def ref_of(k):
    return rf"C:\AnimeIAI\calibracao\ref\cut_218\frames\f_{FRAMES[k] + 1:04d}.png"


# ---------------------------------------------------------------------------------------------- dados por desenho
D1 = {
    # nomes iguais aos da tabela do cabelo: herdam a cor de tinta medida (d218_ink.json) e a largura fixa
    "brows": {"franja_h3": [(700, 170), (750, 147), (780, 132), (825, 136)],
              "franja_l1": [(1130, 99), (1175, 85), (1205, 90), (1225, 100), (1245, 108), (1262, 117)]},
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
DATA = {k: D1 for k in FRAMES}  # desenhos ainda não relidos usam os dados do 1
# desenho 2 (q6): só as sobrancelhas descem ~15–20 px (varredura de colunas da tinta, runs_cmp.py)
DATA[2] = dict(D1, brows={"franja_h3": [(696, 191), (710, 186), (720, 182), (730, 178), (740, 173), (750, 169), (770, 161), (790, 155),
                                         (800, 152), (810, 151), (820, 151), (832, 150)],
                          "franja_l1": [(1122, 117), (1130, 114.5), (1150, 111.5), (1170, 111.5), (1190, 112.5), (1220, 118), (1240, 122.5),
                                        (1258, 127)]})

# desenho 3 (q9): sobrancelhas bem mais baixas, boca maior (trapézio) e uma gota nova à direita, cortada pelo rosto
DATA[3] = dict(D1,
    brows={"franja_h3": [(698, 202), (710, 199.5), (720, 197.5), (730, 196), (740, 194), (750, 192), (760, 190.5), (780, 186), (800, 180.5),
                         (810, 176), (820, 171), (832, 167)],
           "franja_l1": [(1118, 133), (1130, 136), (1150, 138.5), (1170, 139), (1190, 138), (1220, 135), (1240, 133.5), (1258, 132)]},
    mouth_out=[(1093, 676), (1127, 676), (1132, 680), (1135, 684), (1137, 689), (1140, 697), (1144, 705), (1146, 713), (1148, 721), (1149.5, 729),
               (1149.5, 733), (1146, 737), (1134, 741), (1120, 745), (1096, 749), (1076, 753), (1049, 757), (1019, 761), (990, 764), (983, 757),
               (981, 749), (981.5, 741), (983, 733), (986, 729), (989, 721), (991, 713), (995, 705), (998, 701), (999, 697), (1003, 693),
               (1011, 689), (1026, 685), (1057, 681)],
    mouth_in=[(1123, 688.5), (1086, 688.5), (1056, 692.5), (1024, 696.5), (1013, 700.5), (1010, 704.5), (1008, 708.5), (1006, 712.5), (1004, 716.5),
              (1003, 720.5), (1001, 724.5), (1000, 728.5), (998, 732.5), (997, 736.5), (996, 740.5), (996, 744.5), (997, 748.5), (1006, 749),
              (1047, 745.5), (1074, 741.5), (1092, 737.5), (1120, 733.5), (1135, 729.5), (1138, 725), (1137, 721), (1135, 717), (1135, 713),
              (1133, 709), (1133, 705), (1131, 701), (1130, 697), (1127, 693)],
    drops=D1["drops"] + [
        {"poly": [(1347, 569), (1353, 575), (1353, 595), (1351, 599), (1349, 603), (1351, 606), (1356, 609), (1363, 609), (1367, 606),
                  (1370, 607), (1369, 612), (1367, 616), (1360, 620), (1350, 621), (1343, 619), (1339, 615), (1336, 611), (1334, 607),
                  (1332, 603), (1332, 591), (1333, 587), (1334, 583), (1336, 579), (1339, 575), (1344, 571)],
         "inner": [(1353, 598), (1362, 599), (1365, 603), (1363, 608), (1355, 608), (1351, 604)],
         "shadow": [(1331, 611), (1337, 615), (1345, 619), (1359, 623), (1358, 627), (1348, 631), (1340, 629), (1333, 623)]}])

# desenho 4 (q14–53): sobrancelhas e gota direita iguais às do 3, gota esquerda igual à do 1, boca menor
DATA[4] = dict(DATA[3],
    mouth_out=[(1096, 684), (1128, 684), (1132, 688), (1134, 692), (1136, 697), (1138, 701), (1139, 705), (1140, 709), (1141, 713), (1142, 717),
               (1142.5, 723), (1141, 729), (1134, 733), (1117, 737), (1095, 741), (1065, 745), (1036, 749), (1000, 751), (992, 749), (988, 745),
               (987, 741), (988, 737), (988, 733), (990, 729), (991, 725), (992, 721), (994, 717), (996, 713), (998, 709), (1000, 705),
               (1004, 701), (1010, 697), (1035, 693), (1062, 689)],
    mouth_in=[(1125, 696.5), (1092, 696.5), (1061, 700.5), (1028, 704.5), (1012, 708.5), (1009, 712.5), (1007, 716.5), (1005, 720.5),
              (1004, 724.5), (1003, 728.5), (1001, 732.5), (1001, 736.5), (1033, 737.5), (1069, 733.5), (1095, 729.5), (1122, 725.5),
              (1131, 721), (1131, 717), (1131, 713), (1129, 709), (1128, 705), (1126, 701)])

# desenho 5 (q54–56): boca aberta (contorno sólido, palato #8f464c, língua rosa), sobrancelhas inclinadas; gotas como no 4
DATA[5] = dict(DATA[4],
    brow_w={"franja_h3": 11.0, "franja_h3_r": 16.0},  # a sobrancelha engrossa para a direita
    brows={"franja_h3": [(706, 206), (720, 206), (740, 208), (760, 212), (780, 217), (795, 220)],
           "franja_h3_r": [(790, 220), (805, 223), (820, 225), (835, 225.5), (850, 225), (865, 222.5), (880, 224.5), (895, 224)],
           "franja_l1": [(1100, 183), (1112, 187), (1125, 186), (1140, 184), (1150, 181.5), (1160, 180), (1170, 174.5), (1185, 168), (1200, 163),
                         (1210, 160), (1220, 155.5), (1230, 152), (1240, 148), (1250, 143.5), (1262, 137)]},
    mouth_solid=True, mouth_in=None,
    mouth_out=[(1050, 634), (1073, 634), (1084, 640), (1092, 645), (1098, 650), (1107, 660), (1114, 670), (1121, 680), (1127, 690), (1133, 700),
               (1138, 710), (1143, 720), (1146, 730), (1149, 740), (1151, 750), (1153.5, 757), (1152, 765), (1149, 770), (1144, 775), (1135, 780),
               (1126, 785), (1116, 790), (1108, 793), (1095, 796), (1086, 799), (1071, 802), (1039, 805), (1026, 805), (998, 802), (992, 799),
               (989, 796), (985, 790), (983, 785), (982, 780), (981, 770), (981.5, 760), (983, 755), (984, 750), (986, 740), (988, 735), (990, 730),
               (993, 720), (997, 710), (1001, 700), (1005, 690), (1009, 680), (1016, 670), (1023, 660), (1031, 650), (1042, 640)],
    mouth_extra=[("#8f464c", [(1047, 651), (1075, 651), (1082, 655), (1087, 659), (1091, 663), (1095, 667), (1098, 671), (1099, 676), (1089, 676),
                              (1070, 674), (1052, 676), (1036, 680), (1026, 684), (1029, 676), (1032, 672), (1034, 668), (1037, 664), (1040, 660),
                              (1043, 656)]),
                 ("#ea7582", [(1044, 699), (1075, 696), (1105, 699), (1117, 704), (1122, 709), (1124, 714), (1127, 719), (1130, 724), (1131, 729),
                              (1133, 734), (1135, 739), (1136, 744), (1136.5, 750), (1135, 755), (1131, 760), (1125, 765), (1117, 770), (1104, 775),
                              (1089, 780), (1074, 786), (1015, 786), (1005, 780), (1001, 775), (1000, 770), (1000, 765), (1001, 760), (1002, 755),
                              (1002, 750), (1004, 745), (1005, 740), (1006, 735), (1008, 730), (1009, 725), (1011, 720), (1013, 715), (1016, 710),
                              (1026, 705)])])

WHITE = [0xf6 / 255, 0xf2 / 255, 0xf6 / 255, 1.0]
PINK_SHADOW = [0xd7 / 255, 0x78 / 255, 0x82 / 255, 1.0]
MOUTH_PINK = [0xea / 255, 0x75 / 255, 0x82 / 255, 1.0]


def area(poly):
    return sum(poly[i][0] * poly[(i + 1) % len(poly)][1] - poly[(i + 1) % len(poly)][0] * poly[i][1] for i in range(len(poly))) / 2


def layers():
    out = {}
    for key, name, kind in [("sobrancelha", "Sobrancelhas", "vector"), ("boca", "Boca", "raster"), ("gota", "Gotas", "vector"),
                            ("gota_r", "Gotas (sombra)", "raster"), ("gota_r2", "Gotas (miolo)", "raster")]:
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
    saved = dict(H.FIXED_W)
    H.FIXED_W.update(data.get("brow_w", {}))  # largura própria deste desenho (lida nos recortes)
    try:
        curves, _ = H.measured_curves(data["brows"], ref=ref)
    finally:
        H.FIXED_W.clear()
        H.FIXED_W.update(saved)
    d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": fr, "layer": lay["sobrancelha"], "curves": curves})
    # boca: anel de tinta com furo + rosa por dentro (+ extras da boca aberta)
    outer = [list(p) for p in data["mouth_out"]]
    inner = [list(p) for p in data.get("mouth_in") or []]
    # cor da tinta da boca: amostras 5 px para dentro do contorno externo (rumo ao centro), só as com cara de tinta
    cx = sum(p[0] for p in outer) / len(outer)
    cy = sum(p[1] for p in outer) / len(outer)
    probe_pts = []
    for p in meas.densify(data["mouth_out"], 12, closed=True):
        vx, vy = cx - p[0], cy - p[1]
        n = (vx * vx + vy * vy) ** 0.5 or 1.0
        probe_pts.append([round(p[0] + vx / n * 5), round(p[1] + vy / n * 5)])
    pr = meas.first_list(d.call("PROBE_COLOR", {"reference": ref, "points": probe_pts, "radius": 1}))
    pr = [p for p in pr if sum((p["rgb"][k] * 255 - (92, 46, 46)[k]) ** 2 for k in range(3)) ** 0.5 < 60] or pr
    rgb = sorted([p["rgb"] for p in pr], key=sum)
    ink = list(rgb[len(rgb) // 2]) + [1.0]
    if data.get("mouth_solid"):
        # boca aberta: forma externa inteira em tinta; palato e língua por cima (em "mouth_extra")
        d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": fr, "layer": lay["boca"], "shapes": [{"points": outer, "geometry": "polyline"}],
                              "color": {"type": "direct", "rgba": ink}})
        for hexc, poly in data["mouth_extra"]:
            d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": fr, "layer": lay["boca"],
                                  "shapes": [{"points": [list(p) for p in poly], "geometry": "polyline"}],
                                  "color": {"type": "direct", "rgba": [int(hexc[i:i + 2], 16) / 255 for i in (1, 3, 5)] + [1.0]}})
        draw_drops(k, lay, data, fr, ref)
        return
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
    draw_drops(k, lay, data, fr, ref)


def draw_drops(k, lay, data, fr, ref):
    gc = []
    for drop in data["drops"]:
        d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": fr, "layer": lay["gota_r"], "shapes": [{"points": [list(p) for p in drop["shadow"]],
                                                                                                    "geometry": "polyline"}],
                              "color": {"type": "direct", "rgba": PINK_SHADOW}})
        if drop.get("u"):
            gc.append({"points": [list(p) for p in drop["u"]], "width": drop.get("w", 17.0), "tip": "marker", "geometry": "spline",
                       "color": {"type": "direct", "rgba": WHITE}})
        if drop.get("dot"):
            gc.append({"points": [list(p) for p in drop["dot"]], "width": drop.get("dot_w", 8.0), "tip": "marker", "geometry": "polyline",
                       "color": {"type": "direct", "rgba": WHITE}})
    if gc:
        d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": fr, "layer": lay["gota"], "curves": gc})
    for drop in data["drops"]:
        if drop.get("poly"):  # gota em crescente desenhada como forma branca (antes do miolo rosa, na mesma camada)
            d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": fr, "layer": lay["gota_r2"], "shapes": [{"points": [list(p) for p in drop["poly"]],
                                                                                                         "geometry": "polyline"}],
                                  "color": {"type": "direct", "rgba": WHITE}})
        if drop.get("inner"):  # miolo rosado da gota (por cima do branco): camada `gota_r2`
            d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": fr, "layer": lay["gota_r2"], "shapes": [{"points": [list(p) for p in drop["inner"]],
                                                                                                         "geometry": "polyline"}],
                                  "color": {"type": "direct", "rgba": PINK_SHADOW}})


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
