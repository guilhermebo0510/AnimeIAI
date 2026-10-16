"""Desenho medido (decisão 22): o agente estima a curva; a referência é MEDIDA perto dela (MEASURE_LINE para linhas,
MEASURE_EDGE para bordas de massas); o agente emite o traço com os pontos/larguras medidos. Nada é extraído sozinho."""
import math


def first_list(r):
    if isinstance(r, list):
        return r
    for k in ("measures", "results", "probes", "colors", "samples"):
        if isinstance(r.get(k), list):
            return r[k]
    raise ValueError(f"resposta sem lista: {list(r)}")


def densify(poly, step=6.0, closed=False):
    out = []
    n = len(poly)
    segs = n if closed else n - 1
    for i in range(segs):
        a, b = poly[i], poly[(i + 1) % n]
        L = math.hypot(b[0] - a[0], b[1] - a[1])
        k = max(1, int(L // step))
        for j in range(k):
            t = j / k
            out.append([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
    if not closed:
        out.append(list(poly[-1]))
    return out


def circle(cx, cy, rx, ry=None, n=24, start=0.0):
    ry = rx if ry is None else ry
    return [[cx + rx * math.cos(start + 2 * math.pi * k / n), cy + ry * math.sin(start + 2 * math.pi * k / n)] for k in range(n)]


def _median_filter(vals, closed, win=3):
    n = len(vals)
    out = []
    for i in range(n):
        w = []
        for k in range(-win, win + 1):
            j = i + k
            if closed:
                j %= n
            elif j < 0 or j >= n:
                continue
            if vals[j] is not None:
                w.append(vals[j])
        if not w:
            out.append(None)
        else:
            w.sort()
            out.append(w[len(w) // 2])
    return out


def measure_line(d, ref, pts, closed=False, passes=((14.0, 0.06), (5.0, 0.06)), line_color=None, smooth=0):
    """Centros e larguras da linha da referência ao longo da curva estimada. Devolve (pontos, larguras).
    `line_color` ("#rrggbb"): mede pela distância à cor da tinta (linhas sobre massas escuras)."""
    widths = None
    for search, mc in passes:
        req = {"reference": ref, "points": pts, "search": search, "min_contrast": mc}
        if line_color:
            req["line_color"] = line_color
        r = d.call("MEASURE_LINE", req)
        res = first_list(r)
        ox = _median_filter([(m["center"][0] - m["given"][0]) if m["contrast"] > 0 else None for m in res], closed)
        oy = _median_filter([(m["center"][1] - m["given"][1]) if m["contrast"] > 0 else None for m in res], closed)
        ws = _median_filter([m["width"] if m["contrast"] > 0 else None for m in res], closed)
        pts = [[m["given"][0] + (ox[i] or 0.0), m["given"][1] + (oy[i] or 0.0)] for i, m in enumerate(res)]
        widths = ws
    return smooth_points(pts, closed, smooth), widths


def smooth_points(pts, closed=False, win=2):
    """Média móvel (±win) dos pontos medidos: tira o tremor de compressão da medição; pontas abertas ficam fixas."""
    if win <= 0 or len(pts) < 3:
        return pts
    n = len(pts)
    out = []
    for i in range(n):
        if not closed and (i == 0 or i == n - 1):
            out.append(pts[i]); continue
        acc, k = [0.0, 0.0], 0
        for j in range(i - win, i + win + 1):
            if closed:
                j %= n
            elif j < 0 or j >= n:
                continue
            acc[0] += pts[j][0]; acc[1] += pts[j][1]; k += 1
        out.append([acc[0] / k, acc[1] / k])
    return out


def measure_edge(d, ref, pts, passes=((16.0, 0.03), (6.0, 0.03))):
    for search, ms in passes:
        r = d.call("MEASURE_EDGE", {"reference": ref, "points": pts, "search": search, "min_strength": ms})
        res = first_list(r)
        ox = _median_filter([(m["edge"][0] - m["given"][0]) if m["strength"] > 0 else None for m in res], True)
        oy = _median_filter([(m["edge"][1] - m["given"][1]) if m["strength"] > 0 else None for m in res], True)
        pts = [[m["given"][0] + (ox[i] or 0.0), m["given"][1] + (oy[i] or 0.0)] for i, m in enumerate(res)]
    return pts


def curve_from_measure(pts, widths, color, closed=False, tip="marker", every=2):
    """Curva para DRAW_CURVES: pontos medidos (a cada `every`) e perfil de largura a partir das larguras medidas."""
    ws = [w for w in widths if w]
    base = sorted(ws)[len(ws) // 2] if ws else 3.0
    sel = list(range(0, len(pts), every))
    if not closed and sel[-1] != len(pts) - 1:
        sel.append(len(pts) - 1)
    prof = []
    n = len(sel)
    for k, i in enumerate(sel):
        w = widths[i] or base
        prof.append([k / max(1, n - 1), max(0.2, min(3.0, w / base))])
    return {"points": [pts[i] for i in sel], "width": round(base, 2), "tip": tip, "geometry": "spline",
            "profile": prof, "color": color, "closed": closed}
