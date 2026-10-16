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
VAR_F = os.path.join(os.path.dirname(os.path.abspath(__file__)), "a138_variantes.json")  # escolhidas por a138_opt.py

def arc(cx, cy, rx, ry, t0, t1, n=24):
    """Arco de elipse (graus; 0 = direita, 90 = topo, sentido anti-horário na tela)."""
    import math
    return [(round(cx + rx * math.cos(math.radians(t0 + (t1 - t0) * k / n)), 1),
             round(cy - ry * math.sin(math.radians(t0 + (t1 - t0) * k / n)), 1)) for k in range(n + 1)]


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
    "c_l14": ([(607, 213), (597, 230), (583, 253), (570, 287), (560, 317), (555, 340)], 2.5, False, {"fino": True}),
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
    "c_m4": ([(858, 5), (845, 18), (830, 33), (815, 50), (802, 65), (780, 88), (757, 113), (730, 140), (707, 157), (683, 173), (657, 188), (632, 200)], 2.0, False),
    "c_m5": ([(708, 137), (703, 160)], 2.0, False),
    "c_m6a": ([(845, 73), (840, 93), (835, 113), (830, 127), (823, 147), (815, 170), (807, 188)], 2.5, False),
    "c_m6b": ([(830, 125), (807, 140), (787, 152), (760, 167), (733, 180), (713, 192)], 2.5, False),
    "c_m7": ([(720, 190), (713, 213), (707, 233), (697, 260), (685, 283), (675, 300), (660, 315), (648, 325)], 2.5, False, {"fino": True}),
    "c_m8": ([(692, 205), (690, 233)], 2.0, False, {"fino": True}),
    "c_m9": ([(610, 210), (633, 211), (657, 209), (680, 205), (700, 199)], 2.0, False, {"fino": True}),
    "c_m10": ([(632, 177), (632, 200)], 2.5, False),
    "c_m11": ([(613, 200), (615, 233), (620, 263), (628, 290), (637, 310), (643, 325)], 3.0, False, {"fino": True}),
    "c_m13": ([(788, 215), (767, 240), (743, 263), (720, 283), (697, 300), (673, 313), (650, 325)], 2.5, False, {"fino": True}),
    "c_m14": ([(860, 187), (840, 213), (817, 240), (790, 267), (760, 287), (733, 302), (707, 312), (680, 318), (650, 325)], 2.5, False, {"fino": True}),
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
    # cabelo, embaixo à esquerda (ink_grid 350–610 × 330–670)
    "c_d1": ([(358, 340), (362, 353), (368, 372), (378, 390), (390, 402), (402, 410)], 2.5, False),
    "c_d2": ([(382, 337), (383, 363)], 2.5, False),
    "c_d3": ([(385, 370), (392, 388), (400, 410)], 2.0, False),
    "c_d4": ([(415, 330), (412, 363), (411, 397), (412, 430), (418, 460), (427, 483), (437, 503), (447, 520)], 2.5, False),
    "c_d5": ([(432, 333), (427, 363), (425, 390), (427, 417), (430, 440), (433, 460)], 2.0, False),
    "c_d6": ([(377, 430), (378, 453), (383, 470), (392, 490), (400, 507), (407, 520), (412, 528)], 2.5, False),
    "c_d7": ([(400, 470), (402, 490), (403, 510), (408, 523), (412, 530)], 2.0, False),
    "c_d8": ([(400, 480), (410, 500), (422, 518), (433, 535), (443, 543), (448, 547)], 2.5, False),
    "c_d9": ([(497, 330), (493, 363), (497, 397), (500, 430), (501, 463), (503, 497), (508, 520), (515, 537), (518, 542)], 3.0, False),
    "c_d10": ([(513, 330), (513, 363), (515, 397), (518, 423), (520, 440)], 3.0, False),
    "c_d11": ([(522, 330), (525, 350), (532, 370), (540, 390), (547, 402)], 2.0, False),
    "c_d12": ([(497, 397), (507, 417), (517, 430), (520, 440)], 2.5, False),
    "c_d14": ([(447, 488), (450, 510), (460, 530), (472, 543), (483, 553), (497, 560), (508, 562)], 2.5, False),
    "c_d15": ([(440, 510), (445, 530), (450, 547)], 2.5, False),
    "c_d16": ([(462, 495), (470, 510), (482, 523), (497, 530), (507, 533)], 2.5, False),
    "c_d20": ([(450, 547), (460, 567), (473, 583), (487, 595)], 2.5, False),
    "c_d21": ([(470, 553), (480, 567), (493, 575), (507, 578), (517, 580)], 2.0, False),
    "c_d22": ([(502, 533), (508, 547), (515, 563), (520, 577), (522, 587)], 2.0, False),
    "c_d23": ([(493, 598), (498, 617), (507, 633), (517, 647), (523, 655)], 2.5, False),
    "c_d24": ([(493, 600), (507, 603), (520, 605), (530, 606)], 2.5, False),
    "c_d25": ([(527, 597), (537, 617), (550, 637), (563, 650), (580, 668)], 2.0, False),
    "c_d26": ([(512, 613), (517, 630), (522, 645), (525, 655)], 3.0, False),
    "c_d27": ([(527, 630), (533, 645), (542, 657), (545, 665)], 3.0, False),
    # cabelo, embaixo à direita (ink_grid 850–1110 × 330–670)
    "c_e1": ([(950, 338), (950, 363), (949, 390), (947, 417)], 2.5, False),
    "c_e2": ([(970, 357), (963, 383), (957, 403), (950, 417)], 2.0, False),
    "c_e3": ([(967, 390), (967, 417), (968, 443), (970, 457)], 2.0, False),
    "c_e4": ([(973, 443), (970, 463), (967, 480), (963, 497), (958, 513), (952, 530)], 2.5, False),
    "c_e5": ([(1008, 447), (1000, 470), (990, 490), (982, 503), (970, 517), (960, 527), (948, 537)], 2.5, False),
    "c_e7": ([(1042, 440), (1032, 467), (1020, 487), (1007, 503), (997, 513), (985, 523)], 2.0, False),
    "c_e9": ([(1057, 440), (1052, 470), (1052, 497), (1050, 517), (1048, 523)], 2.5, False),
    "c_e10": ([(1072, 473), (1067, 490), (1060, 503), (1050, 517)], 2.5, False),
    "c_e11": ([(1047, 483), (1040, 497), (1032, 508), (1025, 517)], 3.0, False),
    "c_e12": ([(1008, 520), (1000, 532), (983, 547), (970, 555), (960, 557)], 3.0, False),
    "c_e13": ([(1027, 503), (1020, 530), (1010, 547), (997, 563), (980, 580), (963, 597)], 2.0, False),
    "c_e14": ([(960, 540), (955, 563), (948, 580), (942, 597)], 2.0, False),
    "c_e15": ([(973, 503), (968, 520), (965, 537), (960, 557)], 2.5, False),
    "c_e16": ([(982, 510), (977, 523), (970, 537)], 2.5, False),
    "c_e17": ([(940, 598), (952, 598), (963, 597)], 2.5, False),
    "c_e18": ([(940, 598), (927, 617), (910, 637), (897, 650), (882, 668)], 2.0, False),
    "c_e19": ([(963, 597), (962, 620), (957, 640), (950, 657), (940, 668)], 2.5, False),
    "c_e20": ([(932, 633), (927, 647), (920, 667)], 3.0, False),
    "c_e21": ([(948, 627), (945, 643), (940, 667)], 3.0, False),
    "c_contorno_d": ([(1099, 287), (1095, 330), (1095, 363), (1092, 397), (1083, 430), (1078, 447), (1073, 463), (1070, 477), (1063, 493), (1058, 502)], 2.5, False),
    "c_e22": ([(1082, 330), (1077, 352), (1072, 375)], 2.5, False),
    "c_e23": ([(1043, 352), (1047, 368), (1050, 383)], 2.0, False),
    # olhos (recortes 4×: 510–700 e 780–970 × 360–500); íris com contorno só em cima e dos lados (embaixo degradê sem linha)
    "olho_e_palpebra": ([(517, 440), (525, 425), (535, 414), (547, 405), (560, 398), (580, 392), (600, 388), (618, 385)], 11.0, False, {"raw": "#74502f"}),
    "olho_e_palpebra_d": ([(644, 397), (658, 396), (667, 400), (674, 414)], 7.0, False, {"raw": "#74502f"}),
    "olho_e_iris": (arc(609, 438, 41, 38.5, -25, 205), 3.0, False, {"raw": "#6a4048"}),
    "olho_e_pupila": (arc(609, 441, 7.5, 7.5, 0, 360, 16)[:-1], 2.0, True, {"raw": "#5a3a48"}),
    "olho_e_cilio1": ([(567, 485), (575, 486)], 2.0, False, {"raw": INK}),
    "olho_e_cilio2": ([(590, 490), (608, 490), (627, 490)], 2.0, False, {"raw": INK}),
    "olho_d_palpebra": ([(792, 420), (797, 402), (810, 395), (830, 389), (855, 387), (880, 386), (905, 390), (925, 400), (940, 412), (950, 425)], 10.0, False, {"raw": "#6f5530"}),
    "olho_d_dobra": ([(797, 386), (817, 377), (842, 371), (867, 371), (892, 377), (915, 385), (932, 392)], 2.0, False, {"raw": "#5b3025"}),
    "olho_d_iris": (arc(862.5, 437.5, 40.5, 38.5, -25, 205), 3.0, False, {"raw": "#6a4048"}),
    "olho_d_pupila": (arc(861.75, 441.25, 7, 7, 0, 360, 16)[:-1], 2.0, True, {"raw": "#5a3a48"}),
    "olho_d_cilio1": ([(832, 489), (847, 490), (862, 490)], 2.0, False, {"raw": INK}),
    "olho_d_cilio2": ([(892, 489), (901, 487), (910, 485)], 2.0, False, {"raw": INK}),
    # gola, broche, laço e camisa (ink_grid 580–900 × 740–1080, 2×)
    "g_gola_e": ([(600, 772), (606, 790), (612, 806), (617, 820)], 2.5, False),
    "g_gola_baixo_e": ([(617, 822), (645, 832), (680, 842), (700, 845)], 2.5, False),
    "g_gola_baixo_d": ([(750, 842), (790, 835), (820, 827), (837, 820)], 2.5, False),
    "l_asa_e_topo": ([(580, 845), (610, 835), (640, 830), (670, 837), (700, 845)], 2.5, False),
    "l_asa_e_baixo": ([(580, 932), (610, 905), (640, 885), (670, 872), (700, 860)], 2.5, False),
    "l_asa_d": ([(755, 847), (790, 860), (820, 872), (850, 885), (880, 905), (900, 920)], 2.5, False),
    "p_lapela_d": ([(837, 820), (860, 835), (880, 855), (900, 875)], 2.5, False),
    "p_lapela_d2": ([(837, 822), (845, 860), (852, 912)], 2.5, False),
    "p_lapela_e": ([(617, 822), (612, 855), (602, 890), (595, 915)], 2.5, False),
    "b_broche_fora": (arc(728.5, 864.75, 25, 35.75, 0, 360, 32)[:-1], 2.0, True, {"raw": "#4a2a30"}),  # perfis na linha 866 e coluna 728
    "b_broche_dentro": (arc(728.5, 862.5, 18, 26, 0, 360, 32)[:-1], 2.0, True, {"raw": "#4a2a30"}),
    "b_botao": (arc(731, 950, 5, 7.5, 0, 360, 12)[:-1], 2.0, True),
    "k_camisa_e": ([(715, 905), (712, 965), (707, 1020), (705, 1080)], 2.5, False),
    "k_camisa_d": ([(740, 905), (742, 965), (747, 1020), (752, 1080)], 2.5, False),
    "k_camisa_e2": ([(705, 965), (697, 1020), (687, 1080)], 3.0, False),
    "k_camisa_d2": ([(752, 965), (762, 1020), (770, 1080)], 3.0, False),
    # paletó (ink_grid 190–600 × 820–1080 e 860–1270 × 800–1080)
    "p_ombro_e": ([(600, 837), (575, 846), (550, 857), (530, 880), (510, 887), (470, 900), (430, 910), (400, 917), (360, 926), (310, 940), (270, 945), (242, 948)], 2.0, False),
    "p_braco_e": ([(242, 948), (225, 970), (215, 995), (207, 1030), (197, 1060), (192, 1080)], 2.0, False),
    "p_lapela_e_longa": ([(540, 875), (500, 925), (470, 965), (445, 1010), (425, 1050), (415, 1080)], 2.0, False),
    "l_borda_e": ([(600, 897), (590, 915), (570, 940), (550, 965), (535, 990), (522, 1012), (522, 1030), (540, 1045), (570, 1060), (600, 1075)], 3.0, False),
    "p_vinco_e1": ([(262, 970), (263, 990), (264, 1012)], 3.0, False),
    "p_vinco_e2": ([(265, 1027), (268, 1055), (270, 1080)], 3.0, False),
    "p_vinco_e3": ([(295, 1012), (294, 1045), (293, 1080)], 3.0, False),
    "p_lapela_d_longa": ([(860, 835), (880, 847), (900, 867), (920, 885), (935, 910), (952, 935), (970, 970), (990, 1000), (1005, 1025), (1020, 1050), (1032, 1077)], 2.0, False),
    "p_ombro_d": ([(925, 887), (960, 897), (1000, 907), (1030, 915), (1070, 925), (1100, 934), (1140, 942), (1180, 950), (1210, 959), (1230, 967), (1240, 985), (1245, 1010), (1255, 1050), (1260, 1075)], 2.0, False),
    "l_borda_d": ([(860, 920), (880, 945), (900, 972), (915, 1000), (925, 1020), (922, 1035), (910, 1050), (890, 1055), (870, 1062)], 3.0, False),
    "p_vinco_d1": ([(1162, 1017), (1163, 1050), (1165, 1080)], 3.0, False),
    "p_vinco_d2": ([(1202, 1010), (1204, 1035), (1205, 1062)], 3.0, False),
    # franja sobre a testa e sobrancelhas (recorte 2× 540–900 × 180–420)
    "f_franja_baixo": ([(705, 377), (720, 366), (737, 355), (755, 345), (775, 335), (798, 320), (820, 305), (838, 288), (855, 270), (870, 252), (885, 235)], 2.0, False),
    "f_ponta": ([(700, 377), (715, 383), (730, 390), (745, 398), (760, 405), (775, 411), (785, 415)], 2.0, False),
    "f_ponta2": ([(740, 355), (745, 368), (750, 380), (762, 397), (775, 412)], 2.0, False),
    "s_sobr_e": ([(597, 342), (620, 339), (645, 335), (670, 330)], 2.0, False),
    "s_sobr_d": ([(795, 342), (808, 340), (822, 337)], 2.0, False),
    "olho_e_dobra": ([(540, 385), (565, 378), (590, 375), (615, 375), (640, 377), (665, 383), (690, 390)], 2.0, False),
    # boca (traços suaves, recorte 6×)
    "boca_e": ([(711.7, 636), (716.7, 633.7), (723.3, 632.5)], 2.5, False, {"raw": "#7f5a50"}),
    "boca_d": ([(735.8, 632.2), (743.3, 634.2), (749.2, 635.8)], 2.5, False, {"raw": "#7f5a50"}),
    # franja sobre o olho esquerdo (releitura ink_grid 3× 560–780 × 190–400)
    "c_m8b": ([(688, 222), (685, 243), (682, 262), (677, 287), (675, 300)], 2.0, False, {"fino": True}),
    "c_m4b": ([(630, 210), (631, 230), (631, 248)], 2.0, False, {"fino": True}),
    "olho_e_dobra2": ([(568, 290), (590, 289), (610, 288), (625, 287)], 3.0, False, {"fino": True}),
    "f_ponta_e": ([(650, 333), (643, 350), (633, 367), (623, 383)], 2.0, False, {"fino": True}),
    # topo do cabelo (mapa de faltantes 780–1060 × 0–110)
    "c_t4": ([(903, 3), (915, 5), (925, 12), (930, 22), (933, 33)], 2.0, False, {"fino": True}),
    "c_t5": ([(948, 0), (958, 5), (970, 12), (978, 17)], 2.0, False, {"fino": True}),
    "c_contorno_dt": ([(1000, 0), (1012, 12), (1025, 28), (1037, 45), (1047, 62), (1055, 80), (1060, 95)], 2.0, False, {"fino": True}),
    # arcos internos da íris (azul-marinho)
    "olho_d_arco_e": ([(826, 464), (835, 472), (847, 478)], 2.0, False, {"raw": "#2c2f86"}),
    "olho_d_arco_d": ([(877, 478), (890, 472), (899, 463)], 2.0, False, {"raw": "#2c2f86"}),
    "olho_e_arco_e": ([(572, 464), (581, 472), (593, 478)], 2.0, False, {"raw": "#2c2f86"}),
    "olho_e_arco_d": ([(624, 478), (637, 472), (646, 463)], 2.0, False, {"raw": "#2c2f86"}),
    # babados da gola (ink_grid 3× 595–865 × 750–840)
    "g_aro_e": ([(600, 772), (612, 767), (625, 762), (638, 757)], 2.0, False),
    "g_dobra_e": ([(628, 778), (632, 800), (637, 813), (638, 830)], 2.0, False),
    "g_laco_e": ([(627, 774), (634, 772), (642, 774), (642, 783)], 2.0, False),
    "g_topo_c": ([(642, 783), (662, 783), (682, 785), (685, 793)], 2.5, False),
    "g_dobra_c1": ([(672, 793), (673, 817), (682, 837)], 2.0, False),
    "g_faixa_c": ([(675, 793), (705, 793), (722, 790)], 3.0, False),
    "g_laco_c": ([(705, 787), (722, 785), (735, 783), (742, 787)], 2.5, False),
    "g_dobra_c2": ([(723, 795), (723, 812), (723, 830)], 3.0, False),
    "g_dobra_c3": ([(732, 793), (733, 812), (733, 830)], 2.0, False),
    "g_topo_d": ([(742, 788), (768, 790), (778, 785)], 2.5, False),
    "g_dobra_d1": ([(772, 782), (782, 783), (783, 800), (783, 817)], 2.5, False),
    "g_topo_d2": ([(782, 782), (792, 782), (802, 782)], 2.0, False),
    "g_aro_d": ([(818, 755), (828, 757), (842, 760), (853, 765)], 2.5, False),
    "g_aro_d2": ([(818, 768), (828, 770), (842, 772)], 2.0, False),
    "g_borda_d": ([(853, 765), (848, 787), (843, 807), (840, 823)], 2.5, False),
    "g_dobra_d2": ([(818, 777), (835, 780), (835, 803), (833, 817)], 2.0, False),
}


BG_POINTS = [(100, 300), (200, 700), (300, 150), (1150, 300), (1150, 700), (1500, 500), (1300, 850), (1700, 1000), (880, 780),
             (1000, 800), (1080, 850), (560, 790), (450, 830), (20, 1050), (1250, 30)]

# fechos do balde: camada vetorial oculta, só nas line_refs da cor (bordas de cor sem tinta na referência)
FECHO = {
    "iris_e_baixo": arc(609, 438, 41, 38.5, 200, 340, 16),
    "iris_d_baixo": arc(862.5, 437.5, 40.5, 38.5, 200, 340, 16),
    "branco_e": [(517, 440), (525, 437), (535, 460), (550, 475), (567, 482), (590, 486), (615, 486), (635, 484), (652, 475), (664, 462),
                 (667, 450), (667, 435), (662, 422), (655, 415), (650, 410)],
    "branco_d": [(797, 415), (800, 422), (802, 440), (807, 455), (817, 467), (835, 480), (860, 485), (885, 485), (905, 482), (922, 475),
                 (935, 465), (944, 452), (947, 440), (949, 430), (950, 425)],
    "pescoco_gola": [(636, 762), (660, 785), (680, 790), (700, 790), (735, 788), (760, 787), (790, 775), (832, 762)],
    # sombras do cabelo e da testa (fronteiras sem tinta lidas no mapa de bordas faltantes)
    "sombra_franja": [(620, 237), (660, 228), (700, 212), (740, 195), (770, 180), (800, 160), (818, 110)],
    "sombra_cunha": [(818, 110), (830, 90), (842, 70), (852, 50), (857, 40), (858, 60), (857, 85), (855, 110)],
    # olhos: sombra lilás no alto do branco, faixa marinho/azul da íris
    "branco_d_sombra_e": [(800, 427), (811, 424), (823, 423)],
    "branco_d_sombra_d": [(896, 419), (902, 420), (920, 425), (935, 433), (945, 442), (951, 452)],
    "branco_e_sombra_e": [(522, 438), (545, 428), (569, 425)],
    "branco_e_sombra_d": [(643, 418), (649, 420), (658, 428), (665, 438)],
    "iris_d_faixa": [(823, 452), (845, 452), (855, 450), (868, 450), (880, 448), (902, 450)],
    "iris_e_faixa": [(569, 453), (591, 453), (601, 451), (614, 451), (626, 449), (649, 451)],
    "sombra_dir": [(880, 60), (884, 95), (890, 130), (898, 165), (906, 198)],
    "sombra_esq": [(525, 140), (545, 120), (565, 103), (578, 95)],
    "sombra_testa_d": [(848, 200), (856, 230), (866, 262), (878, 292), (888, 320)],
}


# formas chapadas sem contorno (camada raster `detalhe`, entre a cor e a linha): contorno digitado + MEASURE_EDGE
SHAPES = {
    "nariz": ([(724, 525), (729, 532), (731, 540), (731, 548), (729, 554), (725, 557.5), (720, 557), (713, 552), (707.5, 546),
               (706.7, 541), (709, 536), (715, 529), (720, 526)], "#d9918a"),
    "narina": ([(728, 549.5), (731, 550.5), (731, 554), (728.5, 555)], "#8a4236"),
}
ORDER = ["bg", "bg_foco", "cor", "detalhe", "linha", "fx", "fecho"]


def reorder():
    present = [k for k in ORDER if k in ids]
    for i, k in enumerate(present):
        d.apply({"kind": "move_node", "id": ids[k], "parent": ids["cut"], "index": i})


def shapes():
    node("detalhe", "Detalhes chapados", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("detalhe_c1", "1", {"type": "cel", "number": 1}, ids["detalhe"])
    reorder()
    d.apply({"kind": "replace_data", "id": ids["detalhe_c1"], "data": {"type": "cel", "number": 1}})
    for name, (poly, col) in SHAPES.items():
        pts = meas.densify([list(p) for p in poly], 2.0, closed=True)
        if len(poly) > 6:  # contornos maiores: borda medida; detalhes de poucos px ficam como digitados
            pts = meas.measure_edge(d, REF, pts, passes=((4.0, 0.03), (2.0, 0.03)))
        d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": F, "layer": ids["detalhe"], "shapes": [{"points": pts, "geometry": "polyline"}],
                              "color": {"type": "direct", "rgba": [int(col[i:i + 2], 16) / 255 for i in (1, 3, 5)] + [1.0]}})
    return len(SHAPES)


def measured(poly, w_nom, closed=False, fine=False):
    ps = ((6.0, 0.06), (3.0, 0.06)) if fine else ((14.0, 0.06), (8.0, 0.06))  # curta: áreas escuras vizinhas (laço) puxam a linha
    pts, ws = meas.measure_line(d, REF, meas.densify(poly, 3, closed=closed), closed=closed, passes=ps, smooth=1)
    for sr in ((3.0, 2.0) if fine else (6.0, 3.0)):
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
    var = json.load(open(VAR_F)) if os.path.exists(VAR_F) else {}
    for name, (poly, w, closed, *opt) in LINES.items():
        o = dict(opt[0]) if opt else {}
        v = var.get(name)
        if v == "raw" and "raw" not in o:
            o["raw"] = INK
        elif v in ("fino", "longo"):
            o.pop("raw", None) if "raw" not in (opt[0] if opt else {}) else None
            o["fino"] = v == "fino"
        if "raw" in o:  # geometria digitada sem medição (olhos: o miolo escuro da íris engana o paquímetro)
            c = [int(o["raw"][i:i + 2], 16) / 255 for i in (1, 3, 5)]
            curves.append({"points": [list(p) for p in poly], "width": float(w), "tip": "marker", "geometry": "spline",
                           "color": {"type": "direct", "rgba": c + [1.0]}, "closed": closed})
            continue
        fine = o.get("fino", name[:2] in ("g_", "l_", "p_", "b_", "k_"))
        key = json.dumps([name, poly, w, closed, F, fine])
        if key not in cache:
            cache[key] = measured(poly, w, closed, fine)
        curves.append(cache[key])
    json.dump(cache, open(CACHE_F, "w"))
    r = d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": F, "layer": ids["linha"], "curves": curves}).get("description")
    f = node("fecho", "Fecho do balde", {"type": "layer", "kind": "vector"}, ids["cut"])
    node("fecho_c1", "1", {"type": "cel", "number": 1}, f)
    d.apply({"kind": "set_props", "id": f, "data": {"visible": False}})
    d.apply({"kind": "set_props", "id": ids["cor"], "data": {"line_refs": [ids["linha"], f]}})
    d.apply({"kind": "replace_data", "id": ids["fecho_c1"], "data": {"type": "cel", "number": 1}})
    fc = [{"points": [list(p) for p in c], "width": 2.0, "tip": "marker", "geometry": "spline",
           "color": {"type": "direct", "rgba": [0, 0, 0, 1]}} for c in FECHO.values()]
    d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": F, "layer": f, "curves": fc})
    return r


def fill(gap=4.0, min_area=40):
    """Mesma rotulagem do balde do app (fill.rs): núcleos = componentes de {D > gap/2} (D = distância à linha); cada pixel
    fora da linha vai para o núcleo mais próximo. Cor do núcleo = mediana da referência na região; semente no ponto mais
    interno; região que na referência é o nosso BG fica vazia."""
    from scipy.ndimage import label, distance_transform_edt, find_objects
    tl, tb = os.path.join(TMP, "a138_linhas.png"), os.path.join(TMP, "a138_bg.png")
    d.call("RENDER", {"cut": ids["cut"], "frame": F, "out": tl, "layers": [ids["linha"]] + ([ids["fecho"]] if "fecho" in ids else [])})
    d.call("RENDER", {"cut": ids["cut"], "frame": F, "out": tb, "layers": [ids["bg"], ids["bg_foco"]]})
    line = np.asarray(Image.open(tl).convert("RGBA"))[..., 3] >= 0.35 * 255
    D = distance_transform_edt(~line)
    cores, n = label(D > gap / 2)
    # propagação dos núcleos só por pixels fora da linha (como o balde do app: a onda não atravessa a linha)
    from skimage.segmentation import watershed
    lab = watershed(-D, markers=cores, mask=~line)
    ref = np.asarray(Image.open(REF).convert("RGB")).astype(float)
    bgr = np.asarray(Image.open(tb).convert("RGB")).astype(float)
    # fundo = regiões que contêm pontos digitados de fundo (cor parecida não serve: branco do olho ≈ parede creme)
    bg_labels = {int(lab[y, x]) for x, y in BG_POINTS} - {0}
    seeds, skipped = [], 0
    flat, refl, bgl = lab.ravel(), ref.reshape(-1, 3), bgr.reshape(-1, 3)
    order = np.argsort(flat, kind="stable")
    bounds = np.searchsorted(flat[order], np.arange(1, n + 2))
    Dc = np.where(cores > 0, D, 0).ravel()
    d.apply({"kind": "replace_data", "id": ids["cor_c1"], "data": {"type": "cel", "number": 1}})
    for k in range(1, n + 1):
        idx = order[bounds[k - 1]:bounds[k]]
        if len(idx) < min_area:
            continue
        if k in bg_labels:  # região de fundo (o BG já está pintado embaixo)
            skipped += 1
            continue
        med = np.median(refl[idx], axis=0)
        ci = np.flatnonzero(cores.ravel() == k)
        j = ci[np.argmax(Dc[ci])]
        seeds.append({"x": int(j % line.shape[1]), "y": int(j // line.shape[1]), "color": {"type": "direct", "rgba": [float(v) / 255 for v in med] + [1.0]}})
    if not seeds:
        return "sem regiões"
    r = d.call("FILL", {"cut": ids["cut"], "frame": F, "layer": ids["cor"], "seeds": seeds, "gap": gap}).get("description")
    # 2ª passada, folga pequena: regiões fechadas estreitas (íris entre contorno e pupila, branco do olho) não têm núcleo
    # com D > gap/2 e ficam vazias na 1ª
    tc = os.path.join(TMP, "a138_cor.png")
    d.call("RENDER", {"cut": ids["cut"], "frame": F, "out": tc, "layers": [ids["cor"]]})
    empty = np.asarray(Image.open(tc).convert("RGBA"))[..., 3] < 20
    g2 = 4.0
    lab2, n2 = label(~line & (D > g2 / 2))
    bg2 = {int(lab2[y, x]) for x, y in BG_POINTS} - {0}
    seeds2 = []
    for k, sl in enumerate(find_objects(lab2), start=1):
        m = lab2[sl] == k
        if k in bg2 or m.sum() < min_area or empty[sl][m].mean() < 0.8:
            continue
        med = np.median(ref[sl][m], axis=0)
        dt = distance_transform_edt(np.pad(m, 1))[1:-1, 1:-1]
        y, x = np.unravel_index(np.argmax(dt), dt.shape)
        seeds2.append({"x": int(x + sl[1].start), "y": int(y + sl[0].start), "color": {"type": "direct", "rgba": [float(v) / 255 for v in med] + [1.0]}})
    if seeds2:
        d.call("FILL", {"cut": ids["cut"], "frame": F, "layer": ids["cor"], "seeds": seeds2, "gap": g2})
    return f"{n} núcleos, {len(seeds)} sementes, {skipped} fundo; {r}; 2ª passada {len(seeds2)}"


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "tudo"
    if what in ("linhas", "tudo"):
        print(lines())
    if what in ("formas", "tudo"):
        print("formas", shapes())
    if what in ("balde", "tudo"):
        print(fill(float(sys.argv[2]) if len(sys.argv) > 2 else 4.0))
