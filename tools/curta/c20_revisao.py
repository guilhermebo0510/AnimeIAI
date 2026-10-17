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
# ---------- proporções: medidas dos desenhos (y em px) ----------
DRAWN = [("C003", "hana", 120, 640), ("C004", "ren", 150, 600), ("C005", "hana", 190, 360), ("C005", "ren", 160, 367.2),
         ("C006", "hana", 300, 170), ("C006", "ren", 215, 170)]
for cut, who, top, H in DRAWN:
    r = d.call("CONSISTENCY", {"character": ids["char_" + who], "head_top": top, "chin": top + H, "eyes": top + 0.60 * H, "shoulders": top + 1.33 * H})
    rep["consistencia"].append({"corte": cut, "quem": who, **{k: r.get(k) for k in ("errors_pct", "max_error_pct", "ok")}})


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
    print(x["corte"], x["quem"], "erro máx %", x["max_error_pct"], "ok" if x["ok"] else "FALHA")
print("ΔE > 2:", [x for x in rep["delta_e"] if x["delta_e"] > 2])
for x in rep["lint"]:
    r = x["resultado"]
    print(x["corte"], json.dumps({k: v for k, v in r.items() if k not in ("_via",)}, ensure_ascii=False)[:300])
