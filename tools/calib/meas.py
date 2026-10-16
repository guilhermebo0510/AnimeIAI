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


def _normals(pts, closed=False):
    n = len(pts)
    out = []
    for i in range(n):
        a = pts[(i - 1) % n] if closed else pts[max(0, i - 1)]
        b = pts[(i + 1) % n] if closed else pts[min(n - 1, i + 1)]
        tx, ty = b[0] - a[0], b[1] - a[1]
        L = math.hypot(tx, ty) or 1.0
        out.append((-ty / L, tx / L))
    return out


def anchor_line(d, ref, pts, widths, w_nom, closed=False, search=4.0, strong=0.035, w_range=(8.0, 30.0), clear_side=0.08):
    """Ajusta centro/largura da linha pelas BORDAS medidas dos dois lados (MEASURE_EDGE em p ± n·w/2): com as duas bordas
    fortes, centro = ponto médio e largura = distância; com uma só, ancora nela com a largura nominal."""
    ns = _normals(pts, closed)
    ws = [(w or w_nom) for w in widths]
    plus = [[p[0] + n[0] * w / 2, p[1] + n[1] * w / 2] for p, n, w in zip(pts, ns, ws)]
    minus = [[p[0] - n[0] * w / 2, p[1] - n[1] * w / 2] for p, n, w in zip(pts, ns, ws)]
    rp = first_list(d.call("MEASURE_EDGE", {"reference": ref, "points": plus, "search": search, "min_strength": 0.02}))
    rm = first_list(d.call("MEASURE_EDGE", {"reference": ref, "points": minus, "search": search, "min_strength": 0.02}))
    new_pts, new_w = [], []
    proj, clear = [], []
    for i, (p, n) in enumerate(zip(pts, ns)):
        ep, em = rp[i], rm[i]
        # projeção das bordas na normal (offset relativo ao centro atual)
        op = (ep["edge"][0] - p[0]) * n[0] + (ep["edge"][1] - p[1]) * n[1]
        om = (em["edge"][0] - p[0]) * n[0] + (em["edge"][1] - p[1]) * n[1]
        proj.append((op, om, ep["strength"] >= strong, em["strength"] >= strong))
        clear.append(max(ep["strength"], em["strength"]) >= clear_side)
    # linha sem nenhum lado nítido (cabelo sobre cabelo, sombra): a borda medida não é confiável — fica como está
    both_found = sum(1 for op, om, sp, sm in proj if sp and sm and w_range[0] <= op - om <= w_range[1])
    if sum(clear) < 0.3 * len(clear) and both_found < 0.3 * len(proj):
        return pts, widths
    # largura da própria curva onde as duas bordas são confiáveis: vale para os pontos com um lado só
    ok = lambda op, om: w_range[0] <= op - om <= w_range[1]
    both = sorted(op - om for op, om, sp, sm in proj if sp and sm and ok(op, om))
    w_curve = both[len(both) // 2] if len(both) >= 5 else None
    for i, (p, n, w) in enumerate(zip(pts, ns, ws)):
        op, om, sp, sm = proj[i]
        if w_curve:
            w = w_curve
        if sp and sm and ok(op, om):
            c, wi = (op + om) / 2, op - om
        elif sp:
            c, wi = op - w / 2, w
        elif sm:
            c, wi = om + w / 2, w
        else:
            c, wi = 0.0, w
        new_pts.append([p[0] + n[0] * c, p[1] + n[1] * c])
        new_w.append(wi)
    ox = _median_filter([q[0] - p[0] for q, p in zip(new_pts, pts)], closed)
    oy = _median_filter([q[1] - p[1] for q, p in zip(new_pts, pts)], closed)
    new_pts = [[p[0] + ox[i], p[1] + oy[i]] for i, p in enumerate(pts)]
    return smooth_points(new_pts, closed, 2), _median_filter(new_w, closed)


def edge_offset_line(d, ref, pts, side, half_w, search=6.0, passes=2):
    """Linha cujo lado `side` (+1/−1 na normal) encosta numa massa clara e nítida (pele): mede essa borda com MEASURE_EDGE
    e põe o centro a `half_w` dela, para dentro da linha. Pontas mantêm a posição estimada (normal pouco confiável)."""
    cur = [list(p) for p in pts]
    for _ in range(passes):
        ns = _normals(cur)
        probe = [[p[0] + side * n[0] * half_w, p[1] + side * n[1] * half_w] for p, n in zip(cur, ns)]
        r = first_list(d.call("MEASURE_EDGE", {"reference": ref, "points": probe, "search": search, "min_strength": 0.05}))
        off = []
        for p, n, m in zip(cur, ns, r):
            if m["strength"] <= 0:
                off.append(None)
                continue
            o = (m["edge"][0] - p[0]) * n[0] + (m["edge"][1] - p[1]) * n[1]  # posição da borda na normal
            off.append(o - side * half_w)
        off = _median_filter(off, False)
        nxt = []
        for i, (p, n) in enumerate(zip(cur, ns)):
            o = off[i] if (off[i] is not None and 0 < i < len(cur) - 1) else 0.0
            nxt.append([p[0] + n[0] * o, p[1] + n[1] * o])
        cur = smooth_points(nxt, False, 2)
        search = max(3.0, search / 2)
    return cur
