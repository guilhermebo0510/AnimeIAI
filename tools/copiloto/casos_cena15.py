"""Casos da bancada a partir dos traços desenhados à mão na cena 15 (tracos.json: a lista exata de cada traço).

- rascunho: cada traço redesenhado duas vezes com tremor e lápis fino (o que um animador entrega antes de limpar)
- parcial: só alguns traços (silhueta, perfil); o prompt pede o resto
Saída: caso_<nome>.png (preto sobre branco, 512×512) e caso_<nome>.txt (prompt).
"""
import json, math, os, random, sys
from PIL import Image, ImageDraw

LOG = sys.argv[1]
OUT = sys.argv[2]
os.makedirs(OUT, exist_ok=True)
log = json.load(open(LOG, encoding="utf-8"))
REG = {
    "menino": ((500, 95, 900, 495), "anime boy head in side view, spiky black hair, eye looking down, thick eyebrow, ear, blush lines, sweat drop"),
    "menina": ((1150, 150, 1500, 500), "anime girl head in side view, bob haircut with bangs, big eye, open mouth, ear, hair strands"),
}
PARCIAL = {"menino": ("cabelo menino silhueta", "rosto menino:0:3"), "menina": ("cabeça menina:0:2", "cabeça menina:10:13")}


def spline(pts, n=8):
    """Catmull-Rom pelos pontos (como o pincel com smooth)."""
    if len(pts) < 3:
        return pts
    out = []
    P = [pts[0]] + pts + [pts[-1]]
    for i in range(1, len(P) - 2):
        p0, p1, p2, p3 = P[i - 1], P[i], P[i + 1], P[i + 2]
        for k in range(n):
            t = k / n
            out.append(tuple(0.5 * (2 * p1[j] + (-p0[j] + p2[j]) * t + (2 * p0[j] - 5 * p1[j] + 4 * p2[j] - p3[j]) * t * t + (-p0[j] + 3 * p1[j] - 3 * p2[j] + p3[j]) * t ** 3) for j in range(2)))
    out.append(tuple(pts[-1][:2]))
    return out


def draw(strokes, box, rough, seed):
    x0, y0, x1, y1 = box
    S = 512 / (x1 - x0)
    im = Image.new("L", (512, 512), 255)
    dr = ImageDraw.Draw(im)
    rnd = random.Random(seed)
    for size, s in strokes:
        pts = [(p[0], p[1]) for p in s]
        reps = 2 if rough else 1
        for _ in range(reps):
            j = 2.2 if rough else 0.0
            off = (rnd.uniform(-j, j), rnd.uniform(-j, j))
            q = [((x + off[0] + rnd.uniform(-j, j) * 0.5 - x0) * S, (y + off[1] + rnd.uniform(-j, j) * 0.5 - y0) * S) for x, y in pts]
            q = spline(q)
            w = max(1, round((1.3 if rough else size * 0.75) * S * 0.6))
            dr.line(q, fill=40 if rough else 20, width=w, joint="curve")
    return im


def pick(region_spec):
    name, *rng = region_spec.split(":")
    for r in log:
        if r["region"] == name:
            ss = r["strokes"]
            if rng:
                ss = ss[int(rng[0]):int(rng[1])]
            return [(r["size"], s) for s in ss]
    return []


for nome, (box, prompt) in REG.items():
    x0, y0, x1, y1 = box
    inside = lambda s: any(x0 <= p[0] <= x1 and y0 <= p[1] <= y1 for p in s)
    todos = [(r["size"], s) for r in log for s in r["strokes"] if inside(s)]
    draw(todos, box, True, 1).save(os.path.join(OUT, f"caso_{nome}_rascunho.png"))
    open(os.path.join(OUT, f"caso_{nome}_rascunho.txt"), "w", encoding="utf-8").write(prompt)
    parc = [x for spec in PARCIAL[nome] for x in pick(spec)]
    draw(parc, box, False, 2).save(os.path.join(OUT, f"caso_{nome}_parcial.png"))
    open(os.path.join(OUT, f"caso_{nome}_parcial.txt"), "w", encoding="utf-8").write(prompt)
    # o desenho limpo de referência (para comparar)
    draw(todos, box, False, 3).save(os.path.join(OUT, f"ref_{nome}_limpo.png"))
print("ok", sorted(os.listdir(OUT)))
