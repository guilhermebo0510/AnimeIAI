"""Corte A (#138), fundo estático, passada 1: parede creme, batente lilás com frisos, porta marrom, filete dourado (direita) e
papel de parede lilás (esquerda, sem os ornamentos dourados ainda). Geometria = faixas verticais DIGITADAS a partir do perfil
por coluna (bordas a meia transição); cor de cada faixa por banda de linhas = ajuste de mínimos quadrados ao perfil medido da
referência com o mesmo desfoque que o app aplica depois (σ=3 lido nas transições de 10–90% ≈ 8 px), pesando só os pixels
que são fundo (fora do personagem). Desfoque do BG por camada de efeito logo acima dele (não pega o personagem).
uso: python a138_bg.py [fit|run]"""
import json, os, sys
import numpy as np
from PIL import Image
sys.path.insert(0, os.path.dirname(__file__))
from a138 import d, ids, node, F, REF

SIGMA = 3.0
BAND = 60
# bordas digitadas (x) das faixas da direita, lidas do perfil mediano das linhas 560–660 (`a138_bg.py perfil`)
EDGES = [860, 886, 892, 898, 934, 944, 1178, 1186, 1196, 1222, 1238, 1394, 1412, 1444, 1468, 1480, 1709, 1736, 1767, 1795, 1882,
         1892, 1920]
WALL_X1 = 606   # papel de parede lilás até aqui; filete dourado até 632; atrás do personagem até 860 (coberto)
GOLD_X1 = 632


def valid_mask(h, w):
    """Onde a referência mostra fundo (o personagem cobre o resto). Polígonos digitados com folga."""
    m = np.zeros((h, w), bool)
    m[:, 1250:] = True
    m[:865, 1110:1250] = True
    m[665:835, 860:1110] = True
    return m


def gauss_rows(x0, x1, n):
    xs = np.arange(n)
    k = np.zeros((len(EDGES) - 1, n))
    from math import erf, sqrt
    e = np.vectorize(lambda v: erf(v / (sqrt(2) * SIGMA)))
    for i in range(len(EDGES) - 1):
        a, b = EDGES[i], EDGES[i + 1]
        # fração da faixa [a,b) que cai no pixel x depois do desfoque (pixel = centro x+0,5); bordas extremas se estendem
        lo = (xs + x0 + 0.5) - a if i > 0 else np.full(n, 1e9)
        hi = (xs + x0 + 0.5) - b if i < len(EDGES) - 2 else np.full(n, -1e9)
        k[i] = 0.5 * (e(lo) - e(hi))
    return k


def fit():
    a = np.asarray(Image.open(REF).convert("RGB")).astype(float)
    h, w, _ = a.shape
    vm = valid_mask(h, w)
    x0, x1 = EDGES[0], EDGES[-1]
    K = gauss_rows(x0, x1, x1 - x0).T  # (n px, n faixas)
    bands = []
    for y in range(0, h, BAND):
        blk, v = a[y:y + BAND, x0:x1], vm[y:y + BAND, x0:x1]
        cnt = v.sum(0)
        prof = np.array([np.median(blk[v[:, j], j], 0) if cnt[j] >= 5 else np.zeros(3) for j in range(x1 - x0)])
        wt = (cnt >= 5).astype(float)
        cover = (K * wt[:, None]).sum(0)  # quanto de cada faixa é visível nesta banda
        A = K * wt[:, None]
        lam = 1e-3
        cols = np.linalg.solve(A.T @ A + lam * np.eye(K.shape[1]), A.T @ (prof * wt[:, None]))
        cols = np.clip(cols, 0, 255)
        res = np.abs(K @ cols - prof)[wt > 0].mean() if wt.sum() else 0.0
        bands.append({"y": y, "cols": cols, "cover": cover, "res": round(float(res), 2)})
    # faixa coberta pelo personagem numa banda: herda a banda visível mais próxima
    ok = np.array([b["cover"] >= 3.0 for b in bands])  # (bandas, faixas)
    for b_i, b in enumerate(bands):
        for i in range(ok.shape[1]):
            if not ok[b_i, i] and ok[:, i].any():
                j = min(np.flatnonzero(ok[:, i]), key=lambda j: abs(j - b_i))
                b["cols"][i] = bands[j]["cols"][i]
    for b in bands:
        b["cols"] = b["cols"].round().astype(int).tolist()
        del b["cover"]
    # papel de parede: mediana dos pixels lilases (sem ornamento) por banda; filete dourado só visível em 690–800
    wall, gold = [], np.median(a[700:790, 610:628].reshape(-1, 3), 0)
    for y in range(0, h, BAND):
        blk = a[y:y + BAND, :560]
        lil = (blk[..., 2] > blk[..., 0] + 25) & (np.abs(blk[..., 1] - blk[..., 0]) < 14)  # lilás: r≈g (o paletó tem g > r)
        wall.append(np.median(blk[lil], 0).round().astype(int).tolist() if lil.sum() > 50 else wall[-1])
    out = {"edges": EDGES, "band": BAND, "bands": bands, "wall": wall, "gold": gold.round().astype(int).tolist()}
    json.dump(out, open(os.path.join(os.path.dirname(__file__), "a138_bg.json"), "w"), indent=0)
    print("resíduo médio por banda:", [b["res"] for b in bands])
    return out


def rect(x0, y0, x1, y1):
    return {"points": [[x0, y0], [x1, y0], [x1, y1], [x0, y1]], "geometry": "polyline"}


def paint(layer, shapes, rgb):
    d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": F, "layer": layer, "shapes": shapes,
                          "color": {"type": "direct", "rgba": [c / 255 for c in rgb] + [1.0]}})


def run():
    fd = json.load(open(os.path.join(os.path.dirname(__file__), "a138_bg.json")))
    d.apply({"kind": "replace_data", "id": ids["bg_c1"], "data": {"type": "cel", "number": 1}})
    bg = ids["bg"]
    h = 1080
    n = 0
    # faixas da direita: bandas iguais (±1 nível) consecutivas viram um retângulo só
    for i in range(len(EDGES) - 1):
        xa, xb = EDGES[i], EDGES[i + 1]
        if i == 0:
            xa = GOLD_X1
        y0, cur = 0, None
        for b in fd["bands"] + [None]:
            c = None if b is None else b["cols"][i]
            if cur is not None and (c is None or max(abs(p - q) for p, q in zip(c, cur)) > 1):
                paint(bg, [rect(xa, y0, xb, b["y"] if b else h)], cur)
                n += 1
                y0 = b["y"] if b else h
                cur = c
            elif cur is None:
                cur = c
    # papel de parede e filete dourado da esquerda
    y0, cur = 0, None
    for k, c in enumerate(fd["wall"] + [None]):
        if cur is not None and (c is None or max(abs(p - q) for p, q in zip(c, cur)) > 1):
            paint(bg, [rect(0, y0, WALL_X1, k * BAND if c else h)], cur)
            n += 1
            y0, cur = k * BAND, c
        elif cur is None:
            cur = c
    paint(bg, [rect(WALL_X1, 0, GOLD_X1, h)], fd["gold"])
    n += 1
    # desfoque só do BG: camada de efeito logo acima dele
    f = node("bg_foco", "Desfoque do BG", {"type": "layer", "kind": "effect"}, ids["cut"])
    d.apply({"kind": "set_props", "id": f, "data": {"params": {"effects": [{"effect": {"type": "gaussian_blur", "radius": 3 * SIGMA}}]}}})
    d.apply({"kind": "move_node", "id": f, "parent": ids["cut"], "index": 1})
    return n


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "fit"
    if what == "perfil":
        a = np.asarray(Image.open(REF).convert("RGB")).astype(float)
        r = np.median(a[560:660], axis=0)
        print(" ".join(f"{x}:{'%02x%02x%02x' % tuple(r[x].astype(int))}" for x in range(850, 1920, 2)))
    elif what == "fit":
        fit()
    else:
        print("retângulos:", run())
