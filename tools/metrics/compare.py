#!/usr/bin/env python3
"""Relatorio humano lado a lado: referencia | teste | mapa de diferenca | overlay de bordas | numeros.

    uv run --project tools python tools/metrics/compare.py referencia.png teste.png --out pasta
        [--rust-json relatorio.json] [--flip-png mapa_flip.png] [--tol 2] [--look]

IMPORTANTE: os numeros AUTORITATIVOS vem do crate Rust `aiai-metrics` (acao COMPARE do app). Este script e para
olho humano e QA visual. Sem `--rust-json`, ele calcula aproximacoes em Python (SSIM de scikit-image, DeltaE2000 de
scikit-image, Canny do OpenCV, esqueleto de scikit-image, correlacao de fase do OpenCV, k-means de scikit-learn) e usa
o mapa de DeltaE2000 como "mapa de diferenca" (nao ha FLIP em Python aqui). Com `--rust-json`/`--flip-png`
(gerados pelo gancho `harness_compare_files` do crate) o relatorio mostra os numeros e o heatmap FLIP do Rust.

Ideias reaproveitadas de C:\\ANIGO\\scripts\\look_calibration: analise de "look" (`--look`: faixas de tom, dureza da
sombra, linha de contorno, realces, rim, paleta) e a tabela com dica "para onde mexer"; ver look_analysis.py (copia).
Saidas: report.png (mosaico), report.html (autocontido), report.json.
"""
from __future__ import annotations

import argparse
import base64
import html
import io
import json
import os
import sys

import cv2
import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage as ndi
from skimage import color, morphology
from skimage.metrics import structural_similarity
from sklearn.cluster import KMeans

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

TH = {"flip_mean": 0.05, "ssim": 0.95, "de_flat": 5.0, "edge_f1": 0.95, "lw_tol": 0.05, "pal_de": 3.0, "cam_frac": 0.005}


def load_rgb(p):
    return np.array(Image.open(p).convert("RGB"))


def luma(a):
    return 0.2126 * a[..., 0] + 0.7152 * a[..., 1] + 0.0722 * a[..., 2]


def canny(rgb, hi=None):
    """Mesma receita do Rust: gaussiana sigma 1 na luma, limiar alto = max(P95 do gradiente, 10), baixo = 0.4*alto."""
    y = cv2.GaussianBlur(luma(rgb).astype(np.float32), (0, 0), 1.0)
    gx = cv2.Sobel(y, cv2.CV_32F, 1, 0, ksize=3) / 8.0
    gy = cv2.Sobel(y, cv2.CV_32F, 0, 1, ksize=3) / 8.0
    mag = np.hypot(gx, gy)
    if hi is None:
        hi = max(float(np.percentile(mag, 95)), 10.0)
    # cv2.Canny usa Sobel nao normalizado (x8) sobre uint8: escalar os limiares
    e = cv2.Canny(np.clip(y, 0, 255).astype(np.uint8), hi * 0.4 * 8.0, hi * 8.0, apertureSize=3, L2gradient=True)
    return e > 0, hi


def edge_f1(er, et, tol):
    if not er.any() and not et.any():
        return 1.0, 1.0, 1.0
    if not er.any() or not et.any():
        return 0.0, 0.0, 0.0
    dr = cv2.distanceTransform((~er).astype(np.uint8), cv2.DIST_L2, 5)
    dt = cv2.distanceTransform((~et).astype(np.uint8), cv2.DIST_L2, 5)
    p = float((dr[et] <= tol + 1e-4).mean())
    r = float((dt[er] <= tol + 1e-4).mean())
    return (2 * p * r / (p + r) if p + r else 0.0), p, r


def line_width(rgb):
    l = luma(rgb).astype(np.float32)
    local = cv2.GaussianBlur(l, (0, 0), 8.0)
    mask = l < local - np.maximum(12.0, 0.12 * local)
    try:
        mask = morphology.remove_small_objects(mask, max_size=11)  # remove < 12 px (scikit-image >= 0.26)
    except TypeError:
        mask = morphology.remove_small_objects(mask, 12)
    skel = morphology.skeletonize(mask)
    d = ndi.distance_transform_edt(mask)
    w = 2.0 * d[skel] - 1.0
    w = w[(w >= 1.0) & (w <= 30.0)]
    if len(w) == 0:
        return {"median": 0.0, "p10": 0.0, "p90": 0.0, "samples": 0}
    return {"median": float(np.median(w)), "p10": float(np.percentile(w, 10)), "p90": float(np.percentile(w, 90)), "samples": int(len(w))}


def flat_mask(rgb, edges):
    l = luma(rgb).astype(np.float64)
    m1 = cv2.boxFilter(l, -1, (5, 5), borderType=cv2.BORDER_REPLICATE)
    m2 = cv2.boxFilter(l * l, -1, (5, 5), borderType=cv2.BORDER_REPLICATE)
    near = cv2.distanceTransform((~edges).astype(np.uint8), cv2.DIST_L2, 5) <= 2.0
    return ((m2 - m1 * m1) <= 6.0) & ~near


def palette_shift(lab_r, lab_t, mask, k=6):
    pts = lab_r[mask]
    if len(pts) < k:
        return []
    km = KMeans(n_clusters=k, n_init=1, random_state=0).fit(pts[:: max(1, len(pts) // 60000)])
    lab = km.predict(pts)
    out = []
    from skimage.color import deltaE_ciede2000
    tt = lab_t[mask]
    for c in range(k):
        sel = lab == c
        if not sel.any():
            continue
        a, b = pts[sel].mean(0), tt[sel].mean(0)
        rgb = lambda x: [int(v) for v in np.clip(color.lab2rgb(x[None, None, :])[0, 0] * 255 + 0.5, 0, 255)]
        out.append({"a": rgb(a), "b": rgb(b), "fraction": float(sel.mean()), "delta_e": float(deltaE_ciede2000(a[None], b[None])[0])})
    out.sort(key=lambda r: -r["fraction"])
    return out


def python_metrics(ref, test, tol):
    from skimage.color import deltaE_ciede2000
    res = {}
    res["ssim"] = float(structural_similarity(luma(ref), luma(test), gaussian_weights=True, sigma=1.5, use_sample_covariance=False, data_range=255.0))
    lr, lt = color.rgb2lab(ref / 255.0), color.rgb2lab(test / 255.0)
    de = deltaE_ciede2000(lr, lt)
    er, hi = canny(ref)
    et, _ = canny(test, hi)
    flat = flat_mask(ref, er)
    res["delta_e"] = {"overall_mean": float(de.mean()), "flat_mean": float(de[flat].mean()) if flat.any() else 0.0, "flat_fraction": float(flat.mean())}
    f1, p, r = edge_f1(er, et, tol)
    res["edges"] = {"f1": f1, "precision": p, "recall": r, "ref_pixels": int(er.sum()), "test_pixels": int(et.sum())}
    lw_r, lw_t = line_width(ref), line_width(test)
    res["line_width"] = {"reference": lw_r, "test": lw_t, "median_error": (lw_t["median"] - lw_r["median"]) / lw_r["median"] if lw_r["median"] else 0.0}
    gr, gt = [cv2.createHanningWindow((ref.shape[1], ref.shape[0]), cv2.CV_64F) * luma(x) for x in (ref, test)]
    (dx, dy), peak = cv2.phaseCorrelate(gr, gt)
    res["camera"] = {"dx": float(dx), "dy": float(dy), "peak": float(peak), "error_frac_of_width": float(np.hypot(dx, dy) / ref.shape[1])}
    res["palette"] = palette_shift(lr, lt, flat if flat.any() else np.ones(flat.shape, bool))
    res["flip"] = None
    res["_de_map"] = de
    res["_edges"] = (er, et)
    return res


def edge_overlay(ref, er, et, tol):
    dr = cv2.distanceTransform((~er).astype(np.uint8), cv2.DIST_L2, 5)
    dt = cv2.distanceTransform((~et).astype(np.uint8), cv2.DIST_L2, 5)
    out = (255 - (255 - ref.astype(np.float32)) * 0.25).astype(np.uint8)
    both = (er & (dt <= tol)) | (et & (dr <= tol))
    only_ref = er & ~(dt <= tol)
    only_test = et & ~(dr <= tol)
    out[only_ref] = (0, 200, 0)
    out[only_test] = (230, 0, 0)
    out[both] = (255, 220, 0)
    return out


def heat(de_map):
    v = np.clip(de_map / 10.0, 0, 1)
    c = cv2.applyColorMap((v * 255).astype(np.uint8), cv2.COLORMAP_MAGMA)
    return cv2.cvtColor(c, cv2.COLOR_BGR2RGB)


def b64(img: Image.Image, w=640):
    img = img.copy()
    img.thumbnail((w, w))
    buf = io.BytesIO()
    img.save(buf, "PNG")
    return base64.b64encode(buf.getvalue()).decode()


def fmt(v, n=4):
    return "-" if v is None else (f"{v:.{n}f}" if isinstance(v, float) else str(v))


def rows_from(m, source):
    f = m.get("flip")
    lw = m["line_width"]
    pal = m.get("palette") or []
    pal_max = max((p["delta_e"] for p in pal), default=None)
    cam = m["camera"]
    rows = [
        ("FLIP medio" + ("" if f else " (n/d em Python)"), f["mean"] if f else None, "<= %.2f" % TH["flip_mean"], (f["mean"] <= TH["flip_mean"]) if f else None),
        ("SSIM (luma, gauss. 11)", m["ssim"], ">= %.2f" % TH["ssim"], m["ssim"] >= TH["ssim"]),
        ("DeltaE2000 medio, regioes chapadas", m["delta_e"]["flat_mean"], "<= %.1f" % TH["de_flat"], m["delta_e"]["flat_mean"] <= TH["de_flat"]),
        ("DeltaE2000 medio, imagem toda", m["delta_e"]["overall_mean"], "-", None),
        ("F1 de bordas @%gpx" % m.get("tol", 2), m["edges"]["f1"], ">= %.2f" % TH["edge_f1"], m["edges"]["f1"] >= TH["edge_f1"]),
        ("  precisao / revocacao", "%.3f / %.3f" % (m["edges"]["precision"], m["edges"]["recall"]), "-", None),
        ("Espessura mediana ref / teste (px)", "%.2f / %.2f" % (lw["reference"]["median"], lw["test"]["median"]), "-", None),
        ("Erro da espessura mediana", lw["median_error"], "|.| <= %.2f" % TH["lw_tol"], abs(lw["median_error"]) <= TH["lw_tol"]),
        ("Pior DeltaE por cor (paleta)", pal_max, "<= %.1f" % TH["pal_de"], (pal_max <= TH["pal_de"]) if pal_max is not None else None),
        ("Camera (dx, dy) px / pico", "%.2f, %.2f / %.2f" % (cam["dx"], cam["dy"], cam["peak"]), "<= %.1f%% da largura" % (TH["cam_frac"] * 100), cam["error_frac_of_width"] <= TH["cam_frac"]),
    ]
    return rows


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("reference")
    ap.add_argument("test")
    ap.add_argument("--out", required=True)
    ap.add_argument("--tol", type=float, default=2.0)
    ap.add_argument("--rust-json", help="relatorio do crate Rust (numeros autoritativos)")
    ap.add_argument("--flip-png", help="heatmap FLIP gerado pelo Rust")
    ap.add_argument("--look", action="store_true", help="inclui a analise de look do ANIGO (faixas de tom, linha, realces...)")
    a = ap.parse_args(argv)
    os.makedirs(a.out, exist_ok=True)

    ref, test = load_rgb(a.reference), load_rgb(a.test)
    if ref.shape != test.shape:
        print(f"aviso: tamanhos diferentes {ref.shape} vs {test.shape}; teste redimensionado", file=sys.stderr)
        test = np.array(Image.fromarray(test).resize((ref.shape[1], ref.shape[0]), Image.LANCZOS))
    m = python_metrics(ref, test, a.tol)
    m["tol"] = a.tol
    de_map, (er, et) = m.pop("_de_map"), m.pop("_edges")
    source = "Python (aproximacao)"
    if a.rust_json:
        r = json.load(open(a.rust_json, encoding="utf-8"))
        m.update({k: r[k] for k in ("flip", "ssim", "delta_e", "edges", "line_width", "palette", "camera") if k in r})
        m["tol"] = r.get("edge_tolerance_px", a.tol)
        source = "Rust aiai-metrics (autoritativo)"
    heat_img = Image.open(a.flip_png).convert("RGB") if a.flip_png else Image.fromarray(heat(de_map))
    heat_label = "FLIP (Rust)" if a.flip_png else "DeltaE2000 (0..10) - Python"
    ov = Image.fromarray(edge_overlay(ref, er, et, a.tol))

    look = None
    if a.look:
        try:
            import look_analysis as la
            look = {"reference": la.analyze(a.reference), "test": la.analyze(a.test)}
        except SystemExit as e:
            look = {"erro": str(e)}

    rows = rows_from(m, source)
    ok_all = all(r[3] for r in rows if r[3] is not None)

    # mosaico PNG
    tw = 480
    th = int(tw * ref.shape[0] / ref.shape[1])
    panels = [("referencia", Image.fromarray(ref)), ("teste", Image.fromarray(test)), (heat_label, heat_img), ("bordas: verde=so ref, vermelho=so teste, amarelo=ambos", ov)]
    sheet = Image.new("RGB", (tw * 4, th + 20 + 18 * (len(rows) + 2)), (24, 24, 24))
    d = ImageDraw.Draw(sheet)
    for i, (lab, im) in enumerate(panels):
        sheet.paste(im.resize((tw, th), Image.LANCZOS), (i * tw, 20))
        d.text((i * tw + 4, 4), lab, fill=(255, 255, 0))
    y = th + 26
    d.text((4, y), f"fonte dos numeros: {source}  |  veredito: {'APROVADO' if ok_all else 'REPROVADO'} nas metricas calculadas", fill=(255, 255, 255))
    for k, r in enumerate(rows):
        mark = "  " if r[3] is None else ("OK" if r[3] else "XX")
        d.text((4, y + 18 * (k + 1)), f"{mark} {r[0]:44s} {fmt(r[1]):>16}   limiar {r[2]}", fill=(255, 255, 255) if r[3] in (None, True) else (255, 120, 120))
    sheet.save(os.path.join(a.out, "report.png"))

    # HTML autocontido
    tr = "".join(
        f"<tr class='{'' if r[3] is None else ('ok' if r[3] else 'bad')}'><td>{html.escape(r[0])}</td><td>{html.escape(fmt(r[1]))}</td><td>{html.escape(r[2])}</td><td>{'' if r[3] is None else ('passa' if r[3] else 'falha')}</td></tr>"
        for r in rows
    )
    pal = "".join(
        f"<span class='sw' style='background:rgb({p['a'][0]},{p['a'][1]},{p['a'][2]})'></span><span class='sw' style='background:rgb({p['b'][0]},{p['b'][1]},{p['b'][2]})'></span> dE {p['delta_e']:.2f} ({p.get('fraction', p.get('a_fraction', 0)) * 100:.0f}%)<br>"
        for p in (m.get("palette") or [])
    )
    imgs = "".join(f"<figure><img src='data:image/png;base64,{b64(im)}'><figcaption>{html.escape(l)}</figcaption></figure>" for l, im in panels)
    page = f"""<!doctype html><meta charset=utf-8><title>Comparacao</title>
<style>body{{font:14px system-ui;background:#181818;color:#eee;margin:16px}}figure{{display:inline-block;margin:4px}}img{{width:460px}}
table{{border-collapse:collapse}}td{{padding:2px 10px;border-bottom:1px solid #333}}tr.bad td{{color:#f88}}tr.ok td:last-child{{color:#8f8}}
.sw{{display:inline-block;width:18px;height:18px;margin-right:2px;vertical-align:middle}}</style>
<h2>Referencia x teste</h2><p>Numeros: {html.escape(source)}. Veredito: <b>{'APROVADO' if ok_all else 'REPROVADO'}</b></p>{imgs}
<table>{tr}</table><h3>Cores do settei (referencia | teste)</h3>{pal}"""
    open(os.path.join(a.out, "report.html"), "w", encoding="utf-8").write(page)
    out_json = {"source": source, "metrics": m, "rows": [[r[0], r[1] if not isinstance(r[1], float) else round(r[1], 6), r[2], r[3]] for r in rows], "look": look}
    json.dump(out_json, open(os.path.join(a.out, "report.json"), "w", encoding="utf-8"), indent=1, ensure_ascii=False, default=float)
    print(f"{'APROVADO' if ok_all else 'REPROVADO'}  ({source})  -> {a.out}")
    for r in rows:
        print(f"  {r[0]:44s} {fmt(r[1]):>18}  {r[2]}")
    return 0 if ok_all else 1


if __name__ == "__main__":
    sys.exit(main())
