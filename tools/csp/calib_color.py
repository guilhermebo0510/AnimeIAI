"""Comparador de COR para as folhas dos pincéis que misturam (S25): pincel (nanquim, aquarela, pintura grossa) e mistura.

As medidas da `calib_compare.py` olham a tinta como máscara (preto no branco). Aqui a cor importa: quanto do vermelho o
azul arrasta, o quanto a aquarela clareia e escurece, o borrado dos misturadores. Para cada par referência/nosso:
  - cobertura: IoU das máscaras de tinta (pixel longe do papel branco);
  - dE: diferença média em OKLab (×100) dentro da união das máscaras, com as duas imagens desfocadas (estrutura, não
    o grão exato) e sem desfoque;
  - nas folhas de mistura (`*_mix_*`): a cor média em 4 zonas (faixa só vermelha, vertical só azul, cruzamento, fora)
    e o quanto o azul puxa de vermelho ao passar pelo cruzamento;
  - escore = dE desfocado + 40 × (1 − IoU).
Grava `<nome>_lado.png` (referência | nossa | diferença) e `metricas_cor.json`.

Uso:
    python tools/csp/calib_color.py --ref calibracao/csp/ref/pincel --ours calibracao/csp/ours/pincel --names round-watercolor_mix_s30,... --out DIR
    python tools/csp/calib_color.py --ref ... --ours ... --glob "*_mix_s30.png" --out DIR
"""
import argparse
import fnmatch
import json
import os

import numpy as np
from PIL import Image, ImageFilter

from csp_sheet import COR_A, COR_B, mix_sheet


def load(path):
    return np.asarray(Image.open(path).convert("RGB"), dtype=np.float32) / 255.0


def srgb_to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def oklab(rgb):
    l = srgb_to_linear(rgb)
    m1 = np.array([[0.4122214708, 0.5363325363, 0.0514459929], [0.2119034982, 0.6806995451, 0.1073969566], [0.0883024619, 0.2817188376, 0.6299787005]])
    lms = np.cbrt(np.clip(l @ m1.T, 0, None))
    m2 = np.array([[0.2104542553, 0.7936177850, -0.0040720468], [1.9779984951, -2.4285922050, 0.4505937099], [0.0259040371, 0.7827717662, -0.8086757660]])
    return lms @ m2.T


def ink_mask(rgb, thr=0.04):
    return (1.0 - rgb.min(axis=2)) > thr


def blur(rgb, r):
    im = Image.fromarray((np.clip(rgb, 0, 1) * 255).astype(np.uint8))
    return np.asarray(im.filter(ImageFilter.GaussianBlur(r)), dtype=np.float32) / 255.0


def zones(size):
    """Máscaras das zonas da folha de mistura (faixas horizontais e verticais com meia largura `size/2`)."""
    base, cruz = mix_sheet(size)
    h, w = 700, 900
    yy, xx = np.mgrid[0:h, 0:w]
    hr = max(2.0, size * 0.35)  # miolo da faixa (sem a borda antisserrilhada)
    band = np.zeros((h, w), bool)
    vert = np.zeros((h, w), bool)
    for st in base:
        y = st["points"][0][1]
        band |= (np.abs(yy - y) < hr) & (xx > 70) & (xx < 830)
    for st in cruz:
        x = st["points"][0][0]
        vert |= (np.abs(xx - x) < hr) & (yy > 50) & (yy < 650)
    return {"faixa": band & ~vert, "vertical": vert & ~band, "cruz": band & vert}


def zone_colors(rgb, size):
    z = zones(size)
    return {k: [round(float(v), 3) for v in rgb[m].mean(axis=0)] if m.any() else None for k, m in z.items()}


def compare(ref, ours, name):
    mr, mo = ink_mask(ref), ink_mask(ours)
    union = mr | mo
    inter = mr & mo
    iou = float(inter.sum() / max(1, union.sum()))
    lab_r, lab_o = oklab(ref), oklab(ours)
    de_raw = float(np.linalg.norm(lab_r - lab_o, axis=2)[union].mean() * 100) if union.any() else 0.0
    lab_rb, lab_ob = oklab(blur(ref, 3)), oklab(blur(ours, 3))
    de_blur = float(np.linalg.norm(lab_rb - lab_ob, axis=2)[union].mean() * 100) if union.any() else 0.0
    out = {"iou": round(iou, 3), "dE": round(de_raw, 2), "dE_desfocado": round(de_blur, 2), "escore": round(de_blur + 40 * (1 - iou), 2)}
    if "_mix_" in name:
        size = float(name.rsplit("_s", 1)[1].split(".")[0].replace("_", "."))
        zr, zo = zone_colors(ref, size), zone_colors(ours, size)
        out["zonas_ref"], out["zonas_nossa"] = zr, zo
        # quanto o cruzamento puxa para o vermelho (0 = azul puro, 1 = vermelho puro), pela projeção na reta azul→vermelho
        a, b = np.array(COR_A) / 255.0, np.array(COR_B) / 255.0

        def pull(c):
            if c is None:
                return None
            d = a - b
            return round(float(np.dot(np.array(c) - b, d) / np.dot(d, d)), 3)

        out["puxa_vermelho"] = {"ref": pull(zr["cruz"]), "nossa": pull(zo["cruz"])}
    return out


def side(ref, ours, path):
    diff = np.clip(np.abs(ref - ours) * 2.0, 0, 1)
    h, w, _ = ref.shape
    canvas = np.ones((h, w * 3 + 20, 3), np.float32) * 0.35
    canvas[:, :w] = ref
    canvas[:, w + 10:2 * w + 10] = ours
    canvas[:, 2 * w + 20:] = 1.0 - diff
    Image.fromarray((canvas * 255).astype(np.uint8)).save(path)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--ref", required=True)
    ap.add_argument("--ours", required=True)
    ap.add_argument("--names", default=None, help="arquivos (sem .png), separados por vírgula")
    ap.add_argument("--glob", default=None)
    ap.add_argument("--out", required=True)
    ap.add_argument("--no-images", action="store_true")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    if a.names:
        names = [n if n.endswith(".png") else n + ".png" for n in a.names.split(",") if n]
    else:
        names = sorted(f for f in os.listdir(a.ours) if f.endswith(".png") and not f.endswith("_lado.png") and fnmatch.fnmatch(f, a.glob or "*.png"))
    res = {}
    for n in names:
        rp, op = os.path.join(a.ref, n), os.path.join(a.ours, n)
        if not (os.path.exists(rp) and os.path.exists(op)):
            print(f"faltando: {n}")
            continue
        ref, ours = load(rp), load(op)
        r = compare(ref, ours, n)
        res[n] = r
        extra = f"  cruz ref/nossa {r['puxa_vermelho']['ref']}/{r['puxa_vermelho']['nossa']}" if "puxa_vermelho" in r else ""
        print(f"{n:40s} escore {r['escore']:6.2f}  IoU {r['iou']:.3f}  dE {r['dE']:5.2f}  dE desf {r['dE_desfocado']:5.2f}{extra}")
        if not a.no_images:
            side(ref, ours, os.path.join(a.out, n.replace(".png", "_lado.png")))
    with open(os.path.join(a.out, "metricas_cor.json"), "w", encoding="utf-8") as f:
        json.dump(res, f, ensure_ascii=False, indent=1)
    if res:
        print(f"média do escore: {np.mean([r['escore'] for r in res.values()]):.2f} em {len(res)} folhas")


if __name__ == "__main__":
    main()
