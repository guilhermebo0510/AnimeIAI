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
    # sombra do queixo sobre o cabelo de trás: carmim escuro (a tinta só nas linhas), e o respiro rosado ao lado do queixo
    "sombra_queixo": ("#680922", [(615, 800), (660, 840), (720, 880), (800, 920), (880, 950), (945, 968), (940, 1080), (612, 1080)], False),
    "tinta_gola": ("#42191c", [(945, 975), (1000, 973), (1050, 971), (1100, 965), (1150, 957), (1200, 940), (1250, 918), (1300, 890),
                               (1345, 853), (1380, 818), (1405, 790), (1420, 830), (1415, 900), (1440, 960), (1430, 1080), (940, 1080)], False),
    "vestido": ("#eaf07c", [(1392, 1080), (1396, 1020), (1425, 995), (1450, 960), (1495, 925), (1530, 900), (1560, 860), (1600, 836),
                            (1630, 820), (1660, 840), (1705, 905), (1730, 960), (1750, 1020), (1758, 1080)], True),
    "vestido_meio": ("#eaf07c", [(1410, 1080), (1415, 1030), (1440, 1010), (1440, 1080)], True),
    "vestido_esq": ("#e3e872", [(895, 1080), (905, 1060), (925, 1045), (942, 1048), (935, 1065), (925, 1080)], True),
    "rosa": ("#d65aa6", [(1800, 917), (1810, 916), (1833, 933), (1835, 943), (1823, 967), (1810, 993), (1793, 1013), (1795, 1027),
                         (1807, 1032), (1850, 1032), (1880, 1020), (1900, 1000), (1920, 977), (1922, 1082), (1780, 1082), (1773, 1033),
                         (1767, 997), (1780, 960), (1790, 933)], True),
    "rosa_canto": ("#e070b4", [(1907, 767), (1920, 763), (1922, 825), (1893, 812), (1900, 790)], True),
    "gola_a": ("#f3f4dc", [(1400, 817), (1380, 847), (1377, 867), (1373, 893), (1340, 903), (1342, 927), (1323, 937), (1320, 917),
                           (1330, 880), (1360, 850)], True),
    "gola_b": ("#f3f4dc", [(1355, 940), (1390, 962), (1420, 965), (1413, 987), (1360, 993), (1333, 1010), (1313, 1043), (1323, 1047),
                           (1347, 1023), (1365, 1013), (1365, 1080), (1302, 1080), (1297, 1027), (1313, 987), (1337, 960)], True),
    "gola_esq": ("#b1be7e", [(962, 985), (976, 984), (981, 992), (984, 1004), (988, 1015), (992, 1025), (996, 1035), (999, 1045), (999, 1056),
                             (988, 1058), (966, 1052), (963, 1040), (960, 1025), (958, 1010), (958, 995)], False),
    "gola_esq_luz": ("#f0f4cc", [(958, 1016), (970, 1019), (985, 1024), (993, 1028), (996, 1035), (999, 1045), (999, 1056), (988, 1058),
                                 (966, 1052), (963, 1040), (960, 1025)], False),
    "gola_baixo": ("#f0f2dc", [(1125, 1065), (1145, 1060), (1162, 1080), (1120, 1080)], True),
    "gola_topo": ("#f3f4dc", [(1418, 787), (1430, 785), (1442, 793), (1418, 793)], False),
    "laco_1": ("#994021", [(995, 985), (1047, 987), (1080, 997), (1090, 1000), (1092, 1027), (1100, 1047), (1105, 1060), (1102, 1080),
                           (1026, 1080), (1023, 1057), (1010, 1027), (1003, 1007)], True),
    "laco_2": ("#994021", [(1160, 977), (1200, 950), (1243, 928), (1257, 927), (1260, 963), (1257, 997), (1263, 1023), (1273, 1040),
                           (1282, 1060), (1283, 1080), (1193, 1080), (1167, 1060), (1152, 1030), (1153, 1007)], True),
    "laco_topo": ("#994021", [(1270, 913), (1297, 902), (1290, 920), (1277, 935), (1272, 928)], True),
    "laco_3": ("#8e361a", [(1108, 1000), (1133, 992), (1135, 1007), (1135, 1047), (1127, 1048), (1117, 1033), (1110, 1013)], True),
    "laco_luz_1": ("#ca5f28", [(1015, 1035), (1060, 1040), (1098, 1045), (1103, 1060), (1102, 1080), (1026, 1080), (1021, 1057)], False),
    "laco_luz_2": ("#ca5f28", [(1157, 1043), (1208, 1030), (1258, 1010), (1270, 1032), (1283, 1065), (1283, 1080), (1193, 1080), (1165, 1062)], False),
    "lilas": ("#b995c5", [(1295, 937), (1302, 950), (1300, 975), (1293, 1000), (1285, 1015), (1277, 1005), (1276, 980), (1283, 955)], True),
    "lilas_luz": ("#dfe4ee", [(1302, 955), (1300, 975), (1293, 1000), (1285, 1015), (1277, 1005), (1275, 978), (1292, 964)], False),
    "gola_verde": ("#b1be7e", [(1410, 848), (1418, 832), (1428, 822), (1455, 825), (1440, 838), (1420, 848)], True),
    "gola_fresta": ("#8a8070", [(1419, 868), (1432, 860), (1436, 862), (1423, 870)], False),
    "rosa_peq": ("#e07a86", [(1105, 972), (1150, 962), (1145, 976), (1110, 982)], True),
}


def draw_shapes(measure=True):
    d.apply({"kind": "replace_data", "id": ids["baixo_c1"], "data": {"type": "cel", "number": 1}})
    out = {}
    for name, (color, poly, do_measure) in SHAPES.items():
        pts = meas.densify(poly, 3, closed=True)
        if measure and do_measure:
            pts = meas.measure_edge(d, REF0, pts, passes=((4.0, 0.03), (2.5, 0.03)))
        c = [int(color[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        r = d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": 0, "layer": ids["baixo"], "shapes": [{"points": pts, "geometry": "polyline"}],
                                  "color": {"type": "direct", "rgba": c + [1.0]}})
        out[name] = r.get("description")
    return out


if __name__ == "__main__":
    layers()
    print(json.dumps(draw_shapes(), ensure_ascii=False))
    print(render("q0_baixo"))
