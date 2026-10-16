"""Corte C: cor da moldura por balde na camada `moldura_cor` (line_refs = linhas + fecho). As regiões são lidas no render das
linhas (componentes conexos com a mesma folga do balde); cada uma recebe a MEDIANA da referência sobre a região inteira
(leitura de cor, como PROBE_COLOR, só que na área toda) e a semente vai no ponto mais interno. Regiões com cor de fundo ficam
vazias (o BG aparece). uso: python c131_fill.py [gap]"""
import os, sys
import numpy as np
from PIL import Image
from scipy.ndimage import label, binary_dilation, distance_transform_edt, find_objects
sys.path.insert(0, os.path.dirname(__file__))
from c131 import d, ids, F, REF

BG = ["#f0ebc2", "#eed9b7", "#e1c0a8", "#f2f5ca", "#ecccae", "#e6bba5", "#f0e3be", "#d0beac"]
TMP = r"C:\AnimeIAI\calibracao\tmp\c131_barreira.png"


def rgb(h):
    return [int(h[i:i + 2], 16) for i in (1, 3, 5)]


def run(gap=8.0, min_area=40):  # folga 8: fecha as pontas soltas das pétalas (4: F1 0,665; 8: 0,691; 12: 0,680)
    layers = [ids["moldura_linha"]] + [ids[k] for k in ("linha_suave", "fecho") if k in ids]
    d.call("RENDER", {"cut": ids["cut"], "frame": F, "out": TMP, "layers": layers})
    wall = np.asarray(Image.open(TMP).convert("RGBA"))[..., 3] > 40
    wall = binary_dilation(wall, iterations=max(1, int(gap / 2)))
    lab, n = label(~wall)
    ref = np.asarray(Image.open(REF).convert("RGB")).astype(float)
    bgc = np.array([rgb(h) for h in BG], float)
    seeds = []
    d.apply({"kind": "replace_data", "id": ids["moldura_cor_c1"], "data": {"type": "cel", "number": 1}})
    for i, sl in enumerate(find_objects(lab)):
        m = lab[sl] == i + 1
        area = int(m.sum())
        if area < min_area:
            continue
        med = np.median(ref[sl][m], axis=0)
        if np.min(np.sqrt(((bgc - med) ** 2).sum(1))) < 30:
            continue
        dt = distance_transform_edt(np.pad(m, 1))[1:-1, 1:-1]
        y, x = np.unravel_index(np.argmax(dt), dt.shape)
        seeds.append({"x": int(x + sl[1].start), "y": int(y + sl[0].start), "color": {"type": "direct", "rgba": [float(v) / 255 for v in med] + [1.0]}})
    res = d.call("FILL", {"cut": ids["cut"], "frame": F, "layer": ids["moldura_cor"], "seeds": seeds, "gap": gap})
    return n, len(seeds), res.get("description")


if __name__ == "__main__":
    print(run(*(float(a) for a in sys.argv[1:2])))
