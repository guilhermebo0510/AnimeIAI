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
