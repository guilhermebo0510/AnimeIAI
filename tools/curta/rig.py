"""Rig de desenho dos personagens do curta (desenhado à mão pelo agente, sem IA generativa).

Cada parte é escrita em unidades de cabeça (altura topo→queixo = 1; origem no topo da cabeça, x para a direita, y para
baixo) e projetada por `Head(cx, top, H, yaw)`: `yaw` em graus gira o rosto em torno do eixo vertical (0 = frente,
+ = olha para a direita da tela). Os traços saem como CurveSpec do DRAW_CURVES; as sementes de cor saem com o nome da
cor do settei para o FILL.
"""
import math

INK = {"hana": "#3a2420", "ren": "#2a2026"}


def lerp(a, b, t):
    return a + (b - a) * t


class Head:
    def __init__(self, cx, top, H, yaw=0.0, roll=0.0):
        self.cx, self.top, self.H, self.yaw, self.roll = cx, top, H, math.radians(yaw), math.radians(roll)
        self.w = max(1.6, H * 0.014)  # largura base do traço

    def p(self, x, y, z=None, rot=True):
        """Ponto da cabeça: x em unidades de cabeça; z (profundidade na esfera) estimada se None. `rot=False`: ponto do
        contorno da cabeça (silhueta de uma esfera não muda com o giro) ou do eixo (pescoço)."""
        R = 0.40
        if z is None:
            z = math.sqrt(max(0.0, R * R - min(x * x, R * R)))  # superfície frontal
        xs = x * math.cos(self.yaw) + z * math.sin(self.yaw) if rot else x
        X, Y = self.cx + xs * self.H, self.top + y * self.H
        if self.roll:
            ox, oy = self.cx, self.top + 0.5 * self.H
            c, s = math.cos(self.roll), math.sin(self.roll)
            X, Y = ox + (X - ox) * c - (Y - oy) * s, oy + (X - ox) * s + (Y - oy) * c
        return [round(X, 1), round(Y, 1)]

    def facing(self, x):
        """Quanto o ponto x da frente do rosto está voltado para a câmera (0..1): encolhe o olho do lado distante."""
        R = 0.40
        a = math.asin(max(-1.0, min(1.0, x / R)))
        return max(0.15, math.cos(a + self.yaw))

    def curve(self, pts, w=1.0, color="#000000", closed=False, tip="g_pen", widths=None, geometry="spline", rot=True):
        c = {"points": [self.p(q[0], q[1], None, q[2] if len(q) > 2 else rot) for q in pts], "width": round(self.w * w, 2), "tip": tip, "geometry": geometry,
             "color": {"type": "direct", "rgba": hexrgba(color)}, "closed": closed}
        if widths:
            c["widths"] = widths
        return c

    def seed(self, x, y, name, rot=True):
        X, Y = self.p(x, y, None, rot)
        return {"x": int(X), "y": int(Y), "name": name}


def hexrgba(h, a=1.0):
    return [int(h[i:i + 2], 16) / 255 for i in (1, 3, 5)] + [a]


# ------------------------------------------------------------------------------------------------- rosto (comum)

def face_outline(h, ink, chin=1.0, cheek=0.37):
    """Contorno do rosto: têmpora → bochecha → maxilar → queixo, dos dois lados (o lado oculto encolhe com o yaw)."""
    side = lambda s: [(s * 0.39, 0.40), (s * 0.385, 0.55), (s * cheek, 0.66), (s * 0.29, 0.84), (s * 0.12, 0.97 * chin), (0.0, chin)]
    left, right = side(-1), side(1)
    # maxilar projetado (pontos densos, da esquerda para a direita) para o pescoço achar onde encosta
    dense = []
    chain = left[2:] + list(reversed(right[2:]))[1:]
    for a, b in zip(chain, chain[1:]):
        for k in range(8):
            t = k / 8
            dense.append(h.p(lerp(a[0], b[0], t), lerp(a[1], b[1], t)))
    h.jaw = sorted(dense)
    return [h.curve(left, 1.1, ink, widths=[0.4, 1.1, 0.9]), h.curve(right, 1.1, ink, widths=[0.4, 1.1, 0.9])]


def eye(h, ink, side, iris_col, open_=1.0, mood="neutral", female=True):
    """Olho de anime. Devolve (traços, formas pintadas): as formas (branco, íris, pupila) vão numa camada raster abaixo
    da linha, então não dependem do balde. `open_` 0 = fechado, 1 = aberto; `mood`: neutral | angry | happy | surprised."""
    s = side  # -1 = olho à esquerda da tela, +1 = direita
    ex, ey = s * 0.175, 0.60
    hw = 0.105 if female else 0.095
    f = h.facing(ex)
    out, shapes = [], []
    if mood == "happy" and open_ > 0.3:
        out.append(h.curve([(ex - s * hw, ey + 0.01), (ex, ey - 0.045), (ex + s * hw, ey + 0.012)], 1.6, ink, widths=[0.5, 1.4, 0.6]))
        return out, shapes
    if open_ < 0.15:
        out.append(h.curve([(ex - s * hw, ey + 0.005), (ex, ey + 0.03), (ex + s * hw, ey)], 1.5, ink, widths=[0.5, 1.3, 0.7]))
        return out, shapes
    top_h = (0.075 if female else 0.06) * open_ + 0.004
    lift = -0.02 if mood == "angry" else (0.015 if mood == "surprised" else 0.0)
    inner_drop = 0.022 if mood == "angry" else 0.0
    bot = 0.085 * open_ + 0.004
    # contorno do branco do olho: cílios (de dentro para fora) e pálpebra de baixo de volta
    upper = [(ex - s * hw, ey - 0.002 + inner_drop), (ex - s * hw * 0.45, ey - top_h + lift + inner_drop * 0.6),
             (ex + s * hw * 0.45, ey - top_h + lift * 0.5 + 0.003), (ex + s * hw * 1.0, ey - 0.012)]
    lower = [(ex + s * hw * 0.95, ey + bot * 0.55), (ex + s * hw * 0.4, ey + bot), (ex - s * hw * 0.35, ey + bot * 0.95), (ex - s * hw * 0.9, ey + bot * 0.35)]
    shapes.append(("branco_olho", [h.p(*q) for q in upper + lower]))
    # íris e pupila (limitadas à altura do olho)
    ir_rx, ir_ry = (0.056 if female else 0.05) * f, min((0.074 if female else 0.066) * open_, top_h + bot * 0.9)
    icx, icy = ex + s * 0.004, ey + 0.01
    if mood == "surprised":
        ir_rx, ir_ry = ir_rx * 0.72, ir_ry * 0.76
    clamp_y = lambda y: max(ey - top_h + lift * 0.5 + 0.006, min(ey + bot - 0.004, y))
    ring = [(icx + ir_rx * math.cos(t), clamp_y(icy + ir_ry * math.sin(t))) for t in [k * math.tau / 16 for k in range(16)]]
    shapes.append(("olho", [h.p(*q) for q in ring]))
    pup = [(icx + ir_rx * 0.48 * math.cos(t), clamp_y(icy + 0.006 + ir_ry * 0.5 * math.sin(t))) for t in [k * math.tau / 12 for k in range(12)]]
    shapes.append(("pupila", [h.p(*q) for q in pup]))
    # meia-lua mais clara no fundo da íris
    shapes.append(("olho_claro", [h.p(icx + ir_rx * 0.75 * math.cos(t), clamp_y(icy + ir_ry * 0.55 + ir_ry * 0.32 * math.sin(t))) for t in [k * math.tau / 12 for k in range(12)]]))
    # linhas: cílios grossos com a ponta de fora levantada, pálpebra de baixo fina, contorno fino da íris
    if female:
        out.append(h.curve(upper + [(ex + s * hw * 1.18, ey - 0.038)], 2.6, ink, widths=[0.5, 1.5, 0.7]))
    else:
        out.append(h.curve(upper, 2.0, ink, widths=[0.6, 1.4, 0.9]))
    out.append(h.curve(lower[:3], 0.9, ink, widths=[0.3, 1.0, 0.4]))
    out.append(h.curve(ring[9:16] + ring[:2], 0.5, ink, tip="marker"))
    return out, shapes


def eye_highlights(h, side, open_=1.0, mood="neutral"):
    """Brilhos (formas brancas por cima, camada própria): um grande em cima, um pequeno embaixo."""
    if open_ < 0.15 or mood == "happy":
        return []
    s = side
    ex, ey = s * 0.175 + s * 0.004, 0.60 + 0.012
    f = h.facing(s * 0.175)
    big = [(ex - 0.02 * f + 0.016 * math.cos(t) * f, ey - 0.03 * open_ + 0.018 * math.sin(t) * open_) for t in [k * math.tau / 8 for k in range(8)]]
    small = [(ex + 0.022 * f + 0.008 * math.cos(t) * f, ey + 0.035 * open_ + 0.008 * math.sin(t) * open_) for t in [k * math.tau / 6 for k in range(6)]]
    return [[h.p(*q) for q in big], [h.p(*q) for q in small]]


def brows(h, ink, mood="neutral", female=True):
    out = []
    for s in (-1, 1):
        bx = s * 0.17
        y0 = 0.475
        if mood == "angry":
            pts = [(bx - s * 0.085, y0 + 0.03), (bx, y0 - 0.0), (bx + s * 0.09, y0 - 0.015)]
        elif mood == "surprised":
            pts = [(bx - s * 0.08, y0 - 0.02), (bx, y0 - 0.05), (bx + s * 0.09, y0 - 0.03)]
        elif mood == "happy":
            pts = [(bx - s * 0.08, y0 - 0.0), (bx, y0 - 0.028), (bx + s * 0.09, y0 - 0.008)]
        else:
            pts = [(bx - s * 0.085, y0 + 0.005), (bx, y0 - 0.02), (bx + s * 0.09, y0 - 0.005)]
        out.append(h.curve(pts, 1.1 if female else 1.5, ink, widths=[0.6, 1.1, 0.4]))
    return out


def nose(h, ink):
    return [h.curve([(0.01, 0.745), (0.022, 0.775), (0.004, 0.785)], 0.8, ink, widths=[0.3, 1.0, 0.5])]


MOUTHS = ("closed", "a", "i", "u", "e", "o", "smile", "shout", "small_o")


def mouth(h, ink, shape="closed", y=0.875):
    """Boca: closed | a | i | u | e | o | smile | shout | small_o. Devolve (traços, sementes)."""
    cx = 0.0
    if shape == "closed":
        return [h.curve([(cx - 0.045, y), (cx, y + 0.006), (cx + 0.045, y - 0.002)], 1.0, ink, widths=[0.4, 1.0, 0.4])], []
    if shape == "smile":
        return [h.curve([(cx - 0.06, y - 0.012), (cx, y + 0.014), (cx + 0.06, y - 0.012)], 1.0, ink, widths=[0.4, 1.0, 0.4])], []
    dims = {"a": (0.05, 0.055), "i": (0.055, 0.02), "u": (0.025, 0.03), "e": (0.05, 0.035), "o": (0.035, 0.05),
            "shout": (0.065, 0.075), "small_o": (0.02, 0.022)}[shape]
    rx, ry = dims
    if shape in ("a", "shout", "e"):
        # boca aberta de anime: topo reto, fundo arredondado
        pts = [(cx - rx, y - ry * 0.25), (cx + rx, y - ry * 0.25), (cx + rx * 0.6, y + ry * 0.6), (cx, y + ry * 0.85), (cx - rx * 0.6, y + ry * 0.6)]
    else:
        pts = [(cx + rx * math.cos(t), y + ry * 0.2 + ry * 0.6 * math.sin(t)) for t in [k * math.tau / 8 for k in range(8)]]
    out = [h.curve(pts, 0.9, ink, closed=True, tip="marker")]
    seeds = [h.seed(cx, y + ry * 0.35, "boca")]
    if shape in ("a", "shout"):
        # língua
        out.append(h.curve([(cx - rx * 0.45, y + ry * 0.55), (cx, y + ry * 0.35), (cx + rx * 0.45, y + ry * 0.55)], 0.6, ink, tip="marker"))
        seeds.append(h.seed(cx, y + ry * 0.68, "lingua"))
    return out, seeds


def jaw_y(h, X):
    """y (em unidades de cabeça) do maxilar projetado na coluna de tela X (px); um pouco acima para cruzar a linha."""
    pts = getattr(h, "jaw", None)
    if not pts:
        return 0.95
    best = min(pts, key=lambda q: abs(q[0] - X))
    return (best[1] - h.top) / h.H - 0.012


def neck(h, ink, female=True):
    """Pescoço no eixo da cabeça (não gira); cada lado começa onde encontra o maxilar projetado."""
    nw = 0.085 if female else 0.105
    out = []
    for sg in (-1, 1):
        X = h.p(sg * nw, 1.0, None, False)[0]
        y0 = jaw_y(h, X)
        out.append(h.curve([(sg * nw, y0), (sg * nw * 1.03, 1.12)], 0.9, ink, widths=[0.5, 1.0, 1.0], rot=False))
    h.neck_top = [jaw_y(h, h.p(sg * nw, 1.0, None, False)[0]) for sg in (-1, 1)]
    return out


# ------------------------------------------------------------------------------------------------- cabelo

def hair_hana(h, ink):
    """Bob curto castanho-avermelhado: silhueta, franja com borda de mechas (fecha a massa do cabelo), mechas internas,
    presilha verde à esquerda. A borda da franja termina nas têmporas, onde começa o contorno do rosto."""
    out, seeds = [], []
    sil = [(-0.43, 0.94), (-0.51, 0.70), (-0.54, 0.42), (-0.48, 0.15), (-0.30, -0.03), (0.02, -0.08), (0.31, -0.03), (0.49, 0.15),
           (0.55, 0.42), (0.52, 0.70), (0.43, 0.94)]
    out.append(h.curve(sil, 1.3, ink, widths=[0.5, 1.2, 0.5], rot=False))
    # borda de baixo da franja: mechas com lados curvos que afinam até a ponta (vales mais altos, pontas acima das
    # sobrancelhas); cada lado é um traço, as pontas se encontram (fecha a massa do cabelo)
    valleys = [(-0.39, 0.40), (-0.29, 0.30), (-0.16, 0.31), (-0.04, 0.29), (0.09, 0.27), (0.22, 0.29), (0.32, 0.31), (0.39, 0.40)]
    tips = [(-0.335, 0.455), (-0.215, 0.445), (-0.095, 0.47), (0.03, 0.445), (0.165, 0.44), (0.28, 0.43), (0.37, 0.44)]
    for i, t in enumerate(tips):
        for v, sgn in ((valleys[i], -1), (valleys[i + 1], 1)):
            # lado da mecha: sai do vale abaulado para fora e chega reto na ponta
            dx, dy = t[0] - v[0], t[1] - v[1]
            n = (math.hypot(dx, dy) or 1.0)
            ox, oy = -dy / n * sgn * -0.018, dx / n * sgn * -0.018
            c1 = (v[0] + dx * 0.3 + ox, v[1] + dy * 0.3 + oy)
            c2 = (v[0] + dx * 0.7 + ox * 0.5, v[1] + dy * 0.7 + oy * 0.5)
            out.append(h.curve([v, c1, c2, t], 1.05, ink, widths=[0.35, 1.0, 0.12]))
    # sombra do cabelo (cel): por dentro das laterais do bob, junto do rosto
    shadow = []
    for sg in (-1, 1):
        shadow.append([h.p(sg * 0.39, 0.40), h.p(sg * 0.385, 0.62), h.p(sg * 0.38, 0.84), h.p(sg * 0.43, 0.94, None, False), h.p(sg * 0.47, 0.88, None, False),
                       h.p(sg * 0.47, 0.62, None, False), h.p(sg * 0.45, 0.42, None, False)])
    h.shadow_shapes = [("cabelo_sombra", sh) for sh in shadow]
    # lados do bob: da têmpora até a ponta, por dentro (encosta na bochecha) e a ponta curvando para dentro
    for sg in (-1, 1):
        out.append(h.curve([(sg * 0.39, 0.40), (sg * 0.385, 0.55), (sg * 0.375, 0.68), (sg * 0.38, 0.84), (sg * 0.415, 0.92, False), (sg * 0.43, 0.94, False)], 1.0, ink, widths=[0.6, 1.0, 0.5]))
    # mechas internas (traços soltos, não fecham região)
    for a, b in [((-0.30, 0.05), (-0.36, 0.30)), ((-0.12, -0.02), (-0.16, 0.28)), ((0.12, -0.04), (0.10, 0.22)), ((0.30, 0.03), (0.30, 0.25)),
                 ((-0.47, 0.50), (-0.45, 0.80)), ((0.48, 0.50), (0.47, 0.80))]:
        out.append(h.curve([a, ((a[0] + b[0]) / 2 + 0.01, (a[1] + b[1]) / 2), b], 0.55, ink, widths=[0.2, 1.0, 0.2], rot=abs(a[0]) < 0.45))
    # presilha verde (retângulo inclinado) à esquerda, sobre a franja
    clip = [(-0.35, 0.22), (-0.21, 0.17), (-0.195, 0.215), (-0.335, 0.265)]
    out.append(h.curve(clip, 0.9, ink, closed=True, geometry="polyline", tip="marker"))
    seeds += [h.seed(-0.27, 0.218, "presilha"), h.seed(0.02, 0.10, "cabelo", rot=False), h.seed(-0.47, 0.72, "cabelo", rot=False), h.seed(0.48, 0.72, "cabelo", rot=False)]
    return out, seeds


def face_hana(h, mood="neutral", open_=1.0, mouth_shape="closed"):
    """Rosto inteiro de Hana: (traços, sementes, brilhos)."""
    ink = INK["hana"]
    strokes, seeds = [], []
    strokes += face_outline(h, ink)
    shapes = []
    for s in (-1, 1):
        e, sh = eye(h, ink, s, "#3f8a55", open_, mood, True)
        strokes += e
        shapes += sh
    strokes += brows(h, ink, mood)
    strokes += nose(h, ink)
    m, ms = mouth(h, ink, mouth_shape)
    strokes += m
    seeds += ms
    strokes += neck(h, ink)
    strokes.append(h.curve([(-0.32, 1.16), (-0.09, 1.11), (0.0, 1.18), (0.09, 1.11), (0.32, 1.16)], 1.0, ink, geometry="polyline", rot=False))  # gola
    hs, hsd = hair_hana(h, ink)
    strokes += hs
    seeds += hsd
    seeds += [h.seed(0.25, 0.75, "pele"), h.seed(-0.25, 0.75, "pele"), h.seed(0.0, 1.05, "pele", rot=False)]
    shapes += getattr(h, "shadow_shapes", [])
    xs = [h.p(x, 1.0, None, False)[0] for x in (-0.092, -0.046, 0.0, 0.046, 0.092)]
    top = [[X, h.top + (jaw_y(h, X) + 0.012) * h.H] for X in xs]
    shapes.append(("pele_sombra", top + [h.p(0.094, 1.03, None, False), h.p(0.0, 1.07, None, False), h.p(-0.094, 1.03, None, False)]))
    shapes += [("brilho", q) for q in eye_highlights(h, -1, open_, mood) + eye_highlights(h, 1, open_, mood)]
    return strokes, seeds, shapes


def hair_ren(h, ink):
    """Cabelo preto curto e espetado com bandana branca na testa; mechas curtas caem sobre a testa por baixo da bandana.
    Laterais curtas cobrem as orelhas e fecham no rosto na altura da maçã."""
    out, seeds = [], []
    # silhueta espetada (sem giro), da lateral esquerda (perto da bochecha) à direita
    sil = [(-0.375, 0.64, False), (-0.45, 0.60, False), (-0.47, 0.50, False), (-0.53, 0.40, False), (-0.49, 0.30, False), (-0.55, 0.18, False),
           (-0.45, 0.10, False), (-0.47, -0.02, False), (-0.33, 0.0, False), (-0.27, -0.12, False), (-0.15, -0.05, False), (-0.04, -0.15, False),
           (0.06, -0.06, False), (0.19, -0.13, False), (0.26, -0.02, False), (0.42, -0.06, False), (0.44, 0.08, False), (0.56, 0.14, False),
           (0.50, 0.28, False), (0.55, 0.40, False), (0.47, 0.50, False), (0.45, 0.60, False), (0.375, 0.64, False)]
    out.append(h.curve(sil, 1.3, ink, geometry="polyline", widths=[0.6, 1.2, 0.6]))
    # bandana: faixa entre y 0.17 e 0.33 seguindo a curvatura da cabeça (gira com o rosto)
    top = [(x / 100, 0.17 + 0.03 * (x / 42) ** 2) for x in range(-42, 43, 6)]
    bot = [(x / 100, 0.33 + 0.035 * (x / 40) ** 2) for x in range(-40, 41, 5)]
    out.append(h.curve(top, 1.0, ink, widths=[0.7, 1.0, 0.7]))
    out.append(h.curve(bot, 1.0, ink, widths=[0.7, 1.0, 0.7]))
    for sg in (-1, 1):  # pontas da bandana encostam na silhueta
        out.append(h.curve([top[0 if sg < 0 else -1], (sg * 0.552, 0.22, False)], 0.9, ink, widths=[1.0, 1.0, 1.0]))
        out.append(h.curve([bot[0 if sg < 0 else -1], (sg * 0.56, 0.40, False)], 0.9, ink, widths=[1.0, 1.0, 1.0]))
    # dobras da bandana (traços soltos)
    out.append(h.curve([(-0.12, 0.20), (-0.05, 0.27), (0.02, 0.31)], 0.5, ink, widths=[0.2, 1.0, 0.2]))
    out.append(h.curve([(0.18, 0.21), (0.22, 0.28)], 0.5, ink, widths=[0.2, 1.0, 0.2]))
    # mechas sobre a testa saindo de baixo da bandana (vales na borda de baixo da bandana)
    valleys = [(-0.30, 0.355), (-0.20, 0.34), (-0.08, 0.335), (0.04, 0.335), (0.14, 0.34), (0.24, 0.35)]
    tips = [(-0.27, 0.45), (-0.13, 0.47), (-0.01, 0.43), (0.09, 0.46), (0.21, 0.44)]
    for i, t in enumerate(tips):
        for v in (valleys[i], valleys[i + 1]):
            c = (lerp(v[0], t[0], 0.5) + (0.012 if v is valleys[i] else -0.012), lerp(v[1], t[1], 0.5))
            out.append(h.curve([v, c, t], 1.0, ink, widths=[0.35, 1.0, 0.12]))
    # laterais: da borda da bandana até a maçã do rosto (fecha cabelo × rosto)
    for sg in (-1, 1):
        out.append(h.curve([(sg * 0.40, 0.355), (sg * 0.395, 0.48), (sg * 0.385, 0.58), (sg * 0.375, 0.64, False)], 1.0, ink, widths=[0.6, 1.0, 0.4]))
        # mechas pontudas da costeleta
        out.append(h.curve([(sg * 0.42, 0.42, False), (sg * 0.41, 0.52), (sg * 0.395, 0.60)], 0.6, ink, widths=[0.3, 1.0, 0.1]))
    # mechas internas no topo
    for a, b in [((-0.30, 0.02), (-0.26, 0.15)), ((-0.06, -0.10), (-0.04, 0.12)), ((0.20, -0.08), (0.16, 0.13)), ((0.40, 0.02), (0.34, 0.15))]:
        out.append(h.curve([a, b], 0.6, ink, widths=[0.2, 1.0, 0.2], rot=False))
    seeds += [h.seed(0.0, 0.04, "cabelo", rot=False), h.seed(-0.48, 0.45, "cabelo", rot=False), h.seed(0.49, 0.45, "cabelo", rot=False),
              h.seed(0.0, 0.25, "bandana"), h.seed(-0.30, 0.26, "bandana"), h.seed(0.30, 0.26, "bandana")]
    seeds += [h.seed(lerp(valleys[i][0], valleys[i + 1][0], 0.5), (valleys[i][1] + t[1] * 2) / 3 - 0.02, "cabelo") for i, t in enumerate(tips)]
    # realce azulado do cabelo preto (faixa curva no alto) e sombra sob a bandana
    hl = [(-0.36, 0.06), (-0.18, 0.02), (0.0, 0.0), (0.18, 0.02), (0.36, 0.06), (0.34, 0.10), (0.18, 0.065), (0.0, 0.05), (-0.18, 0.065), (-0.34, 0.10)]
    h.shadow_shapes = [("cabelo_realce", [h.p(x, y, None, False) for x, y in hl])]
    h.shadow_shapes.append(("bandana_sombra", [h.p(*q) for q in [(-0.40, 0.33), (0.40, 0.33), (0.40, 0.355), (0.0, 0.37), (-0.40, 0.355)]]))
    return out, seeds


def face_ren(h, mood="neutral", open_=1.0, mouth_shape="closed"):
    """Rosto inteiro de Ren: (traços, sementes, formas)."""
    ink = INK["ren"]
    strokes, seeds, shapes = [], [], []
    strokes += face_outline(h, ink, chin=1.0, cheek=0.385)
    for sd in (-1, 1):
        e, sh = eye(h, ink, sd, "#b07a2a", open_, mood, False)
        strokes += e
        shapes += sh
    strokes += brows(h, ink, mood, False)
    strokes += nose(h, ink)
    m, ms = mouth(h, ink, mouth_shape, y=0.88)
    strokes += m
    seeds += ms
    strokes += neck(h, ink, False)
    strokes.append(h.curve([(-0.34, 1.17), (-0.11, 1.11), (0.0, 1.16), (0.11, 1.11), (0.34, 1.17)], 1.0, ink, geometry="polyline", rot=False))
    hs, hsd = hair_ren(h, ink)
    strokes += hs
    seeds += hsd
    seeds += [h.seed(0.22, 0.75, "pele"), h.seed(-0.22, 0.75, "pele"), h.seed(0.0, 1.06, "pele", rot=False)]
    shapes += getattr(h, "shadow_shapes", [])
    xs = [h.p(x, 1.0, None, False)[0] for x in (-0.11, -0.055, 0.0, 0.055, 0.11)]
    top = [[X, h.top + (jaw_y(h, X) + 0.012) * h.H] for X in xs]
    shapes.append(("pele_sombra", top + [h.p(0.113, 1.03, None, False), h.p(0.0, 1.07, None, False), h.p(-0.113, 1.03, None, False)]))
    shapes += [("brilho", q) for q in eye_highlights(h, -1, open_, mood) + eye_highlights(h, 1, open_, mood)]
    return strokes, seeds, shapes
