"""Revisão (S19) do curta: proporções contra o model sheet (aiai_consistency), cor pintada × settei (ΔE CIE76 nos pontos
de semente do rig, render só da camada de cor) e lint de cada corte (aiai_production lint_cut). Grava o relatório."""
import json, math
from PIL import Image
from curta import d, ids, OUT, ROOT, PALETTE
import rig

rep = {"consistencia": [], "delta_e": [], "lint": []}
# ---------- settei: a linha dos olhos do rig fica a 0,60 cabeça (olhos grandes, mais baixos que a guia genérica 0,55) ----------
for who in ("hana", "ren"):
    ch = d.query({"kind": "node", "id": ids["char_" + who]})["data"]
    m = dict(ch.get("measurements", {}))
    m["landmarks"] = {"olhos": 0.60}
    d.apply({"kind": "set_props", "id": ids["char_" + who], "data": {"measurements": m}})
# ---------- proporções MEDIDAS no desenho renderizado (não nos parâmetros do rig) ----------
# topo da cabeça = primeiro pixel de linha na faixa do personagem; olhos = centro vertical da camada de olhos;
# queixo = a linha mais baixa na coluna do centro do rosto que tem pele em cima e embaixo (a gola tem roupa embaixo).
def layer_png(cut, frame, layer, name):
    path = f"{OUT}/medida_{name}.png"
    d.call("RENDER", {"cut": cut, "frame": frame, "camera": False, "layers": [layer], "out": path})
    return Image.open(path).convert("RGBA")


def measure(cut, frame, layers, cx, H_hint, skin_hex, name):
    """(topo do cabelo, linha dos olhos, queixo) em px, medidos no render das camadas linha/cor/olhos."""
    ln, co, ol = (layer_png(cut, frame, layers[k], f"{name}_{k}") for k in ("linha", "cor", "olhos"))
    W, Hh = ln.size
    band = range(max(0, int(cx - 0.6 * H_hint)), min(W, int(cx + 0.6 * H_hint)))
    top = next((y for y in range(Hh) if any(ln.getpixel((x, y))[3] > 128 for x in band[::2])), None)
    skin = tuple(int(skin_hex[i:i + 2], 16) for i in (1, 3, 5))
    is_skin = lambda px: px[3] > 200 and sum(abs(a - b) for a, b in zip(px[:3], skin)) < 40
    chin, x, y = None, int(cx), int(top or 0)
    while y < min(Hh - 2, int((top or 0) + 1.3 * H_hint)):
        if ln.getpixel((x, y))[3] > 128:
            y0 = y
            while y < Hh - 1 and ln.getpixel((x, y))[3] > 128:
                y += 1
            if is_skin(co.getpixel((x, max(0, y0 - 3)))) and is_skin(co.getpixel((x, min(Hh - 1, y + 2)))):
                chin = (y0 + y) / 2
        y += 1
    # olhos: só o que está na camada de olhos acima do queixo (alças e outros detalhes ficam de fora)
    eye_ys = [yy for yy in range(top or 0, int(chin or Hh)) for xx in band[::3] if ol.getpixel((xx, yy))[3] > 128]
    eyes = sum(eye_ys) / len(eye_ys) if eye_ys else None
    return top, eyes, chin


def ratios(m):
    top, eyes, chin = m
    return None if None in m else {"olhos": (eyes - top) / (chin - top)}


# referência = o próprio model sheet (vista de frente), medido do mesmo jeito; o corte tem de bater com ele
REF = {}
for who in ("hana", "ren"):
    ch = d.query({"kind": "node", "id": ids["char_" + who]})["data"]["measurements"]
    lay = {"linha": ids[f"sheet_{who}_line"], "cor": ids[f"sheet_{who}_fill"], "olhos": ids[f"sh_{who}_olhos"]}
    REF[who] = measure(ids["sheet_" + who], 0, lay, ch["slot_w"] * 0.5, ch["head_px"], PALETTE[who]["pele"], f"sheet_{who}")

DRAWN = [("C003", 2, "c3_hana", "hana", 960, 640), ("C004", 3, "c4_ren", "ren", 960, 600)]
for cut_name, ci, prefix, who, cx, H_hint in DRAWN:
    lay = {k: ids[f"{prefix}_{k}"] for k in ("linha", "cor", "olhos")}
    m = measure(ids["cuts"][ci], 0, lay, cx, H_hint, PALETTE[who]["pele"], prefix)
    rc, rr = ratios(m), ratios(REF[who])
    item = {"corte": cut_name, "quem": who, "medido_px": {"topo": m[0], "olhos": m[1] and round(m[1], 1), "queixo": m[2]},
            "model_sheet_px": {"topo": REF[who][0], "olhos": REF[who][1] and round(REF[who][1], 1), "queixo": REF[who][2]}}
    if rc is None or rr is None:
        item.update({"ok": False, "erro": "medida não encontrada no render"})
    else:
        errs = {k: round(abs(rc[k] - rr[k]) / rr[k] * 100, 2) for k in rc}
        item.update({"razoes_corte": {k: round(v, 4) for k, v in rc.items()}, "razoes_sheet": {k: round(v, 4) for k, v in rr.items()},
                     "errors_pct": errs, "max_error_pct": max(errs.values()), "ok": max(errs.values()) <= 5.0})
    rep["consistencia"].append(item)


# ---------- ΔE: cor do balde × settei ----------
def lab(rgb):
    def lin(c):
        c /= 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    r, g, b = (lin(float(v)) for v in rgb[:3])
    X, Y, Z = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047, 0.2126 * r + 0.7152 * g + 0.0722 * b, (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883
    f = lambda t: t ** (1 / 3) if t > 0.008856 else 7.787 * t + 16 / 116
    return 116 * f(Y) - 16, 500 * (f(X) - f(Y)), 200 * (f(Y) - f(Z))


def hexrgb(h):
    return [int(h[i:i + 2], 16) for i in (1, 3, 5)]


CHECK = [(2, "hana", "c3_hana", (960, 120, 640, -6.0, 0.0), "angry", (1078 - 120) / 640), (3, "ren", "c4_ren", (960, 150, 600, 8.0, 0.0), "angry", (1078 - 150) / 600)]
for ci, who, prefix, (cx, top, H, yaw, roll), mood, bottom in CHECK:
    cut = ids["cuts"][ci]
    path = f"{OUT}/revisao_cor_{prefix}.png"
    d.call("RENDER", {"cut": cut, "frame": 0, "camera": False, "layers": [ids[prefix + "_cor"]], "out": path})
    img = Image.open(path).convert("RGBA")
    _, seeds, _ = rig.bust(rig.Head(cx, top, H, yaw, roll), who, mood, 1.0, "closed", bottom=bottom)
    worst = 0.0
    for s in seeds:
        if s["name"] not in PALETTE[who] or not (0 <= s["x"] < 1920 and 0 <= s["y"] < 1080):
            continue
        # cor dominante dos pixels opacos numa janela 9×9 em volta da semente (a semente pode cair sob a linha)
        from collections import Counter
        win = [img.getpixel((x, y)) for x in range(s["x"] - 4, s["x"] + 5) for y in range(s["y"] - 4, s["y"] + 5) if 0 <= x < 1920 and 0 <= y < 1080]
        win = [p for p in win if p[3] > 250]
        if not win:
            continue
        got = Counter(p[:3] for p in win).most_common(1)[0][0]
        want = hexrgb(PALETTE[who][s["name"]])
        de = math.dist(lab(got), lab(want))
        worst = max(worst, de)
        rep["delta_e"].append({"corte": f"C00{ci + 1}", "cor": s["name"], "ponto": [s["x"], s["y"]], "settei": PALETTE[who][s["name"]], "pintado": "#%02x%02x%02x" % got, "delta_e": round(de, 2)})
    print(f"C00{ci + 1} {who}: ΔE máx {worst:.2f}")

# ---------- lint de cada corte ----------
for i, c in enumerate(ids["cuts"]):
    r = d.call("PRODUCTION", {"op": "lint_cut", "cut": c})
    rep["lint"].append({"corte": f"C00{i + 1}", "resultado": r})
json.dump(rep, open(f"{ROOT}/revisao.json", "w", encoding="utf-8"), ensure_ascii=False, indent=1)
for x in rep["consistencia"]:
    print(x["corte"], x["quem"], x["medido_px"], "erro máx %", x.get("max_error_pct"), "ok" if x["ok"] else "FALHA")
print("ΔE > 2:", [x for x in rep["delta_e"] if x["delta_e"] > 2])
for x in rep["lint"]:
    r = x["resultado"]
    print(x["corte"], json.dumps({k: v for k, v in r.items() if k not in ("_via",)}, ensure_ascii=False)[:300])
