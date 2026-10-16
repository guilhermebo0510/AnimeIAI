"""Corte D, quadro 0: mechas do cabelo. Polilinhas ESTIMADAS pelo agente (lidas no mapa de proximidade à cor da linha,
grid_crop --near 5c2e2e) e refinadas por MEASURE_LINE (decisão 22). Desenha na camada vetorial `linha` sem apagar o rosto."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import d, ids, REF0, draw, render

MIN_W = 11.0
# larguras lidas nos perfis da referência onde um lado da linha funde numa massa escura (a medição não acha a 2ª borda)
FIXED_W = {"queixo": 14.0, "tras_b": 9.0, "tras_c": 9.0, "tras_d1": 9.0, "tras_d2": 9.0, "tras_e": 9.0}
ANCHOR = "--sem-ancora" not in sys.argv
LINE = {"type": "direct", "rgba": [0x5c / 255, 0x2e / 255, 0x2e / 255, 1.0]}

HAIR = {
    "contorno_esq": [(290, 140), (270, 210), (250, 280), (220, 340), (211, 360), (225, 390), (250, 425), (237, 450), (225, 490),
                     (210, 530), (197, 580), (205, 605), (230, 630), (270, 660), (320, 688), (372, 705)],
    "cacho_esq": [(240, 660), (225, 710), (210, 750), (200, 790), (180, 805), (200, 830), (240, 855), (300, 880), (330, 895), (350, 905)],
    "ramo_c": [(250, 455), (290, 490), (330, 515), (362, 535)],
    "ramo_d": [(300, 245), (315, 290), (335, 320), (355, 350), (360, 365)],
    "mecha_e": [(425, 0), (410, 60), (400, 110), (380, 200), (368, 275), (360, 350), (362, 450), (368, 500), (385, 550), (390, 625),
                (390, 700), (375, 750), (360, 825), (350, 875), (335, 900), (325, 925)],
    "mecha_s": [(485, 570), (480, 625), (470, 675), (460, 715), (440, 760), (410, 800), (375, 840), (365, 850)],
    "mecha_g": [(585, 0), (565, 65), (555, 125), (545, 200), (537, 300), (535, 400), (537, 500), (545, 575), (545, 665), (535, 750),
                (530, 800), (520, 825), (500, 860), (475, 890), (450, 915), (435, 940), (432, 960)],
    "ramo_g": [(620, 50), (610, 100), (585, 165), (565, 220), (550, 260)],
    "base_g": [(550, 790), (575, 800), (625, 800)],
    "franja_h1": [(550, 465), (575, 425), (600, 390), (625, 345), (645, 300), (665, 250), (690, 200), (700, 170), (715, 110), (725, 70), (722, 0)],
    "franja_h2": [(725, 70), (750, 125), (775, 175), (800, 225), (850, 280), (900, 320), (942, 345)],
    "franja_h3": [(700, 170), (750, 147), (780, 132), (825, 136)],
    "franja_i1": [(885, 200), (895, 250), (915, 300), (945, 345)],
    "franja_i2": [(910, 255), (950, 290), (1000, 318), (1050, 350), (1097, 352)],
    "franja_k1": [(957, -6), (965, 30), (980, 80), (1000, 125), (1025, 165), (1040, 200), (1060, 235), (1080, 270), (1100, 300), (1127, 332)],
    "franja_k2": [(1064, -6), (1065, 30), (1058, 75), (1050, 115), (1030, 160), (1025, 170)],
    "franja_k3": [(1095, 280), (1115, 240), (1125, 200), (1130, 175), (1137, 200), (1142, 240), (1147, 280), (1165, 240), (1185, 195),
                  (1205, 140), (1220, 85), (1225, 30), (1228, -6)],
    "franja_l1": [(1125, 100), (1175, 85), (1225, 95), (1275, 125), (1325, 165), (1375, 200), (1415, 215), (1420, 230)],
    "franja_l2": [(1295, 75), (1340, 110), (1390, 140), (1410, 165)],
    "mecha_m": [(1356, -6), (1365, 25), (1385, 75), (1405, 125), (1420, 175), (1430, 225), (1437, 275), (1442, 350), (1445, 425), (1435, 475),
                (1430, 500), (1410, 550), (1385, 600), (1360, 650), (1350, 685), (1360, 725), (1385, 755), (1410, 772), (1445, 778)],
    "mecha_m2": [(1450, 465), (1475, 400), (1500, 350), (1515, 290), (1520, 250), (1510, 225)],
    "mecha_n": [(1575, 5), (1600, 75), (1610, 125), (1625, 200), (1632, 275), (1635, 350), (1630, 450), (1605, 525), (1590, 575),
                (1570, 625), (1555, 650)],
    "mecha_o": [(1515, 500), (1517, 575), (1540, 625), (1555, 645)],
    "cacho_y": [(1555, 650), (1585, 670), (1615, 700), (1640, 740), (1645, 775), (1635, 800), (1600, 825), (1565, 845), (1550, 860)],
    "contorno_n2": [(1615, 107), (1675, 135), (1720, 185), (1732, 210), (1722, 260), (1705, 310), (1675, 365), (1635, 395)],
    "contorno_n3": [(1720, 280), (1755, 325), (1790, 375), (1775, 425), (1760, 480), (1750, 500), (1700, 540), (1640, 580), (1590, 605)],
    "contorno_n4": [(1750, 495), (1800, 540), (1830, 570), (1835, 600), (1825, 640), (1800, 710), (1750, 780), (1675, 815), (1640, 800)],
    "contorno_n5": [(1830, 640), (1875, 685), (1915, 720)],
    "cacho_x": [(1432, 715), (1450, 740), (1465, 775), (1465, 810), (1450, 850), (1425, 880), (1390, 900), (1350, 910), (1345, 920),
                (1365, 930), (1400, 940), (1450, 945), (1500, 930), (1540, 890), (1560, 850), (1565, 800), (1570, 780)],
    "mecha_yy": [(1640, 820), (1673, 847), (1707, 887), (1727, 920), (1740, 960), (1750, 1000), (1760, 1040), (1767, 1084)],
    "cacho_dir_c": [(1750, 917), (1790, 897), (1827, 877), (1857, 850), (1880, 820), (1893, 793), (1897, 780)],
    "cacho_dir_d": [(1890, 813), (1905, 830), (1922, 849)],
    "cacho_dir_e": [(1847, 883), (1860, 900), (1857, 930), (1840, 960), (1813, 993), (1797, 1013), (1800, 1027), (1840, 1030), (1873, 1020),
                    (1900, 1003), (1922, 978)],
    "cacho_dir_f": [(1797, 912), (1825, 927), (1837, 947), (1830, 975), (1813, 995)],
    "laco": [(176, 0), (179, 22), (188, 45), (220, 76), (250, 102), (280, 122), (300, 134)],
    "no_c1": [(350, 35), (372, 40), (395, 47)],
    "no_c2": [(342, 85), (363, 97), (385, 110)],
    "no_c3": [(332, 108), (353, 121), (375, 135)],
    "mecha_f": [(352, 0), (338, 60), (318, 115), (300, 138)],
    "borda_inf_esq": [(190, 800), (200, 860), (207, 930), (212, 1000), (216, 1078)],
    "tras_b": [(600, 802), (650, 838), (700, 868), (765, 890), (830, 902)],
    "tras_c": [(616, 802), (615, 880), (614, 960), (612, 1080)],
    "tras_d1": [(460, 910), (510, 912), (560, 916), (600, 925)],
    "tras_d2": [(460, 912), (452, 945), (452, 985), (472, 1015), (502, 1042), (552, 1080)],
    "tras_e": [(842, 950), (838, 1010), (832, 1080)],
    "queixo": [(697, 855), (760, 895), (840, 935), (960, 962), (1080, 955), (1190, 928), (1250, 905), (1300, 875), (1345, 840), (1380, 805),
               (1405, 775)],
}
# trechos onde duas linhas correm coladas (a medição pula de uma para a outra): desenhados direto da leitura ampliada
DIRECT = {"queixo_a": ([(549, 560), (550, 602), (566, 655), (586, 703), (612, 752), (644, 804), (697, 855)], 14.0)}
CLOSED = {"boca": [(992, 703), (1030, 694), (1080, 689), (1130, 688), (1140, 700), (1142, 722), (1110, 733), (1060, 740), (1000, 748), (990, 726)]}


def measured_curves(table, step=8, closed=False):
    curves, log = [], {}
    for name, poly in table.items():
        pts, ws = meas.measure_line(d, REF0, meas.densify(poly, step, closed=closed), closed=closed, line_color="#5c2e2e",
                                    passes=((14.0, 0.08), (9.0, 0.08)), smooth=3)
        c = meas.curve_from_measure(pts, ws, LINE, closed=closed)
        # linhas do chibi são quase uniformes (11–15 px); onde a tinta funde numa massa escura a meia altura subestima:
        # piso de 11 px e variação do perfil limitada (leitura do agente nos recortes ampliados)
        c["width"] = max(c["width"], MIN_W)
        if name in FIXED_W:
            c["width"] = FIXED_W[name]
            ws = [FIXED_W[name]] * len(ws)
        if ANCHOR:
            # refino pelas bordas (lado da pele/fundo é nítido): centro e largura locais
            ws = [max(w or 0, MIN_W) for w in ws]
            for search in (10.0, 4.0):
                pts, ws2 = meas.anchor_line(d, REF0, pts, ws, c["width"], closed=closed, search=search)
                ws = ws if name in FIXED_W else ws2
            c = meas.curve_from_measure(pts, ws, LINE, closed=closed)
            c["width"] = max(c["width"], MIN_W - 1)
        c["profile"] = [[t, max(0.7, min(1.3, w))] for t, w in c["profile"]]
        curves.append(c)
        log[name] = c["width"]
    return curves, log


# linhas de fechamento do balde (camada vetorial OCULTA `fecho`, em line_refs da camada de cor): fecham regiões
# onde as pontas das mechas não se tocam, sem aparecer no quadro final
FECHO = [
    [(551, 466), (536, 469)],          # mecha g ↔ franja h1 (lado esquerdo do rosto)
    [(1099, 351), (1128, 334)],        # pontas da franja i2 ↔ k1
    [(372, 705), (390, 702)],          # contorno esquerdo ↔ mecha e
    [(1915, 720), (1925, 730)],        # contorno n5 ↔ borda direita
    [(241, 662), (247, 645)],          # cacho esquerdo ↔ contorno esquerdo
    [(737, 95), (800, 70), (880, 50), (965, 40), (1065, 35), (1150, 35), (1228, 35)],  # topo da franja (região do degradê claro)
]


def fecho():
    from d218 import node
    f = node("fecho", "Fecho do balde", {"type": "layer", "kind": "vector"}, ids["cut"])
    node("fecho_c1", "1", {"type": "cel", "number": 1}, f)
    d.apply({"kind": "set_props", "id": f, "data": {"visible": False}})
    d.apply({"kind": "set_props", "id": ids["cor"], "data": {"line_refs": [ids["linha"], ids["linha_massa"], f]}})
    d.apply({"kind": "replace_data", "id": ids["fecho_c1"], "data": {"type": "cel", "number": 1}})
    curves = [{"points": [list(p) for p in c], "width": 2.0, "tip": "marker", "geometry": "polyline", "color": LINE} for c in FECHO]
    return d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": 0, "layer": f, "curves": curves})


if __name__ == "__main__":
    print(fecho().get("description"))
    if "--so-fecho" in sys.argv:
        sys.exit()
    d.apply({"kind": "replace_data", "id": ids["linha_c1"], "data": {"type": "cel", "number": 1}})
    curves, log = measured_curves(HAIR)
    c2, l2 = measured_curves(CLOSED, closed=True)
    for name, (poly, w) in DIRECT.items():
        c2.append({"points": [list(p) for p in poly], "width": w, "tip": "marker", "geometry": "spline", "color": LINE})
    curves += c2; log.update(l2)
    json.dump(curves, open(os.path.join(os.path.dirname(__file__), "..", "..", "calibracao", "projetos", "d218_hair_curves.json"), "w"))
    print(json.dumps(log))
    print(draw(curves).get("description"))
    print(render("q0_cabelo"))
