"""Corte D, quadro 0: mechas do cabelo. Polilinhas ESTIMADAS pelo agente (lidas no mapa de proximidade à cor da linha,
grid_crop --near 5c2e2e) e refinadas por MEASURE_LINE (decisão 22). Desenha na camada vetorial `linha` sem apagar o rosto."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import d, ids, REF0, draw, render, ck, CEL, F0, W, candidates, color_score

MIN_W = 11.0
# larguras lidas nos perfis da referência onde um lado da linha funde numa massa escura (a medição não acha a 2ª borda)
PIN_START = {"queixo"}
# linhas coladas a um realce: a borda realce→cabelo engana a ancoragem (pega a borda errada e alarga a linha)
NO_ANCHOR = {"mecha_e", "mecha_n"}
FIXED_W = {"franja_l1b": 22.0,  "queixo": 14.0, "tras_b": 13.0, "tras_c": 13.0, "tras_d1": 12.0, "tras_d2": 12.0, "tras_e": 12.0, "base_g": 14.0}
ANCHOR = "--sem-ancora" not in sys.argv
_INK_F = os.path.join(os.path.dirname(__file__), "..", "..", "calibracao", "projetos", "d218_ink.json")
INK = json.load(open(_INK_F)) if os.path.exists(_INK_F) else {}  # cor da tinta medida por linha (d218_inkcolors.py)
INK.update({"tras_c": "#451b1f", "tras_e": "#451b1f"})
if "franja_l1" in INK:
    INK.setdefault("franja_l1b", INK["franja_l1"])  # parte fixa de l1 (a parte da sobrancelha está em d218_dyn.py)


def ink_of(name, poly):
    """Cor medida da linha (no alto do quadro a tinta é mais clara de fato: menos contraste com o cabelo)."""
    ys = sorted(p[1] for p in poly)
    h = INK.get(name)
    if not h:
        return LINE
    return {"type": "direct", "rgba": [int(h[i:i + 2], 16) / 255 for i in (1, 3, 5)] + [1.0]}


LINE = {"type": "direct", "rgba": [0x5c / 255, 0x2e / 255, 0x2e / 255, 1.0]}

HAIR = {
    "contorno_n3b": [(1746, 494), (1736, 504), (1724, 520), (1712, 536), (1700, 548), (1640, 580), (1590, 605)],
    "contorno_n4b": [(1825, 640), (1800, 710), (1750, 780), (1675, 815), (1640, 800)],
    "cacho_esq": [(240, 660), (225, 710), (210, 750), (200, 790), (180, 805), (200, 830), (240, 855), (300, 880), (330, 895), (350, 905)],
    "ramo_c": [(250, 462), (290, 497), (330, 522), (364, 545)],
    "ramo_d": [(300, 245), (315, 290), (335, 320), (355, 350), (360, 365)],
    "mecha_e": [(425, 0), (410, 60), (400, 110), (380, 200), (368, 275), (360, 350), (362, 450), (368, 500), (385, 550), (390, 625),
                (390, 700), (375, 750), (360, 825), (350, 875), (335, 900), (325, 925)],
    "mecha_s": [(485, 570), (480, 625), (470, 675), (460, 715), (440, 760), (410, 800), (375, 840), (365, 850)],
    "mecha_g": [(585, 0), (565, 65), (555, 125), (545, 200), (537, 300), (535, 400), (537, 500), (545, 575), (545, 665), (535, 750),
                (530, 800), (520, 825), (500, 860), (475, 890), (450, 915), (435, 940), (432, 960)],
    "ramo_g": [(620, 50), (610, 100), (585, 165), (565, 220), (550, 260)],
    "franja_h1": [(550, 465), (575, 425), (600, 390), (625, 345), (645, 300), (665, 250), (690, 200), (700, 170), (715, 110), (725, 70), (722, 0)],
    "franja_h2": [(725, 70), (750, 125), (775, 175), (800, 225), (850, 280), (900, 320), (937, 341)],
    "franja_i1": [(885, 200), (895, 250), (915, 300), (940, 341)],
    "franja_i2": [(895, 212), (906, 228), (916, 242), (930, 258), (950, 290), (1000, 318), (1050, 350), (1090, 350)],
    "franja_entalhe": [(1068, 314), (1080, 318), (1090, 322)],
    "franja_bico": [(1070, 331), (1080, 338), (1088, 345)],
    "franja_k1": [(957, -6), (965, 30), (980, 80), (1000, 125), (1025, 165), (1040, 200), (1060, 235), (1080, 270), (1100, 300), (1120, 330)],
    "franja_k2": [(1064, -6), (1065, 30), (1058, 75), (1050, 115), (1030, 160), (1025, 170)],
    "franja_k3": [(1095, 280), (1115, 240), (1125, 200), (1130, 175), (1137, 200), (1142, 240), (1147, 280), (1165, 240), (1185, 195),
                  (1205, 140), (1220, 85), (1225, 30), (1228, -6)],
    # l1 até ~x 1255 é a sobrancelha direita (muda por desenho, d218_dyn.py); daqui para a direita é fixa
    "franja_l1b": [(1240, 104), (1255, 112), (1275, 125), (1325, 165), (1375, 200), (1415, 215), (1420, 230)],
    "franja_l2": [(1295, 75), (1340, 110), (1390, 140), (1410, 165)],
    "mecha_m": [(1356, -6), (1365, 25), (1385, 75), (1405, 125), (1420, 175), (1430, 225), (1437, 275), (1442, 350), (1445, 425), (1435, 475),
                (1430, 500), (1410, 550), (1385, 600), (1360, 650), (1350, 685), (1360, 725), (1385, 755), (1410, 772), (1445, 778)],
    "mecha_m2": [(1450, 465), (1475, 400), (1500, 350), (1515, 290), (1520, 250), (1510, 225)],
    "mecha_n": [(1575, 5), (1600, 75), (1610, 125), (1625, 200), (1632, 275), (1635, 350), (1630, 450), (1605, 525), (1590, 575),
                (1570, 625), (1555, 650)],
    "mecha_o": [(1515, 500), (1517, 575), (1540, 625), (1555, 645)],
    "cacho_y": [(1555, 650), (1585, 670), (1615, 700), (1640, 740), (1645, 775), (1635, 800), (1600, 825), (1565, 845), (1550, 860)],
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
    "tras_b": [(600, 802), (650, 838), (695, 862)],
    "tras_c": [(616, 802), (615, 880), (614, 960), (612, 1080)],
    "tras_d1": [(460, 910), (510, 912), (560, 916), (600, 925)],
    "tras_d2": [(460, 912), (452, 945), (452, 985), (472, 1015), (502, 1042), (552, 1080)],
    "tras_e": [(842, 950), (838, 1010), (832, 1080)],
}
# trechos onde duas linhas correm coladas (a medição pula de uma para a outra): desenhados direto da leitura ampliada
DIRECT = {
    # contornos externos que encostam no fundo: ancorados na borda do fundo (nítida), 16 px medidos nos perfis
    "contorno_esq": ([(290, 140), (270, 210), (250, 280), (220, 340), (211, 360), (225, 390), (250, 425), (237, 450), (225, 490), (210, 530),
                      (197, 580), (205, 605), (230, 630), (270, 660), (320, 688), (372, 705)], 16.0),
    "contorno_n2": ([(1610, 108), (1624, 117.5), (1640, 125.5), (1656, 134.5), (1672, 145), (1690, 159), (1705, 172), (1720, 185), (1732, 210), (1722, 260), (1705, 310), (1675, 365), (1635, 395)], 16.0),
    "contorno_n3a": ([(1720, 280), (1755, 325), (1790, 375), (1775, 425), (1760, 480), (1750, 500)], 16.0),
    "contorno_n4a": ([(1752, 494), (1764, 500), (1772, 504), (1785, 520), (1797, 536), (1810, 552), (1830, 570), (1835, 600), (1825, 640)], 16.0),
    "contorno_n5": ([(1830, 640), (1875, 685), (1915, 720)], 16.0),
    "alca": ([(1496, 984), (1490, 990), (1481, 1000), (1471, 1010), (1462, 1020), (1455, 1030), (1446, 1040), (1438, 1050),
              (1429, 1060), (1421, 1070), (1412, 1082)], 22.5),
    "borda_inf_esq": ([(183, 805), (192, 815), (200, 825), (212, 848), (221, 880), (221, 896), (219, 912), (218, 928), (217, 944), (214, 960), (213, 976), (211, 992),
                       (211, 1008), (209, 1024), (208, 1040), (213, 1060), (216, 1084)], 14.0),
    "base_g": ([(546, 793), (560, 799), (575, 804), (590, 807), (605, 808), (632, 809)], 14.0),
          "queixo": ([(549, 560), (550, 602), (566, 655), (586, 703), (612, 752), (644, 804), (697, 858), (760, 898), (840, 937),
                       (960, 964), (1080, 957), (1190, 930), (1250, 906), (1300, 876), (1345, 841), (1380, 806), (1405, 775)], 14.0)}
# lado da normal (−ty, tx) onde fica a pele: o queixo desce para a direita, a pele fica à esquerda da normal → −1
# larguras lidas nos perfis: o queixo engrossa (~20 px) ao lado do respiro rosado, ~14 px no resto
WIDTH_FN = {"contorno_n3a": lambda p, w: w + max(0.0, min(8.0, (p[1] - 400) * 0.1)),
            "contorno_n2": lambda p, w: 20.0 if p[0] < 1700 and p[1] < 175 else w,
            "borda_inf_esq": lambda p, w: max(5.0, min(14.0, 14.0 - (p[1] - 880) * 9.0 / 160.0)),
            "queixo": lambda p, w: 25.0 if (p[0] < 675 and 690 < p[1] < 830) else (20.0 if (p[0] < 675 and 650 < p[1] <= 690) else w)}
# redesenhos de tremor: linhas que o campo de deslocamento não leva para o lugar certo, relidas pelo agente no desenho
# (coordenadas já do desenho; não passam pelo campo)
DIRECT_DES = {7: {"alca": ([(1486, 947), (1480, 960), (1467, 980), (1452, 1000), (1440, 1020), (1427, 1040), (1416, 1060), (1404, 1082)], 27.0),  # runs.py
                  # contorno esquerdo: alto levado pelo campo; o trecho de baixo (junção com o cacho) relido por colunas (~8 px abaixo do 1)
                  "contorno_esq": ([(290, 135), (267, 206), (247, 278), (218, 337), (209, 357), (222, 385), (247, 419), (234, 444), (222, 484),
                                    (207, 524), (195, 577), (205, 607), (222, 628), (240, 650), (260, 667), (280, 682), (300, 694), (320, 704),
                                    (345, 715), (372, 721)], 16.0),
                  # fim do contorno direito: mais grosso no 7 (~24 px pela extensão vertical a 45°, runs.py) e desce até a borda
                  "contorno_n5": ([(1832, 640), (1850, 652), (1870, 672), (1890, 697), (1910, 722), (1925, 738)], 24.0)}}
TAPER_DES = {7: {"franja_h2", "franja_i1"}}
DIRECT_W_DES = {8: {"queixo": 25.0}}  # no 8 o queixo engrossa (~25 px pela varredura por colunas, borda da pele igual)
NO_ANCHOR_DES = {8: {"contorno_n2"}}
# larguras fixas por desenho (lidas por varredura): no alto do quadro a tinta clareia e o paquímetro de faixa estreita a linha
FIXED_W.update({7: {"mecha_n": 20.0}, 8: {"mecha_n": 22.0, "mecha_yy": 23.0}}.get(CEL, {}))
INK.update({7: {"alca": "#3e1d1c"}, 8: {"alca": "#3e1d1c"}}.get(CEL, {}))  # alça curta (sem trechos coloridos): cor medida no desenho
DIRECT_DES[8] = {"alca": ([(1491, 953), (1483, 960), (1476, 970), (1466, 980), (1456, 990), (1446, 1000), (1438, 1010), (1430, 1020), (1422, 1030),
                           (1415, 1040), (1408, 1050), (1402, 1060), (1397, 1070), (1392, 1082)], 24.0),  # centros das varreduras de tinta (runs.py)
                 # contorno de baixo à esquerda: no 8 a ponta do cacho fecha embaixo (curva até a mecha da direita)
                 "borda_inf_esq": ([(172, 801), (181, 811), (189, 821), (201, 844), (210, 866), (212, 880), (209, 892), (205, 904), (200, 916),
                                    (197, 928), (193, 940), (189, 952), (185, 964), (182, 976), (179, 988), (180, 1000), (186, 1012), (196, 1022),
                                    (212, 1031), (232, 1042), (258, 1053), (285, 1062), (305, 1066)], 14.0),
                 # queixo do 8: borda da pele por varredura (runs.py) menos meia largura; sem ancoragem (ela criava um calombo em x≈1040)
                 "queixo": ([(541, 560), (544, 590), (548, 620), (556, 650), (568, 680), (582, 710), (596, 740), (614, 770), (636, 800), (666, 830),
                             (708, 857), (745, 876), (800, 900), (860, 922), (920, 935), (980, 941), (1040, 942), (1100, 935), (1160, 923),
                             (1220, 907), (1280, 882), (1340, 846), (1385, 806), (1410, 770)], 25.0),
                 # contorno esquerdo do 8 (centros das varreduras; sem pontos de campo no alto à esquerda)
                 "contorno_esq": ([(298, 100), (288, 125), (277, 150), (273, 170), (270, 190), (265, 210), (259, 230), (249, 250), (237, 270),
                                   (223, 290), (206, 310), (194, 330), (199, 350), (209, 370), (221, 390), (236, 410), (244, 420), (240, 440),
                                   (235, 460), (227, 480), (218, 500), (209, 520), (201, 540), (195, 571), (205, 600), (220, 611), (250, 637),
                                   (270, 652), (300, 672), (330, 689), (360, 703), (372, 708)], 16.0),
                 # contorno do alto à direita: no 8 começa ~60 px mais alto (centros das varreduras), resto pelo campo
                 "contorno_n2": ([(1603, 40), (1622, 60), (1636, 70), (1649, 80), (1665, 100), (1682, 120), (1697, 140), (1711, 160), (1724, 180),
                                  (1734, 200), (1733, 220), (1728, 240), (1723, 260), (1717, 280), (1709, 300), (1700, 320), (1690, 340), (1677, 360),
                                  (1662, 380), (1645, 392)], 20.0)}
# linhas do cabelo relidas no próprio desenho (varreduras runs.py; coordenadas do desenho, sem campo)
HAIR_DES = {8: {"laco": [(163, 0), (162, 15), (168, 28), (180, 40), (195, 52), (210, 64), (226, 75), (245, 86), (265, 94), (285, 99), (300, 104)],
                "mecha_f": [(333, 0), (326, 20), (317, 40), (309, 60), (302, 80), (298, 100)],
                "franja_i2": [(893, 203), (905, 219), (915, 233), (930, 250), (950, 283), (1002, 310), (1053, 336), (1084, 336)],
                "franja_bico": [(1060, 302), (1070, 307), (1082, 312)],
                "mecha_yy": [(1639, 814), (1672, 840), (1708, 879), (1729, 912), (1740, 928), (1747, 936), (1750, 952), (1755, 968), (1760, 984),
                             (1765, 1000), (1769, 1016), (1773, 1032), (1776, 1048), (1780, 1064), (1784, 1082)],
                "franja_k2": [(1073, -6), (1074, 4), (1067, 49), (1061, 89), (1059, 100), (1057, 110), (1055, 120), (1052, 130), (1049, 140),
                              (1044, 150), (1040, 158)],
                # mecha da esquerda desce e fecha a ponta do cacho no 8 (varredura de tinta, runs.py)
                "mecha_e": [(426, 0), (411, 65), (399, 108), (375, 181), (364, 261), (357, 340), (360, 441), (366, 491), (384, 543), (387, 629),
                            (386, 707), (370, 755), (349, 822), (339, 871), (324, 896), (309, 906), (303, 916), (297, 928), (294, 940), (292, 952),
                            (290, 964), (289, 976), (290, 988), (291, 1000), (295, 1012), (299, 1024), (304, 1036), (310, 1048), (316, 1060),
                            (322, 1072), (326, 1082)],
                "mecha_n": [(1585, 0), (1590, 20), (1597, 40), (1602, 60), (1606, 80), (1610, 100), (1615, 120), (1619, 140), (1622, 160),
                            (1626, 180), (1628, 200), (1631, 240), (1633, 261), (1635, 338), (1630, 439), (1606, 515), (1591, 565), (1574, 616),
                            (1559, 642)]}}
HAIR_RAW = set(HAIR_DES.get(CEL, {}))
HAIR.update(HAIR_DES.get(CEL, {}))
RAW = set(DIRECT_DES.get(CEL, {}))
DIRECT.update(DIRECT_DES.get(CEL, {}))
SKIN_SIDE = {"queixo": -1, "contorno_esq": 1, "contorno_n2": -1, "contorno_n3a": -1, "contorno_n4a": -1, "contorno_n5": -1}
CLOSED = {}  # a boca virou anel medido na linha-massa (d218_face.py)


SEGMENT_INK = True
SMOOTH = 1
INK_Q = 0.5
BAND = "--meia-altura" not in sys.argv


def colored_segments(pts, ws, base_w, seg_len=180.0, clamp=(0.7, 1.3), ref=None):
    """Divide a linha em trechos de ~`seg_len` px e dá a cada um a cor da tinta medida nos seus centros (a tinta clareia
    no alto do quadro e escurece sobre o cabelo de trás ao longo de uma mesma linha)."""
    import math
    acc, cuts = 0.0, [0]
    for i in range(1, len(pts)):
        acc += math.dist(pts[i - 1], pts[i])
        if acc >= seg_len and len(pts) - i > 3:
            cuts.append(i)
            acc = 0.0
    cuts.append(len(pts) - 1)
    if len(cuts) < 3:
        return None
    out = []
    for a, b in zip(cuts[:-1], cuts[1:]):
        sp, sw = pts[a:b + 1], ws[a:b + 1]
        probe = [[round(p[0]), round(p[1])] for p in sp[::2]]
        pr = meas.first_list(d.call("PROBE_COLOR", {"reference": ref or REF0, "points": probe, "radius": 1}))
        # só amostras com cara de tinta (o centro medido pode cair no realce ou no cabelo ao lado)
        inkish = [p["rgb"] for p in pr if sum((p["rgb"][k] * 255 - (92, 46, 46)[k]) ** 2 for k in range(3)) ** 0.5 < 55]
        if len(inkish) >= 2:
            rgb = sorted(inkish, key=sum)
            col = {"type": "direct", "rgba": list(rgb[int(len(rgb) * INK_Q)]) + [1.0]}  # miolo da tinta, não a borda suavizada
        else:
            col = LINE
        c = meas.curve_from_measure(sp, sw, col, every=1)
        c["width"] = base_w
        c["profile"] = [[t, max(clamp[0], min(clamp[1], (w or base_w) / base_w))] for t, w in zip([k / max(1, len(sw) - 1) for k in range(len(sw))], sw)]
        out.append(c)
    return out


def measured_curves(table, step=3, closed=False, ref=None):
    REF = ref or REF0
    curves, log = [], {}
    for name, poly in table.items():
        if BAND:
            # paquímetro de faixa pela cor da tinta: separa a tinta do cabelo, do realce e da sombra sem viés de meia altura;
            # redesenho: mede a estimativa do desenho 1 e a do campo e fica com a que cai mais na tinta
            best = None
            raw = ref is not None or name in HAIR_RAW
            for cand in ([[tuple(p) for p in poly]] if raw else candidates(poly, field_first=False)):  # empate: desenho 1 (medido melhor assim)  # ref dada: dados lidos no próprio desenho (d218_dyn)
                cp, cw = meas.measure_band(d, REF, meas.densify(cand, step, closed=closed), "#5c2e2e", 0.09, closed=closed,
                                           passes=(16.0, 10.0), smooth=SMOOTH)
                sc = color_score(cp, "#5c2e2e", 55) if CEL != 1 else 0
                if best is None or sc > best[0]:
                    best = (sc, cp, cw, cand)
            _, pts, ws, poly = best
        else:
            pts, ws = meas.measure_line(d, REF, meas.densify(W(poly), step, closed=closed), closed=closed, line_color="#5c2e2e",
                                        passes=((14.0, 0.08), (9.0, 0.08)), smooth=3)
        if name in PIN_START:  # emenda exata com o trecho desenhado direto
            pts[0] = list(W(poly)[0])
        c = meas.curve_from_measure(pts, ws, ink_of(name, poly), closed=closed, every=1)
        # linhas do chibi são quase uniformes (11–15 px); onde a tinta funde numa massa escura a meia altura subestima:
        # piso de 11 px e variação do perfil limitada (leitura do agente nos recortes ampliados)
        c["width"] = max(c["width"], MIN_W)
        if name in FIXED_W:
            c["width"] = FIXED_W[name]
            ws = [FIXED_W[name]] * len(ws)
        if ANCHOR and name not in NO_ANCHOR:
            # refino pelas bordas (lado da pele/fundo é nítido): centro e largura locais
            ws = [max(w or 0, MIN_W) for w in ws]
            for search in (10.0, 4.0):
                pts, ws2 = meas.anchor_line(d, REF, pts, ws, c["width"], closed=closed, search=search)
                ws = ws if name in FIXED_W else ws2
                if name in PIN_START:
                    pts[0] = list(W(poly)[0])
            c = meas.curve_from_measure(pts, ws, ink_of(name, poly), closed=closed, every=1)
            c["width"] = max(c["width"], MIN_W - 1)
        c["profile"] = [[t, max(0.7, min(1.3, w))] for t, w in c["profile"]]
        taper = name in TAPER_DES.get(CEL, set())
        if taper:  # ponta fina no fim (as mechas da franja terminam em bico no redesenho)
            n = len(ws)
            k = max(3, n // 5)
            ws = [w if i < n - k else (w or c["width"]) * (0.35 + 0.65 * (n - 1 - i) / k) for i, w in enumerate(ws)]
            base = c["width"]
            c = meas.curve_from_measure(pts, ws, ink_of(name, poly), closed=closed, every=1)
            c["width"] = base
            c["profile"] = [[t, max(0.3, min(1.3, (w or base) / base))] for t, w in zip([i / max(1, n - 1) for i in range(n)], ws)]
        if SEGMENT_INK and not closed and name in INK:
            segs = colored_segments(pts, ws, c["width"], ref=REF, clamp=(0.3, 1.3) if taper else (0.7, 1.3))
            if segs:
                curves.extend(segs)
                log[name] = c["width"]
                continue
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
    [(1212, 93), (1232, 101), (1252, 111)],  # l1 entre o braço de k3 e a parte fixa (a parte visível muda por desenho)
]


def fecho():
    from d218 import node
    f = node("fecho", "Fecho do balde", {"type": "layer", "kind": "vector"}, ids["cut"])
    ck("fecho")
    d.apply({"kind": "set_props", "id": f, "data": {"visible": False}})
    d.apply({"kind": "set_props", "id": ids["cor"], "data": {"line_refs": [ids["linha"], ids["linha_massa"], f]}})
    d.apply({"kind": "replace_data", "id": ids[ck("fecho")], "data": {"type": "cel", "number": CEL}})
    curves = [{"points": [list(p) for p in W(c)], "width": 2.0, "tip": "marker", "geometry": "polyline", "color": LINE} for c in FECHO]
    return d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": F0, "layer": f, "curves": curves})


if __name__ == "__main__":
    print(fecho().get("description"))
    if "--so-fecho" in sys.argv:
        sys.exit()
    d.apply({"kind": "replace_data", "id": ids[ck("linha")], "data": {"type": "cel", "number": CEL}})
    curves, log = measured_curves(HAIR)
    c2, l2 = measured_curves(CLOSED, closed=True)
    for name, (poly, w) in DIRECT.items():
        w = DIRECT_W_DES.get(CEL, {}).get(name, w)
        best = None
        # (escolher entre desenho 1 e campo pela tinta no centro piorou: F1 −0,002 a −0,005; aqui só o campo ou a releitura)
        for cand in ([[tuple(p) for p in poly]] if name in RAW else [W(poly)]):
            if name in NO_ANCHOR_DES.get(CEL, set()):  # centros já lidos na tinta; a borda do fundo aqui é fraca (fundo escuro)
                best = (0, meas.densify(cand, 4))
                break
            cp = meas.densify(cand, 4)
            if name in SKIN_SIDE:  # encosta na pele: a borda da pele é nítida, ancora nela
                cp = meas.edge_offset_line(d, REF0, cp, SKIN_SIDE[name], w / 2, search=16.0 if name == "queixo" else 10.0)
            sc = color_score(cp, "#5c2e2e", 55) if CEL != 1 else 0
            if best is None or sc > best[0]:
                best = (sc, cp)
        pts = best[1]
        wl = [WIDTH_FN[name](p, w) for p in pts] if name in WIDTH_FN and name not in RAW else [w] * len(pts)
        if name in SKIN_SIDE and name in WIDTH_FN:  # largura variável: reancora pela borda da pele com a meia largura local
            pts = [[p[0] - SKIN_SIDE[name] * n[0] * (wi - w) / 2, p[1] - SKIN_SIDE[name] * n[1] * (wi - w) / 2]
                   for p, n, wi in zip(pts, meas._normals(pts), wl)]
        segs = colored_segments(pts, wl, w, clamp=(0.5, 2.5)) if SEGMENT_INK else None
        if segs:
            c2.extend(segs)
        else:
            c2.append({"points": pts, "width": w, "tip": "marker", "geometry": "spline", "color": ink_of(name, poly)})
    curves += c2; log.update(l2)
    json.dump(curves, open(os.path.join(os.path.dirname(__file__), "..", "..", "calibracao", "projetos", "d218_hair_curves.json"), "w"))
    print(json.dumps(log))
    print(draw(curves).get("description"))
    print(render("q0_cabelo"))
