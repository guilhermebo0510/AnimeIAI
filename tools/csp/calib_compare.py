"""Compara a folha de teste desenhada no nosso app com a referência do Clip Studio (mesmos traços, canvas 900x700, 100%).

Para cada caneta e tamanho mede, na referência e no nosso:
    - iou       cobertura: IoU das máscaras de tinta (> 50%), com o melhor deslocamento inteiro de até 2 px (`shift`);
    - tinta     soma da tinta nossa ÷ da referência;
    - corpo     largura do corpo (mediana do perfil de largura) das linhas lenta e rápida, nosso ÷ referência;
    - pontas    erro RMS do perfil de largura no começo e no fim (afinamento), em fração da largura do corpo;
    - borda     distância de 10% a 90% da borda (antisserrilhado/dureza), px;
    - aspereza  desvio-padrão da borda ao longo da linha lenta (canetas ásperas/texturizadas), px;
    - direção   largura do círculo por direção do traço (12 faixas de 15°): erro RMS relativo (caligrafia, marcador chato);
    - rápidos   largura dos traços rápidos curtos, nosso ÷ referência;
    - sobrepos. tinta média na faixa das três linhas sobrepostas e no cruzamento, nosso ÷ referência;
    - centro    linha central subpixel (Steger, `aiai-metrics`): desvio médio da linha central nossa à da referência, px
                (bancada 4.9.6: <= 0,75), e o perfil de espessura w(s) pareado ponto a ponto: R² e erro médio, px
                (R² >= 0,90; com espessura quase constante, erro <= 0,3 px). Precisa do exemplo compilado:
                `cargo build --release -p aiai-metrics --example centerline_pair`.
Grava, por folha, `<caneta>_s<t>_lado.png` (referência | nosso | diferença: vermelho = só na referência, azul = só no nosso)
e um `metricas.json` com tudo.

Uso:
    python tools/csp/calib_compare.py --ours calibracao/csp/ours/caneta [--ref calibracao/csp/ref/caneta]
           [--pens g-pen,milli-pen] [--sizes 3,10,30] [--out calibracao/csp/cmp] [--tag antes]
    python tools/csp/calib_compare.py --ref-only        (só as medidas da referência)
    --ref calibracao/csp/ref/<grupo> --pens ... --sizes 10,30,60 servem aos outros grupos (lápis, pincel, aerógrafo);
    --invert para as borrachas (folha apagada numa camada preta).
"""
import argparse
import json
import math
import os
import subprocess
import sys

import cv2
import numpy as np
from PIL import Image
from scipy.ndimage import map_coordinates

sys.path.insert(0, os.path.dirname(__file__))
from csp_sheet import sheet, W, H  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
PENS = ["g-pen", "real-g-pen", "mapping-pen", "turnip-pen", "calligraphy", "textured-pen", "caneta-rugosa", "milli-pen",
        "felt-pen", "dot-pen", "flat-marker"]


INVERT = False  # borrachas: a folha é desenhada sobre uma camada preta (o "traço" é o branco que aparece)


def load_ink(path):
    """Tinta 0..1 (papel branco, tinta preta; com `--invert`, o apagado sobre o preto)."""
    im = Image.open(path).convert("L")
    if INVERT:
        return np.asarray(im).astype(np.float32) / 255.0
    if im.size != (W, H):
        raise SystemExit(f"{path}: tamanho {im.size}, esperado {(W, H)}")
    return 1.0 - np.asarray(im).astype(np.float32) / 255.0


def dense(points, step=1.0):
    """Polilinha reamostrada a cada `step` px: (pontos Nx2, tangentes Nx2, comprimento acumulado)."""
    p = np.array([[q[0], q[1]] for q in points], dtype=np.float64)
    seg = np.hypot(*(p[1:] - p[:-1]).T)
    keep = np.concatenate([[True], seg > 1e-6])
    p = p[keep]
    seg = np.hypot(*(p[1:] - p[:-1]).T)
    s = np.concatenate([[0.0], np.cumsum(seg)])
    n = max(2, int(s[-1] / step) + 1)
    ss = np.linspace(0, s[-1], n)
    x = np.interp(ss, s, p[:, 0])
    y = np.interp(ss, s, p[:, 1])
    pts = np.stack([x, y], 1)
    t = np.gradient(pts, axis=0)
    t /= np.maximum(np.hypot(t[:, 0], t[:, 1]), 1e-9)[:, None]
    return pts, t, ss


class Sheet:
    """Traços da folha e o mapa de qual traço está mais perto de cada pixel (para medir um traço sem os vizinhos)."""

    def __init__(self, size):
        self.size = size
        self.strokes = sheet(size)
        dists = []
        for st in self.strokes:
            img = np.full((H, W), 255, np.uint8)
            pts = np.array([[q[0], q[1]] for q in st["points"]], np.float64)
            cv2.polylines(img, [np.round(pts * 16).astype(np.int32)], False, 0, 1, cv2.LINE_8, shift=4)
            dists.append(cv2.distanceTransform(img, cv2.DIST_L2, 5))
        self.dist = np.stack(dists)
        self.label = np.argmin(self.dist, 0)
        self.by_name = {st["name"]: i for i, st in enumerate(self.strokes)}

    def own(self, ink, name):
        return np.where(self.label == self.by_name[name], ink, 0.0)

    def width_profile(self, ink, name, ext=None):
        """Largura (soma da tinta pela normal) ao longo do traço, de -ext até o fim + ext (px de arco)."""
        k = self.by_name[name]
        own = self.own(ink, name)
        pts, tan, ss = dense(self.strokes[k]["points"])
        ext = ext if ext is not None else self.size + 4
        # estende as pontas pela tangente para pegar a tampa
        pre = np.arange(-ext, 0, 1.0)
        post = np.arange(1.0, ext + 1, 1.0)
        P = np.concatenate([pts[0] + pre[:, None] * tan[0], pts, pts[-1] + post[:, None] * tan[-1]])
        T = np.concatenate([np.repeat(tan[:1], len(pre), 0), tan, np.repeat(tan[-1:], len(post), 0)])
        S = np.concatenate([pre, ss, ss[-1] + post])
        R = 1.5 * self.size + 6
        off = np.arange(-R, R + 0.25, 0.5)
        nrm = np.stack([-T[:, 1], T[:, 0]], 1)
        X = P[:, 0][:, None] + nrm[:, 0][:, None] * off[None, :] - 0.5
        Y = P[:, 1][:, None] + nrm[:, 1][:, None] * off[None, :] - 0.5
        v = map_coordinates(own, [Y.ravel(), X.ravel()], order=1, mode="constant").reshape(X.shape)
        return S, v.sum(1) * 0.5, ss[-1], T


def body(prof):
    S, w, L, _ = prof
    m = (S > L * 0.2) & (S < L * 0.8)
    return float(np.median(w[m])) if m.any() else 0.0


def end_rms(pr, po, size):
    """Erro RMS dos perfis de largura nas duas pontas (até 3 tamanhos + 10 px para dentro), relativo ao corpo da referência."""
    Sr, wr, Lr, _ = pr
    So, wo, Lo, _ = po
    b = max(body(pr), 0.5)
    span = 3 * size + 10
    out = []
    for start in (True, False):
        if start:
            m = Sr < span
            wo_i = np.interp(Sr[m], So, wo)
        else:
            m = Sr > Lr - span
            wo_i = np.interp(Sr[m] - Lr + Lo, So, wo)
        out.append(float(np.sqrt(np.mean((wo_i - wr[m]) ** 2)) / b))
    return out


def edge_stats(ink, sh):
    """Perfil médio da seção da linha lenta (horizontal): borda 10%→90% (px) e aspereza (desvio-padrão da borda, px)."""
    own = sh.own(ink, "linha_lenta")
    band = own[:, 200:700]
    ys = np.arange(H)
    rows = band[max(0, 50 - int(sh.size * 1.5) - 8):50 + int(sh.size * 1.5) + 9]
    y0 = max(0, 50 - int(sh.size * 1.5) - 8)
    prof = rows.mean(1)
    pk = prof.max()
    if pk <= 0.05:
        return {"edge": None, "rough": None, "peak": float(pk)}
    # borda: posições (subpixel) onde o perfil cruza 10% e 90% do pico em cada lado
    def cross(level, side):
        idx = np.nonzero(prof >= level * pk)[0]
        if len(idx) == 0:
            return None
        i = idx[0] if side < 0 else idx[-1]
        j = i - 1 if side < 0 else i + 1
        if j < 0 or j >= len(prof):
            return float(i)
        a, b = prof[j], prof[i]
        f = (level * pk - a) / (b - a) if b != a else 0.0
        return float(j + (i - j) * f)
    e = []
    for side in (-1, 1):
        c10, c90 = cross(0.1, side), cross(0.9, side)
        if c10 is not None and c90 is not None:
            e.append(abs(c90 - c10))
    # aspereza: posição da borda de cima/baixo (cruzamento de 50%) coluna a coluna, desvio-padrão sem a tendência
    edges = []
    for side in (-1, 1):
        pos = []
        for x in range(band.shape[1]):
            col = rows[:, x]
            idx = np.nonzero(col >= 0.5)[0]
            if len(idx):
                pos.append(idx[0] if side < 0 else idx[-1])
        if len(pos) > 70:
            pos = np.array(pos, float)
            trend = np.convolve(pos, np.ones(61) / 61, mode="same")
            edges.append(float(np.std((pos - trend)[30:-30])))
    return {"edge": float(np.mean(e)) if e else None, "rough": float(np.mean(edges)) if edges else None, "peak": float(pk),
            "profile": [round(float(v), 3) for v in prof], "y0": y0}


def dir_profile(ink, sh):
    """Largura do círculo por direção do traço (0..180°, 12 faixas)."""
    S, w, L, T = sh.width_profile(ink, "circulo", ext=0)
    ang = (np.degrees(np.arctan2(T[:, 1], T[:, 0])) + 180.0) % 180.0
    bins = np.zeros(12)
    for k in range(12):
        m = (ang >= k * 15) & (ang < (k + 1) * 15)
        bins[k] = np.median(w[m]) if m.any() else np.nan
    return bins


def texture_stats(ink, sh):
    """Textura no corpo da linha lenta: densidade (tinta média), contraste do grão (desvio-padrão), espectro radial do
    grão (fração da energia por faixa de frequência, ciclos/px) e partículas (componentes com tinta > 50%: quantas por
    1000 px², área mediana e do percentil 90)."""
    own = sh.own(ink, "linha_lenta")
    half = max(1, int(round(sh.size * 0.3)))
    core = own[50 - half:50 + half, 200:700]
    out = {"densidade": float(core.mean()), "grao": float(core.std())}
    if core.shape[0] >= 4:
        f = np.abs(np.fft.rfft2(core - core.mean())) ** 2
        fy = np.fft.fftfreq(core.shape[0])[:, None]
        fx = np.fft.rfftfreq(core.shape[1])[None, :]
        fr = np.sqrt(fx ** 2 + fy ** 2)
        edges = [0.0, 0.05, 0.1, 0.2, 0.35, 0.71]
        tot = f[fr > 0].sum() + 1e-9
        out["espectro"] = [float(f[(fr > a) & (fr <= b)].sum() / tot) for a, b in zip(edges, edges[1:])]
    else:
        out["espectro"] = None
    band = own[max(0, 50 - int(sh.size * 1.5) - 4):50 + int(sh.size * 1.5) + 5, 200:700]
    n, lab, st, _ = cv2.connectedComponentsWithStats((band > 0.5).astype(np.uint8), connectivity=8)
    areas = st[1:, cv2.CC_STAT_AREA] if n > 1 else np.array([])
    out["particulas"] = {"por_1000px2": float(len(areas) * 1000.0 / band.size), "area_mediana": float(np.median(areas)) if len(areas) else 0.0,
                         "area_p90": float(np.percentile(areas, 90)) if len(areas) else 0.0}
    return out


def blur_map_diff(ref, ours, size):
    """Mapa de tinta desfocado (sigma = tamanho/4): diferença média relativa à tinta da referência. Vale para pincéis
    de grão e de spray, onde a sobreposição pixel a pixel não diz nada."""
    k = max(1.0, size / 4.0)
    a = cv2.GaussianBlur(ref, (0, 0), k)
    b = cv2.GaussianBlur(ours, (0, 0), k)
    return float(np.abs(a - b).sum() / max(a.sum(), 1e-6))


def region_mean(ink, x0, y0, x1, y1):
    return float(ink[y0:y1, x0:x1].mean())


def best_iou(ref, ours, r=2):
    a = ref > 0.5
    best = (-1.0, (0, 0))
    for dy in range(-r, r + 1):
        for dx in range(-r, r + 1):
            b = np.roll(np.roll(ours > 0.5, dy, 0), dx, 1)
            inter = np.logical_and(a, b).sum()
            uni = np.logical_or(a, b).sum()
            iou = inter / uni if uni else 1.0
            if iou > best[0]:
                best = (float(iou), (dx, dy))
    return best


def measure(ink, sh):
    size = sh.size
    m = {}
    m["lenta"] = sh.width_profile(ink, "linha_lenta")
    m["rapida"] = sh.width_profile(ink, "linha_rapida")
    m["rapidos"] = [sh.width_profile(ink, f"rapido_{k}") for k in range(6)]
    m["edge"] = edge_stats(ink, sh)
    m["dir"] = dir_profile(ink, sh)
    gap = max(1.0, size * 0.5)
    m["sobreposta"] = region_mean(ink, 560, int(590 - size), 780, int(590 + 2 * gap + size))
    m["cruz"] = region_mean(ink, 650, 460, 690, 500)
    m["tinta"] = float(ink.sum())
    m["textura"] = texture_stats(ink, sh)
    return m


def compare(ref, ours, size):
    sh = Sheet(size)
    mr, mo = measure(ref, sh), measure(ours, sh)
    iou, shift = best_iou(ref, ours)
    r = {"iou": iou, "shift": shift, "tinta": mo["tinta"] / max(mr["tinta"], 1e-6)}
    r["corpo_ref"] = [body(mr["lenta"]), body(mr["rapida"])]
    r["corpo_nosso"] = [body(mo["lenta"]), body(mo["rapida"])]
    # +0,1 px: linha que quase não aparece (pastel em 3 px) não vira divisão por zero
    r["corpo"] = [(o + 0.1) / (a + 0.1) for o, a in zip(r["corpo_nosso"], r["corpo_ref"])]
    r["pontas"] = end_rms(mr["lenta"], mo["lenta"], size) + end_rms(mr["rapida"], mo["rapida"], size)
    r["borda"] = [mr["edge"]["edge"], mo["edge"]["edge"]]
    r["aspereza"] = [mr["edge"]["rough"], mo["edge"]["rough"]]
    dr, do = mr["dir"], mo["dir"]
    ok = ~(np.isnan(dr) | np.isnan(do))
    r["direcao"] = float(np.sqrt(np.mean(((do[ok] - dr[ok]) / max(np.nanmean(dr), 1e-6)) ** 2))) if ok.any() else None
    r["dir_ref"] = [round(float(v), 2) for v in dr]
    r["dir_nosso"] = [round(float(v), 2) for v in do]
    br = np.mean([body(p) for p in mr["rapidos"]])
    bo = np.mean([body(p) for p in mo["rapidos"]])
    r["rapidos"] = bo / max(br, 1e-6)
    r["sobreposta"] = mo["sobreposta"] / max(mr["sobreposta"], 1e-6)
    r["cruz"] = mo["cruz"] / max(mr["cruz"], 1e-6)
    r["perfil_ref"] = mr["edge"].get("profile")
    r["perfil_nosso"] = mo["edge"].get("profile")
    pr, po = r["perfil_ref"], r["perfil_nosso"]
    r["perfil"] = (float(np.abs(np.array(pr) - np.array(po)).sum() / max(np.sum(pr), 1e-6)) if pr and po and len(pr) == len(po) else None)
    r["mapa"] = blur_map_diff(ref, ours, size)
    tr_, to_ = mr["textura"], mo["textura"]
    r["densidade"] = [tr_["densidade"], to_["densidade"]]
    r["grao"] = [tr_["grao"], to_["grao"]]
    r["espectro"] = (float(np.abs(np.array(tr_["espectro"]) - np.array(to_["espectro"])).sum()) if tr_["espectro"] and to_["espectro"] else None)
    r["particulas"] = [tr_["particulas"], to_["particulas"]]
    return r


CENTERLINE_EXE = os.path.join(ROOT, "app", "target", "release", "examples", "centerline_pair.exe")


def centerline(ref_path, ours_path):
    """Linha central (Steger) das duas folhas pelo exemplo do aiai-metrics; None sem o executável ou nas borrachas."""
    if INVERT or not os.path.exists(CENTERLINE_EXE):
        return None
    out = subprocess.run([CENTERLINE_EXE, ref_path, ours_path, "--json"], capture_output=True, text=True, check=True)
    return json.loads(out.stdout)


def side_by_side(ref, ours, path):
    a, b = ref > 0.5, ours > 0.5
    diff = np.full((H, W, 3), 255, np.uint8)
    both = a & b
    diff[both] = (90, 90, 90)
    diff[a & ~b] = (220, 30, 30)
    diff[b & ~a] = (30, 60, 220)
    g = lambda x: np.repeat(((1 - x) * 255).clip(0, 255).astype(np.uint8)[:, :, None], 3, 2)
    sep = np.full((H, 6, 3), 200, np.uint8)
    Image.fromarray(np.concatenate([g(ref), sep, g(ours), sep, diff], 1)).save(path)


def score(r):
    """Número único para ordenar (0 = igual): cobertura, corpo, pontas, borda, direção e aspereza juntos."""
    textured = bool(r.get("grao")) and r["grao"][0] > 0.04
    # grão/spray: a sobreposição pixel a pixel não diz nada (o mapa desfocado entra no lugar, abaixo)
    s = 0.0 if textured else (1 - r["iou"]) * 2
    s += sum(abs(math.log(max(c, 1e-3))) for c in r["corpo"])
    s += 0.5 * sum(r["pontas"]) / 4
    if r["borda"][0] and r["borda"][1]:
        s += 0.2 * abs(r["borda"][1] - r["borda"][0])
    if r["direcao"] is not None:
        s += r["direcao"]
    if r["aspereza"][0] is not None and r["aspereza"][1] is not None:
        s += 0.3 * abs(r["aspereza"][1] - r["aspereza"][0])
    # textura (lápis, aerógrafo, spray): só pesa quando a referência tem grão
    if textured:
        s += r["mapa"] * 2
        s += abs(r["densidade"][1] - r["densidade"][0]) * 2
        s += abs(r["grao"][1] - r["grao"][0]) * 2
        if r.get("espectro") is not None:
            s += r["espectro"]
    if r.get("perfil") is not None:
        s += 0.5 * r["perfil"]
    return float(s)


def steger_cols(st):
    """Colunas da linha central: desvio médio (px) e R²/erro do perfil w(s); '!' quando reprova a bancada 4.9.6."""
    if not st:
        return f"{'-':>6s} {'-':>13s}"
    c = st["compare"]
    dev = f"{c['deviation_mean']:.2f}" + ("" if c["deviation_ok"] else "!")
    wsp = f"{c['width_r2']:.2f}/{c['width_abs_err']:.2f}" + ("" if c["width_profile_ok"] else "!")
    return f"{dev:>6s} {wsp:>13s}"


def fmt(v, n=2):
    if v is None:
        return "-"
    if isinstance(v, (list, tuple)):
        return "/".join(fmt(x, n) for x in v)
    return f"{v:.{n}f}"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--group", default=None, help="caneta, lapis, aerografo ou borracha: referência, lista, tamanhos e modo")
    ap.add_argument("--ref", default=None)
    ap.add_argument("--ours")
    ap.add_argument("--pens", default=None)
    ap.add_argument("--sizes", default=None)
    ap.add_argument("--out", default=os.path.join(ROOT, "calibracao", "csp", "cmp"))
    ap.add_argument("--tag", default="")
    ap.add_argument("--ref-only", action="store_true")
    ap.add_argument("--no-images", action="store_true")
    ap.add_argument("--invert", action="store_true", help="folhas de borracha (apagado sobre camada preta)")
    ap.add_argument("--textura", action="store_true", help="colunas de textura (lápis, aerógrafo): mapa, densidade, grão, espectro, partículas")
    a = ap.parse_args()
    global INVERT
    if a.group:
        from calib_render import GROUPS
        folder, factory, sizes0 = GROUPS[a.group]
        a.ref = a.ref or os.path.join(ROOT, "calibracao", "csp", "ref", folder)
        a.pens = a.pens or ",".join(factory)
        a.sizes = a.sizes or sizes0
        a.invert = a.invert or a.group == "borracha"
        a.textura = a.textura or a.group in ("lapis", "aerografo")
    a.ref = a.ref or os.path.join(ROOT, "calibracao", "csp", "ref", "caneta")
    a.pens = a.pens or ",".join(PENS)
    a.sizes = a.sizes or "3,10,30"
    INVERT = a.invert
    os.makedirs(a.out, exist_ok=True)
    pens = [p for p in a.pens.split(",") if p]
    sizes = [int(s) for s in a.sizes.split(",")]
    allr = {}
    if a.ref_only:
        for pen in pens:
            for s in sizes:
                sh = Sheet(s)
                m = measure(load_ink(os.path.join(a.ref, f"{pen}_s{s}.png")), sh)
                print(f"{pen:14s} s{s:<2d} corpo {body(m['lenta']):6.2f}/{body(m['rapida']):6.2f}  borda {fmt(m['edge']['edge'])}"
                      f"  aspereza {fmt(m['edge']['rough'])}  dir {fmt([float(v) for v in m['dir']], 1)}")
        return
    if not a.ours:
        raise SystemExit("--ours é obrigatório (ou --ref-only)")
    tex = a.textura
    if tex:
        print(f"{'pincel':20s} {'t':>3s} {'iou':>5s} {'mapa':>5s} {'tinta':>5s} {'corpo L/R':>10s} {'dens r/n':>10s} {'grão r/n':>10s} "
              f"{'espec':>5s} {'perfil':>6s} {'part/1000 r/n':>13s} {'área r/n':>10s} {'score':>6s}")
    else:
        print(f"{'caneta':14s} {'t':>3s} {'iou':>5s} {'desl':>6s} {'tinta':>5s} {'corpo L/R':>10s} {'pontas':>19s} {'borda r/n':>10s} "
              f"{'asp r/n':>10s} {'dir':>5s} {'ráp':>5s} {'sobr':>5s} {'score':>6s} {'centro':>6s} {'w(s) R²/erro':>13s}")
    for pen in pens:
        for s in sizes:
            rp = os.path.join(a.ref, f"{pen}_s{s}.png")
            op = os.path.join(a.ours, f"{pen}_s{s}.png")
            if not os.path.exists(op):
                print(f"{pen:14s} s{s:<2d} (sem render nosso)")
                continue
            ref, ours = load_ink(rp), load_ink(op)
            r = compare(ref, ours, s)
            r["score"] = score(r)
            r["steger"] = centerline(rp, op)
            allr[f"{pen}_s{s}"] = r
            if tex:
                pa = r["particulas"]
                print(f"{pen:20s} {s:3d} {r['iou']:5.3f} {r['mapa']:5.2f} {r['tinta']:5.2f} {fmt(r['corpo']):>10s} {fmt(r['densidade']):>10s} "
                      f"{fmt(r['grao']):>10s} {fmt(r['espectro']):>5s} {fmt(r['perfil']):>6s} "
                      f"{pa[0]['por_1000px2']:6.1f}/{pa[1]['por_1000px2']:<6.1f} {pa[0]['area_mediana']:4.0f}/{pa[1]['area_mediana']:<5.0f} {r['score']:6.3f}")
            else:
                print(f"{pen:14s} {s:3d} {r['iou']:5.3f} {str(tuple(r['shift'])):>6s} {r['tinta']:5.2f} {fmt(r['corpo']):>10s} "
                      f"{fmt(r['pontas']):>19s} {fmt(r['borda'], 1):>10s} {fmt(r['aspereza'], 1):>10s} {fmt(r['direcao']):>5s} "
                      f"{r['rapidos']:5.2f} {r['sobreposta']:5.2f} {r['score']:6.3f} {steger_cols(r.get('steger'))}")
            if not a.no_images:
                side_by_side(ref, ours, os.path.join(a.out, f"{pen}_s{s}_lado.png"))
    name = f"metricas{('_' + a.tag) if a.tag else ''}.json"
    with open(os.path.join(a.out, name), "w", encoding="utf-8") as f:
        json.dump(allr, f, ensure_ascii=False, indent=1)
    if allr:
        print(f"score médio {np.mean([r['score'] for r in allr.values()]):.3f}  ({len(allr)} folhas) -> {os.path.join(a.out, name)}")


if __name__ == "__main__":
    main()
