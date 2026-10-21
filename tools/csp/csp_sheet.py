"""Folha de teste padrão de pincel, em coordenadas do canvas (900x700), igual para o Clip Studio e para o AnimeIAI.

Cada traço é uma lista de pontos (x, y, pressão). O espaçamento entre pontos define a velocidade: o injetor de caneta manda
um ponto a cada 8 ms, então 3 px por ponto ≈ 375 px/s (lento) e 15 px por ponto ≈ 1900 px/s (rápido).

Uso:
    python tools/csp/csp_sheet.py 10 > folha.json                         (traços em coordenadas do canvas)
    from csp_sheet import sheet, to_pen_file                              (no script de captura)
"""
import json
import math
import random
import sys

W, H = 900, 700
SLOW, MED, FAST = 3.0, 7.0, 15.0


def _resample(path, step):
    """Pontos igualmente espaçados (step px) ao longo de uma polilinha densa."""
    out = [path[0]]
    acc = 0.0
    for (x0, y0), (x1, y1) in zip(path, path[1:]):
        seg = math.hypot(x1 - x0, y1 - y0)
        t = step - acc
        while t <= seg:
            k = t / seg
            out.append((x0 + (x1 - x0) * k, y0 + (y1 - y0) * k))
            t += step
        acc = (acc + seg) % step
    if out[-1] != path[-1]:
        out.append(path[-1])
    return out


def _line(a, b, n=200):
    return [(a[0] + (b[0] - a[0]) * i / n, a[1] + (b[1] - a[1]) * i / n) for i in range(n + 1)]


def _stroke(name, path, step, p=1.0):
    return {"name": name, "points": [[round(x, 2), round(y, 2), p] for x, y in _resample(path, step)]}


def sheet(size, pressure=1.0):
    """Traços da folha para um pincel de `size` px (o tamanho só muda o afastamento das linhas sobrepostas)."""
    s = []
    s.append(_stroke("linha_lenta", _line((40, 50), (860, 50)), SLOW, pressure))
    s.append(_stroke("linha_rapida", _line((40, 110), (860, 110)), FAST, pressure))
    circ = [(140 + 90 * math.cos(2 * math.pi * i / 400 - math.pi / 2), 260 + 90 * math.sin(2 * math.pi * i / 400 - math.pi / 2)) for i in range(401)]
    s.append(_stroke("circulo", circ, SLOW, pressure))
    scurve = [(280 + 280 * i / 400, 260 - 80 * math.sin(2 * math.pi * i / 400)) for i in range(401)]
    s.append(_stroke("curva_s", scurve, SLOW, pressure))
    zz = []
    for k in range(6):
        a = (600 + 260 * k / 6, 180 if k % 2 == 0 else 340)
        b = (600 + 260 * (k + 1) / 6, 340 if k % 2 == 0 else 180)
        zz += _line(a, b, 60)
    s.append(_stroke("zigue_zague", zz, MED, pressure))
    rnd = random.Random(20261020)
    ctrl = [(rnd.uniform(60, 420), rnd.uniform(410, 550)) for _ in range(9)]
    scrib = []
    for i in range(len(ctrl) - 3):  # Catmull-Rom pelos pontos de controle
        p0, p1, p2, p3 = ctrl[i:i + 4]
        for j in range(40):
            t = j / 40
            t2, t3 = t * t, t * t * t
            scrib.append(tuple(0.5 * ((2 * p1[d]) + (-p0[d] + p2[d]) * t + (2 * p0[d] - 5 * p1[d] + 4 * p2[d] - p3[d]) * t2 + (-p0[d] + 3 * p1[d] - 3 * p2[d] + p3[d]) * t3) for d in (0, 1)))
    s.append(_stroke("rabisco", scrib, MED, pressure))
    s.append(_stroke("cruz_a", _line((500, 410), (840, 550)), MED, pressure))
    s.append(_stroke("cruz_b", _line((500, 550), (840, 410)), MED, pressure))
    gap = max(1.0, size * 0.5)
    for k in range(3):
        y = 590 + gap * k
        s.append(_stroke(f"sobreposta_{k}", _line((500, y), (840, y)), MED, pressure))
    for k in range(6):
        x = 50 + 65 * k
        s.append(_stroke(f"rapido_{k}", _line((x, 690 - 50), (x + 45, 690 - 90)), FAST, pressure))
    return s


COR_A = (220, 40, 40)   # faixas da base (vermelho)
COR_B = (40, 70, 220)   # traços que cruzam por cima (azul)


def mix_sheet(size, pressure=1.0):
    """Teste de mistura: faixas horizontais na cor A e depois traços verticais na cor B por cima, na mesma camada."""
    base = [_stroke(f"faixa_{k}", _line((60, 90 + 130 * k), (840, 90 + 130 * k)), SLOW, pressure) for k in range(5)]
    cruz = [_stroke(f"vertical_{k}", _line((120 + 150 * k, 40), (120 + 150 * k, 660)), SLOW if k < 4 else FAST, pressure) for k in range(5)]
    return base, cruz


def to_pen_file(strokes, origin_x, origin_y, path):
    """Arquivo do tools/harness/pen.ps1: canvas -> tela (zoom 100%, sem rotação)."""
    lines = []
    for st in strokes:
        for x, y, p in st["points"]:
            lines.append(f"{origin_x + x:.2f} {origin_y + y:.2f} {p:.3f}")
        lines.append("up")
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    size = float(sys.argv[1]) if len(sys.argv) > 1 else 10.0
    json.dump({"canvas": [W, H], "size": size, "strokes": sheet(size)}, sys.stdout)
