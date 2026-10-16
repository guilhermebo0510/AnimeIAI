"""Corte D, quadro 0: parte de baixo (gola, laço, vestido, objeto rosa) — massas desenhadas como formas na camada raster
`baixo`, com contornos ESTIMADOS pelo agente e bordas medidas (MEASURE_EDGE). A massa escura sob o queixo é tinta."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node, REF0, render

INK = "#4a2022"


def layers():
    baixo = node("baixo", "Baixo (roupa)", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("baixo_c1", "1", {"type": "cel", "number": 1}, baixo)
    # ordem de baixo para cima: bg, cor, realce (recortado na cor), baixo, linha-massa, linha, satsuei, fecho
    reorder()
    return baixo


# polígonos estimados (leitura dos recortes ampliados low_r/low_m); a medição puxa cada ponto para a borda real
SHAPES = {
    # massa escura entre as mechas, sob o queixo (fundo das peças)
    # sombra projetada do queixo sobre o cabelo de trás (cor de tinta) com um respiro mais claro
    "sombra_queixo": ("#42191c", [(512, 762), (560, 770), (610, 790), (660, 840), (720, 890), (800, 925), (880, 950), (950, 966),
                                  (1000, 968), (1150, 948), (1300, 885), (1360, 840), (1405, 800), (1420, 830), (1415, 900), (1440, 960),
                                  (1430, 1080), (940, 1080), (900, 1040), (800, 1042), (700, 1046), (600, 1040), (530, 1032), (512, 990),
                                  (510, 900)], False),
    "furo_sombra": ("#640720", [(640, 915), (700, 912), (760, 925), (775, 950), (760, 975), (700, 982), (650, 970), (638, 945)], True),
    "vestido": ("#eaf07c", [(1392, 1080), (1396, 1020), (1425, 995), (1450, 960), (1495, 925), (1530, 900), (1560, 860), (1600, 836),
                            (1630, 820), (1660, 840), (1705, 905), (1730, 960), (1750, 1020), (1758, 1080)], True),
    "vestido_meio": ("#eaf07c", [(1410, 1080), (1415, 1030), (1440, 1010), (1440, 1080)], True),
    "vestido_esq": ("#e3e872", [(895, 1080), (905, 1060), (925, 1045), (942, 1048), (935, 1065), (925, 1080)], True),
    "rosa": ("#d65aa6", [(1800, 920), (1835, 935), (1832, 970), (1802, 1012), (1812, 1030), (1880, 1010), (1918, 972), (1918, 1078),
                         (1772, 1078), (1766, 990), (1782, 950)], True),
    "rosa_canto": ("#e070b4", [(1893, 800), (1905, 770), (1918, 762), (1918, 815), (1898, 812)], True),
    "gola_a": ("#f3f4dc", [(1400, 817), (1380, 880), (1340, 905), (1345, 930), (1325, 937), (1320, 925), (1330, 870), (1365, 845)], True),
    "gola_b": ("#f3f4dc", [(1355, 940), (1420, 968), (1410, 988), (1355, 995), (1320, 1015), (1310, 1040), (1320, 1048), (1365, 1015),
                           (1365, 1080), (1300, 1080), (1298, 1025), (1320, 970)], True),
    "gola_esq": ("#f0f2dc", [(960, 1015), (995, 1030), (1000, 1055), (965, 1052)], True),
    "gola_baixo": ("#f0f2dc", [(1125, 1065), (1145, 1060), (1162, 1080), (1120, 1080)], True),
    "gola_topo": ("#f3f4dc", [(1420, 785), (1440, 787), (1438, 793), (1420, 792)], False),
    "laco_1": ("#994021", [(998, 985), (1040, 990), (1080, 1000), (1100, 1030), (1098, 1070), (1095, 1080), (1020, 1080), (1005, 1040)], True),
    "laco_2": ("#994021", [(1158, 950), (1200, 925), (1250, 915), (1262, 940), (1280, 990), (1290, 1040), (1282, 1080), (1190, 1080),
                           (1162, 1040), (1152, 990)], True),
    "laco_3": ("#8e361a", [(1112, 995), (1128, 990), (1135, 1020), (1132, 1050), (1118, 1048), (1112, 1020)], True),
    "laco_luz_1": ("#ca5f28", [(1012, 1027), (1100, 1045), (1102, 1080), (1025, 1080)], True),
    "laco_luz_2": ("#ca5f28", [(1157, 1042), (1255, 1002), (1280, 1065), (1285, 1080), (1175, 1080)], True),
    "lilas": ("#d8d4f0", [(1300, 955), (1285, 1012), (1274, 985), (1285, 970)], True),
    "rosa_peq": ("#e07a86", [(1106, 962), (1150, 958), (1146, 974), (1112, 980)], True),
}


def draw_shapes(measure=True):
    d.apply({"kind": "replace_data", "id": ids["baixo_c1"], "data": {"type": "cel", "number": 1}})
    out = {}
    for name, (color, poly, do_measure) in SHAPES.items():
        pts = meas.densify(poly, 6, closed=True)
        if measure and do_measure:
            pts = meas.measure_edge(d, REF0, pts, passes=((5.0, 0.03), (3.0, 0.03)))
        c = [int(color[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        r = d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": 0, "layer": ids["baixo"], "shapes": [{"points": pts, "geometry": "polyline"}],
                                  "color": {"type": "direct", "rgba": c + [1.0]}})
        out[name] = r.get("description")
    return out


if __name__ == "__main__":
    layers()
    print(json.dumps(draw_shapes(), ensure_ascii=False))
    print(render("q0_baixo"))
