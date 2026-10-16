"""Corte A (#138), personagem no quadro 12: linhas DIGITADAS pelo agente (leitura em recorte com grade, ±5 px), medidas pelo
paquímetro (meia altura + âncora nas bordas, como o corte C) e desenhadas na camada `linha`; cor por balde na camada `cor`
(mediana da referência por região; região com cor do nosso BG fica vazia).
uso: python a138_char.py [linhas|balde|tudo]"""
import json, os, sys
import numpy as np
from PIL import Image
sys.path.insert(0, os.path.dirname(__file__))
from a138 import d, ids, F, REF, node
import meas

INK = "#6a3533"  # núcleo da linha medido no queixo (linhas de ~2 px, halo claro de nitidez dos dois lados)
TMP = r"C:\AnimeIAI\calibracao\tmp"
CACHE_F = os.path.join(TMP, "a138_medidas.json")

# nome: (estimativa, largura nominal, fechada)
LINES = {
    # contorno do rosto (queixo) e pescoço
    "queixo_e": ([(510, 580), (530, 602), (542, 620), (553, 640), (560, 650), (580, 665), (600, 682), (620, 695), (633, 703),
                  (650, 713), (670, 725), (690, 735), (710, 743), (727, 749), (740, 748)], 2.0, False),
    "queixo_d": ([(722, 748), (750, 742), (780, 732), (807, 718), (830, 703), (853, 690), (877, 675), (893, 663), (910, 650),
                  (923, 633), (933, 617), (940, 600), (950, 580), (960, 558), (968, 543), (977, 527), (983, 510)], 2.0, False),
    "pescoco_e": ([(633, 703), (636, 720), (638, 738), (640, 753)], 2.0, False),
    "pescoco_d": ([(830, 703), (830, 720), (830, 738), (830, 753)], 2.0, False),
    # cabelo, alto à esquerda (leitura em ink_grid 350–610 × 0–340)
    "c_contorno_topo": ([(492, 1), (477, 10), (460, 23), (447, 37), (437, 50), (425, 70), (415, 93), (407, 123), (402, 143), (399, 170), (398, 182)], 2.0, False),
    "c_contorno_e": ([(398, 182), (387, 197), (375, 217), (365, 243), (361, 272), (358, 300), (358, 340)], 2.5, False),
    "c_l3": ([(450, 57), (440, 80), (432, 107), (425, 133), (420, 157), (419, 172)], 3.0, False),
    "c_l5": ([(467, 80), (460, 100), (452, 128)], 2.0, False),
    "c_l6": ([(550, 5), (537, 20), (523, 38), (510, 62), (502, 77)], 2.0, False),
    "c_l7": ([(498, 83), (487, 98), (473, 117), (460, 140), (448, 167), (438, 193), (433, 217)], 2.0, False),
    "c_l8": ([(493, 90), (483, 110), (479, 135), (477, 160), (475, 190), (475, 215), (477, 240), (482, 262), (486, 280)], 2.0, False),
    "c_l9": ([(577, 68), (550, 97), (530, 120), (513, 143), (500, 173), (497, 200), (493, 210), (490, 233), (488, 250)], 2.0, False),
    "c_l10": ([(568, 117), (550, 140), (537, 163), (527, 187), (523, 200), (522, 217), (521, 233)], 2.0, False),
    "c_l11": ([(577, 57), (577, 80), (577, 103)], 2.0, False),
    "c_l12": ([(608, 10), (601, 40), (595, 67), (587, 97), (580, 127), (579, 157), (580, 187), (583, 200), (588, 213)], 3.0, False),
    "c_l13": ([(608, 103), (600, 123), (595, 150), (592, 173), (590, 187), (591, 213)], 2.5, False),
    "c_l14": ([(607, 213), (597, 230), (583, 253), (570, 287), (560, 317), (555, 340)], 2.5, False),
    "c_l16": ([(402, 208), (410, 227), (420, 243), (430, 257), (432, 263)], 3.0, False),
    "c_l17": ([(405, 230), (412, 260), (422, 283), (430, 307), (436, 325)], 2.0, False),
    "c_l18": ([(430, 260), (430, 285), (430, 310)], 2.0, False),
    "c_l19": ([(393, 233), (387, 260), (383, 287), (382, 317), (382, 337)], 3.0, False),
    "c_l20": ([(517, 273), (518, 300), (522, 327), (523, 340)], 3.0, False),
    "c_l21": ([(493, 293), (491, 317), (490, 340)], 2.0, False),
    "c_l22": ([(422, 313), (417, 337)], 2.0, False),
    # cabelo, alto central (ink_grid 600–860 × 0–340)
    "c_m1a": ([(630, 0), (620, 13), (613, 25)], 2.5, False),
    "c_m1": ([(653, 27), (640, 43), (628, 62), (620, 77), (612, 93), (605, 110), (600, 130)], 2.0, False),
    "c_m2": ([(708, 13), (690, 27), (665, 53)], 2.0, False),
    "c_m3": ([(627, 127), (622, 147), (617, 177)], 3.0, False),
    "c_m4": ([(802, 65), (780, 88), (757, 113), (730, 140), (707, 157), (683, 173), (657, 188), (632, 200)], 2.0, False),
    "c_m5": ([(708, 137), (703, 160)], 2.0, False),
    "c_m6a": ([(845, 73), (840, 93), (835, 113), (830, 127), (823, 147), (815, 170), (807, 188)], 2.5, False),
    "c_m6b": ([(830, 125), (807, 140), (787, 152), (760, 167), (733, 180), (713, 192)], 2.5, False),
    "c_m7": ([(720, 190), (713, 213), (707, 233), (697, 260), (685, 283), (675, 300), (660, 315), (648, 325)], 2.5, False),
    "c_m8": ([(692, 205), (690, 233)], 2.0, False),
    "c_m9": ([(610, 210), (633, 211), (657, 209), (680, 205), (700, 199)], 2.0, False),
    "c_m10": ([(632, 177), (632, 200)], 2.5, False),
    "c_m11": ([(613, 200), (615, 233), (620, 263), (628, 290), (637, 310), (643, 325)], 3.0, False),
    "c_m13": ([(788, 215), (767, 240), (743, 263), (720, 283), (697, 300), (673, 313), (650, 325)], 2.5, False),
    "c_m14": ([(860, 187), (840, 213), (817, 240), (790, 267), (760, 287), (733, 302), (707, 312), (680, 318), (650, 325)], 2.5, False),
    "c_m15": ([(847, 230), (833, 253), (820, 273), (807, 290)], 2.0, False),
    "c_m16": ([(800, 297), (787, 310), (773, 323), (763, 340)], 2.5, False),
    # cabelo, alto à direita (ink_grid 850–1110 × 0–340)
    "c_r1": ([(912, 67), (900, 107), (887, 143), (870, 170), (860, 187)], 2.0, False),
    "c_r2": ([(912, 70), (922, 107), (933, 140), (947, 177), (960, 200), (983, 230), (1003, 250), (1027, 267), (1047, 282)], 2.0, False),
    "c_r3": ([(948, 70), (957, 107), (968, 143), (982, 173), (993, 200), (1005, 223), (1020, 247), (1038, 275)], 2.0, False),
    "c_r4a": ([(980, 14), (993, 30), (1003, 45), (1013, 60), (1022, 75)], 2.0, False),
    "c_r4b": ([(987, 57), (998, 70), (1008, 87), (1017, 103), (1023, 118)], 2.0, False),
    "c_r5": ([(973, 92), (983, 107), (993, 128)], 2.0, False),
    "c_r6": ([(1025, 83), (1033, 100), (1042, 127), (1050, 150), (1053, 162)], 2.0, False),
    "c_r6b": ([(1040, 60), (1048, 77), (1054, 97), (1056, 115)], 2.0, False),
    "c_r7": ([(1028, 133), (1032, 157), (1035, 177), (1037, 197), (1037, 223)], 2.0, False),
    "c_r8": ([(1005, 150), (1013, 167), (1023, 193), (1030, 217), (1035, 240)], 2.0, False),
    "c_r9": ([(1058, 130), (1070, 147), (1080, 167), (1085, 182)], 2.0, False),
    "c_r10": ([(1090, 198), (1095, 220), (1098, 243), (1099, 265), (1099, 287)], 2.0, False),
    "c_r11": ([(1060, 213), (1070, 237), (1077, 257), (1080, 277), (1081, 297)], 2.0, False),
    "c_r12a": ([(1082, 307), (1083, 338)], 2.5, False),
    "c_r12b": ([(1095, 310), (1088, 338)], 2.5, False),
    "c_r13e": ([(888, 200), (880, 233), (873, 263), (868, 293), (862, 317), (855, 337)], 2.5, False),
    "c_r13d": ([(888, 200), (903, 220), (917, 247), (930, 273), (940, 293), (948, 320), (950, 338)], 2.5, False),
    "c_r15": ([(922, 220), (930, 233), (936, 250), (940, 267), (942, 277)], 2.0, False),
    "c_r17": ([(973, 313), (973, 333)], 2.0, False),
}


def measured(poly, w_nom, closed=False):
    pts, ws = meas.measure_line(d, REF, meas.densify(poly, 3, closed=closed), closed=closed, passes=((14.0, 0.06), (8.0, 0.06)), smooth=1)
    for sr in (6.0, 3.0):
        pts, ws = meas.anchor_line(d, REF, pts, [max(wv or 0.0, 0.9 * w_nom) for wv in ws], w_nom, closed=closed, search=sr,
                                   w_range=(1.2, 8.0))
    ws = [max(wv or 0.0, 0.9 * w_nom) for wv in ws]
    # tinta fixa: a amostra a raio 0 numa linha de ~2 px cai fora do núcleo (pegou pele e cabelo)
    col = [int(INK[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    c = meas.curve_from_measure(pts, ws, {"type": "direct", "rgba": col + [1.0]}, closed=closed, every=1)
    c["width"] = max(c["width"], 0.6 * w_nom)
    c["profile"] = [[t, max(0.4, min(1.6, w))] for t, w in c["profile"]]
    return c


def lines():
    cache = json.load(open(CACHE_F)) if os.path.exists(CACHE_F) else {}
    d.apply({"kind": "replace_data", "id": ids["linha_c1"], "data": {"type": "cel", "number": 1}})
    curves = []
    for name, (poly, w, closed) in LINES.items():
        key = json.dumps([name, poly, w, closed, F])
        if key not in cache:
            cache[key] = measured(poly, w, closed)
        curves.append(cache[key])
    json.dump(cache, open(CACHE_F, "w"))
    return d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": F, "layer": ids["linha"], "curves": curves}).get("description")


def fill(gap=4.0, min_area=40):
    from scipy.ndimage import label, binary_dilation, distance_transform_edt, find_objects
    tl, tb = os.path.join(TMP, "a138_linhas.png"), os.path.join(TMP, "a138_bg.png")
    d.call("RENDER", {"cut": ids["cut"], "frame": F, "out": tl, "layers": [ids["linha"]]})
    d.call("RENDER", {"cut": ids["cut"], "frame": F, "out": tb, "layers": [ids["bg"], ids["bg_foco"]]})
    wall = binary_dilation(np.asarray(Image.open(tl).convert("RGBA"))[..., 3] > 40, iterations=max(1, int(gap / 2)))
    lab, _ = label(~wall)
    ref = np.asarray(Image.open(REF).convert("RGB")).astype(float)
    bgr = np.asarray(Image.open(tb).convert("RGB")).astype(float)
    seeds = []
    d.apply({"kind": "replace_data", "id": ids["cor_c1"], "data": {"type": "cel", "number": 1}})
    for i, sl in enumerate(find_objects(lab)):
        m = lab[sl] == i + 1
        if m.sum() < min_area:
            continue
        med = np.median(ref[sl][m], axis=0)
        # região que é fundo na referência (nosso BG já cobre): fração de pixels perto do BG renderizado
        close = np.sqrt(((ref[sl][m] - bgr[sl][m]) ** 2).sum(1)) < 30
        if close.mean() > 0.5:
            continue
        dt = distance_transform_edt(np.pad(m, 1))[1:-1, 1:-1]
        y, x = np.unravel_index(np.argmax(dt), dt.shape)
        seeds.append({"x": int(x + sl[1].start), "y": int(y + sl[0].start), "color": {"type": "direct", "rgba": [float(v) / 255 for v in med] + [1.0]}})
    if not seeds:
        return "sem regiões"
    return d.call("FILL", {"cut": ids["cut"], "frame": F, "layer": ids["cor"], "seeds": seeds, "gap": gap}).get("description")


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "tudo"
    if what in ("linhas", "tudo"):
        print(lines())
    if what in ("balde", "tudo"):
        print(fill())
