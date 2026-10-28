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

    def follow(self, x, y):
        """x (unidades de cabeça) de um ponto da lateral do cabelo que acompanha o contorno do rosto naquela altura: o
        deslocamento que a bochecha (x = ±0,385) sofre com o giro. Assim a costeleta não cruza o olho do lado de lá nem
        descola da bochecha (os pontos da lateral ficam fora da esfera de frente e não giram direito com `rot`)."""
        c = math.copysign(0.385, x) if x else 0.385
        d = (self.p(c, y)[0] - self.p(c, y, None, False)[0]) / self.H
        return x + d

    def jaw_x(self, sg, y):
        """x (unidades de cabeça, sem giro) do contorno do rosto girado do lado `sg` na altura `y` (bochecha → maxilar),
        para o cabelo do lado de lá não deixar o maxilar passar dele."""
        pts = [(sg * 0.385, 0.55), (sg * 0.37, 0.66), (sg * 0.29, 0.84), (sg * 0.12, 0.97)]
        px = [(self.p(x, yy)[0], yy) for x, yy in pts]
        for (xa, ya), (xb, yb) in zip(px, px[1:]):
            if ya <= y <= yb:
                return (lerp(xa, xb, (y - ya) / (yb - ya)) - self.cx) / self.H
        return (px[0][0] - self.cx) / self.H if y < px[0][1] else (px[-1][0] - self.cx) / self.H

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
    """"#rrggbb" ou "#rrggbbaa" (o alfa do texto vale no lugar de `a`)."""
    rgb = [int(h[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    if len(h) >= 9:
        a = int(h[7:9], 16) / 255
    return rgb + [a]


# ------------------------------------------------------------------------------------------------- rosto (comum)

def face_outline(h, ink, chin=1.0, cheek=0.37, temple=(0.39, 0.40)):
    """Contorno do rosto: têmpora → bochecha → maxilar → queixo, dos dois lados (o lado oculto encolhe com o yaw).
    `temple` tem de ser o MESMO ponto onde a linha do cabelo encosta (perto da borda da esfera, pontos vizinhos se
    separam ao girar)."""
    side = lambda s: [(s * temple[0], temple[1]), (s * 0.385, 0.55), (s * cheek, 0.66), (s * 0.29, 0.84), (s * 0.12, 0.97 * chin), (0.0, chin)]
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


def sparkle(h, side, open_=1.0):
    """Cintilância de quatro pontas no olho (momento de emoção): perto do brilho grande, um pouco para fora."""
    if open_ < 0.3:
        return []
    s = side
    f = h.facing(s * 0.175)
    # sobre a íris, do lado oposto ao brilho grande (que fica em cima, para dentro)
    cx, cy = s * 0.179 + 0.026 * f, 0.612
    r, k = 0.036, 0.16
    pts = []
    for i in range(8):
        a = i * math.tau / 8 - math.pi / 2
        rr = r if i % 2 == 0 else r * k
        pts.append((cx + rr * math.cos(a) * f, cy + rr * math.sin(a)))
    return [[h.p(*q) for q in pts]]


def blush(h, ink, amount=1.0):
    """Blush de anime: mancha rosada translúcida nas maçãs do rosto e três tracinhos inclinados por cima.
    Devolve (traços, formas)."""
    if amount <= 0.0:
        return [], []
    out, shapes = [], []
    for sg in (-1, 1):
        f = h.facing(sg * 0.22)
        cx, cy = sg * 0.215, 0.745
        rx, ry = 0.085 * f, 0.032
        shapes.append(("blush", [h.p(cx + rx * math.cos(t), cy + ry * math.sin(t)) for t in [k * math.tau / 14 for k in range(14)]]))
        for k in (-1, 0, 1):
            x0 = cx + k * 0.032 * f
            out.append(h.curve([(x0 + 0.012 * f * sg, cy - 0.022 * amount), (x0 - 0.012 * f * sg, cy + 0.022 * amount)], 0.45, ink, widths=[0.3, 1.0, 0.3]))
    return out, shapes


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
    dims = {"a": (0.062, 0.07), "i": (0.065, 0.024), "u": (0.032, 0.038), "e": (0.062, 0.044), "o": (0.044, 0.062),
            "shout": (0.072, 0.085), "small_o": (0.022, 0.025)}[shape]
    rx, ry = dims
    if shape in ("a", "shout", "e"):
        # boca aberta de anime: topo reto, fundo arredondado
        pts = [(cx - rx, y - ry * 0.25), (cx + rx, y - ry * 0.25), (cx + rx * 0.6, y + ry * 0.6), (cx, y + ry * 0.85), (cx - rx * 0.6, y + ry * 0.6)]
    else:
        pts = [(cx + rx * math.cos(t), y + ry * 0.2 + ry * 0.6 * math.sin(t)) for t in [k * math.tau / 8 for k in range(8)]]
    out = [h.curve(pts, 0.9, ink, closed=True, tip="marker")]
    # com língua, o meio da boca (y + 0.35 ry) é o ponto do meio da língua: a semente vai entre o topo e a língua
    seeds = [h.seed(cx, y + ry * (0.05 if shape in ("a", "shout") else 0.35), "boca")]
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
    # 3/4: as laterais do bob acompanham o contorno do rosto (`follow`); a silhueta do lado de lá segue junto, a do
    # lado de cá fica (volume do cabelo) e só a ponta de baixo encosta na lateral
    far = math.copysign(1.0, h.yaw) if h.yaw else 0.0

    def fx(x, y):
        # do lado de lá, o maxilar girado não passa da mecha (a mecha cai por cima da bochecha)
        v = h.follow(x, y)
        if x * far > 0 and y >= 0.55:
            v = far * max(far * v, far * h.jaw_x(far, y) + 0.012)
        return v

    F = lambda x, y: (fx(x, y), y, False)
    wgt = lambda y: max(0.0, min(1.0, (y - 0.22) / 0.16))
    sil = [((lerp(x, fx(x, y), wgt(y)) if x * far > 0 else (h.follow(x, y) if y >= 0.9 else x)), y, False) for x, y in sil]
    out.append(h.curve(sil, 1.3, ink, widths=[0.5, 1.2, 0.5]))
    # borda de baixo da franja: mechas com lados curvos que afinam até a ponta (vales mais altos, pontas acima das
    # sobrancelhas); cada lado é um traço, as pontas se encontram (fecha a massa do cabelo)
    valleys = [(-0.39, 0.40), (-0.29, 0.30), (-0.16, 0.31), (-0.04, 0.29), (0.09, 0.27), (0.22, 0.29), (0.32, 0.31), (0.39, 0.40)]
    tips = [(-0.335, 0.455), (-0.215, 0.445), (-0.095, 0.47), (0.03, 0.445), (0.165, 0.44), (0.28, 0.43), (0.37, 0.44)]
    for i, t in enumerate(tips):
        for v, sgn in ((valleys[i], -1), (valleys[i + 1], 1)):
            # as pontas da franja nas têmporas encostam nas laterais (que acompanham o rosto)
            edge = v in (valleys[0], valleys[-1])
            # lado da mecha: sai do vale abaulado para fora e chega reto na ponta
            dx, dy = t[0] - v[0], t[1] - v[1]
            n = (math.hypot(dx, dy) or 1.0)
            ox, oy = -dy / n * sgn * -0.018, dx / n * sgn * -0.018
            c1 = (v[0] + dx * 0.3 + ox, v[1] + dy * 0.3 + oy)
            c2 = (v[0] + dx * 0.7 + ox * 0.5, v[1] + dy * 0.7 + oy * 0.5)
            out.append(h.curve([F(*v) if edge else v, c1, c2, t], 1.05, ink, widths=[0.35, 1.0, 0.12]))
    # sombra do cabelo (cel): por dentro das laterais do bob, junto do rosto
    shadow = []
    for sg in (-1, 1):
        shadow.append([h.p(*F(sg * 0.39, 0.40)), h.p(*F(sg * 0.385, 0.62)), h.p(*F(sg * 0.38, 0.84)), h.p(*F(sg * 0.43, 0.94)), h.p(*F(sg * 0.47, 0.88)),
                       h.p(*F(sg * 0.47, 0.62)), h.p(*F(sg * 0.45, 0.42))])
    h.shadow_shapes = [("cabelo_sombra", sh) for sh in shadow]
    # lados do bob: da têmpora até a ponta, por dentro (encosta na bochecha) e a ponta curvando para dentro
    for sg in (-1, 1):
        out.append(h.curve([F(sg * 0.39, 0.40), F(sg * 0.385, 0.55), F(sg * 0.375, 0.68), F(sg * 0.38, 0.84), F(sg * 0.415, 0.92), F(sg * 0.43, 0.94)], 1.0, ink, widths=[0.6, 1.0, 0.5]))
    # mechas internas (traços soltos, não fecham região)
    for a, b in [((-0.30, 0.05), (-0.36, 0.30)), ((-0.12, -0.02), (-0.16, 0.28)), ((0.12, -0.04), (0.10, 0.22)), ((0.30, 0.03), (0.30, 0.25)),
                 ((-0.47, 0.50), (-0.45, 0.80)), ((0.48, 0.50), (0.47, 0.80))]:
        out.append(h.curve([a, ((a[0] + b[0]) / 2 + 0.01, (a[1] + b[1]) / 2), b], 0.55, ink, widths=[0.2, 1.0, 0.2], rot=abs(a[0]) < 0.45))
    # presilha verde (retângulo inclinado) à esquerda, sobre a franja
    clip = [(-0.35, 0.22), (-0.21, 0.17), (-0.195, 0.215), (-0.335, 0.265)]
    out.append(h.curve(clip, 0.9, ink, closed=True, geometry="polyline", tip="marker"))
    seeds += [h.seed(-0.27, 0.218, "presilha"), h.seed(0.02, 0.10, "cabelo", rot=False), h.seed(h.follow(-0.47, 0.72) if far < 0 else -0.47, 0.72, "cabelo", rot=False),
              h.seed(h.follow(0.48, 0.72) if far > 0 else 0.48, 0.72, "cabelo", rot=False),
              # laterais altas, entre a franja e a silhueta, e o alto da cabeça entre as mechas internas (no giro as
              # mechas internas encostam na silhueta e fecham regiões novas)
              h.seed(-0.46, 0.30, "cabelo", rot=False), h.seed(0.46, 0.30, "cabelo", rot=False),
              h.seed(-0.46, 0.22, "cabelo", rot=False), h.seed(0.46, 0.22, "cabelo", rot=False),
              # entre as mechas internas do alto (giram junto com elas)
              h.seed(-0.21, 0.10, "cabelo"), h.seed(0.21, 0.10, "cabelo"), h.seed(0.0, 0.10, "cabelo")]
    # pele entre duas mechas da franja (embaixo de cada vale): a sobrancelha pode fechar esse triângulo
    # (centroide do triângulo ponta anterior, vale, ponta seguinte: fica dentro mesmo quando as mechas são estreitas)
    seeds += [h.seed((tips[i][0] + valleys[i + 1][0] + tips[i + 1][0]) / 3, (tips[i][1] + valleys[i + 1][1] + tips[i + 1][1]) / 3, "pele") | {"reserve": True} for i in range(len(tips) - 1)]
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
    if getattr(h, "collar", True):
        strokes.append(h.curve([(-0.32, 1.16), (-0.09, 1.11), (0.0, 1.18), (0.09, 1.11), (0.32, 1.16)], 1.0, ink, geometry="polyline", rot=False))  # gola
    hs, hsd = hair_hana(h, ink)
    strokes += hs
    seeds += hsd
    # bochechas: perto do nariz (no 3/4 a bochecha do lado de lá fica estreita e a semente em 0,25 caía no cabelo)
    seeds += [h.seed(0.13, 0.75, "pele"), h.seed(-0.13, 0.75, "pele"), h.seed(0.0, 1.05, "pele", rot=False)]
    shapes += getattr(h, "shadow_shapes", [])
    xs = [h.p(x, 1.0, None, False)[0] for x in (-0.092, -0.046, 0.0, 0.046, 0.092)]
    top = [[X, h.top + (jaw_y(h, X) + 0.012) * h.H] for X in xs]
    shapes.append(("pele_sombra", top + [h.p(0.094, 1.03, None, False), h.p(0.0, 1.07, None, False), h.p(-0.094, 1.03, None, False)]))
    shapes += [("brilho", q) for q in eye_highlights(h, -1, open_, mood) + eye_highlights(h, 1, open_, mood)]
    # blush (vergonha/alegria) e cintilância (emoção): `h.blush` 0..1 e `h.sparkle`, ou pelo humor
    amt = getattr(h, "blush", 1.0 if mood in ("happy", "shy") else 0.0)
    bs, bsh = blush(h, ink, amt)
    strokes += bs
    shapes += bsh
    if getattr(h, "sparkle", False):
        shapes += [("brilho", q) for q in sparkle(h, -1, open_) + sparkle(h, 1, open_)]
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
    far = math.copysign(1.0, h.yaw) if h.yaw else 0.0
    wgt = lambda y: max(0.0, min(1.0, (y - 0.22) / 0.16))
    # do lado de lá, a parte baixa acompanha o rosto; do lado de cá a silhueta fica (volume do cabelo), menos a ponta
    # de baixo, que encosta na lateral (senão sobra uma fresta e o cabelo vaza para o fundo)
    sil = [((lerp(x, h.follow(x, y), wgt(y)) if x * far > 0 else (h.follow(x, y) if y >= 0.62 else x)), y, False) for x, y, _ in sil]
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
    # laterais: da borda da bandana até a maçã do rosto (fecha cabelo × rosto); acompanham o contorno do rosto no giro
    F = lambda x, y: (h.follow(x, y), y, False)
    for sg in (-1, 1):
        out.append(h.curve([F(sg * 0.40, 0.355), F(sg * 0.395, 0.48), F(sg * 0.385, 0.58), F(sg * 0.375, 0.64)], 1.0, ink, widths=[0.6, 1.0, 0.4]))
        # mechas pontudas da costeleta
        out.append(h.curve([F(sg * 0.42, 0.42), F(sg * 0.41, 0.52), F(sg * 0.395, 0.60)], 0.6, ink, widths=[0.3, 1.0, 0.1]))
    # mechas internas no topo
    for a, b in [((-0.30, 0.02), (-0.26, 0.15)), ((-0.06, -0.10), (-0.04, 0.12)), ((0.20, -0.08), (0.16, 0.13)), ((0.40, 0.02), (0.34, 0.15))]:
        out.append(h.curve([a, b], 0.6, ink, widths=[0.2, 1.0, 0.2], rot=False))
    # as mechas internas fecham o topo em três regiões (esquerda, meio, direita): uma semente em cada
    seeds += [h.seed(0.0, 0.04, "cabelo", rot=False), h.seed(-0.39, 0.09, "cabelo", rot=False), h.seed(0.33, 0.06, "cabelo", rot=False), h.seed(0.47, 0.16, "cabelo", rot=False),
              h.seed(lerp(-0.48, h.follow(-0.48, 0.45), 1.0 if far < 0 else 0.0), 0.45, "cabelo", rot=False), h.seed(lerp(0.49, h.follow(0.49, 0.45), 1.0 if far > 0 else 0.0), 0.45, "cabelo", rot=False),
              # logo abaixo da bandana, entre a lateral (que acompanha o rosto) e a silhueta
              h.seed((h.follow(-0.40, 0.42) - 0.53) / 2, 0.42, "cabelo", rot=False), h.seed((h.follow(0.40, 0.42) + 0.53) / 2, 0.42, "cabelo", rot=False),
              # lasca entre a mecha da costeleta e a lateral (no giro ela abre e fica maior que uma fresta)
              h.seed(h.follow(-0.4045, 0.50), 0.50, "cabelo", rot=False), h.seed(h.follow(0.4045, 0.50), 0.50, "cabelo", rot=False),
              h.seed(h.follow(-0.425, 0.39), 0.39, "cabelo", rot=False), h.seed(h.follow(0.425, 0.39), 0.39, "cabelo", rot=False),
              # bolsinho entre a borda de baixo da bandana, o topo da lateral e o topo da costeleta (centroide em px)
              *[h.seed((sum(q[0] for q in tri) / 3 - h.cx) / h.H, (sum(q[1] for q in tri) / 3 - h.top) / h.H, "cabelo", rot=False)
                for tri in ([h.p(h.follow(sg * 0.40, 0.355), 0.355, None, False), h.p(sg * 0.40, 0.365), h.p(h.follow(sg * 0.42, 0.42), 0.42, None, False)] for sg in (-1, 1))],
              h.seed(0.0, 0.25, "bandana"), h.seed(-0.30, 0.26, "bandana"), h.seed(0.30, 0.26, "bandana")]
    # cada mecha é um triângulo (vale, vale, ponta): a semente vai no centroide, que sempre fica dentro
    seeds += [h.seed((valleys[i][0] + valleys[i + 1][0] + t[0]) / 3, (valleys[i][1] + valleys[i + 1][1] + t[1]) / 3, "cabelo") for i, t in enumerate(tips)]
    # pele entre duas mechas (embaixo de cada vale): a sobrancelha pode fechar esse triângulo, então semente própria
    # (centroide do triângulo ponta anterior, vale, ponta seguinte: fica dentro mesmo quando as mechas são estreitas)
    seeds += [h.seed((tips[i][0] + valleys[i + 1][0] + tips[i + 1][0]) / 3, (tips[i][1] + valleys[i + 1][1] + tips[i + 1][1]) / 3, "pele") | {"reserve": True} for i in range(len(tips) - 1)]
    # realce azulado do cabelo preto (faixa curva no alto) e sombra sob a bandana
    hl = [(-0.36, 0.06), (-0.18, 0.02), (0.0, 0.0), (0.18, 0.02), (0.36, 0.06), (0.34, 0.10), (0.18, 0.065), (0.0, 0.05), (-0.18, 0.065), (-0.34, 0.10)]
    h.shadow_shapes = [("cabelo_realce", [h.p(x, y, None, False) for x, y in hl])]
    h.shadow_shapes.append(("bandana_sombra", [h.p(*q) for q in [(-0.40, 0.33), (0.40, 0.33), (0.40, 0.355), (0.0, 0.37), (-0.40, 0.355)]]))
    return out, seeds


def face_ren(h, mood="neutral", open_=1.0, mouth_shape="closed"):
    """Rosto inteiro de Ren: (traços, sementes, formas)."""
    ink = INK["ren"]
    strokes, seeds, shapes = [], [], []
    strokes += face_outline(h, ink, chin=1.0, cheek=0.385, temple=(0.40, 0.355))
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
    if getattr(h, "collar", True):
        strokes.append(h.curve([(-0.34, 1.17), (-0.11, 1.11), (0.0, 1.16), (0.11, 1.11), (0.34, 1.17)], 1.0, ink, geometry="polyline", rot=False))
    hs, hsd = hair_ren(h, ink)
    strokes += hs
    seeds += hsd
    seeds += [h.seed(0.22, 0.75, "pele"), h.seed(-0.22, 0.75, "pele"), h.seed(0.0, 1.06, "pele", rot=False)]
    # testa entre a primeira/última mecha e a lateral do cabelo: a sobrancelha fecha esse triângulo (é pele)
    seeds += [h.seed(-0.345, 0.40, "pele"), h.seed(0.31, 0.40, "pele")]
    shapes += getattr(h, "shadow_shapes", [])
    xs = [h.p(x, 1.0, None, False)[0] for x in (-0.11, -0.055, 0.0, 0.055, 0.11)]
    top = [[X, h.top + (jaw_y(h, X) + 0.012) * h.H] for X in xs]
    shapes.append(("pele_sombra", top + [h.p(0.113, 1.03, None, False), h.p(0.0, 1.07, None, False), h.p(-0.113, 1.03, None, False)]))
    shapes += [("brilho", q) for q in eye_highlights(h, -1, open_, mood) + eye_highlights(h, 1, open_, mood)]
    # blush (vergonha/alegria) e cintilância (emoção): `h.blush` 0..1 e `h.sparkle`, ou pelo humor
    amt = getattr(h, "blush", 1.0 if mood in ("happy", "shy") else 0.0)
    bs, bsh = blush(h, ink, amt)
    strokes += bs
    shapes += bsh
    if getattr(h, "sparkle", False):
        shapes += [("brilho", q) for q in sparkle(h, -1, open_) + sparkle(h, 1, open_)]
    return strokes, seeds, shapes


# ------------------------------------------------------------------------------------------------- corpo (busto)

def body_bust(h, who, bottom=2.45, turn=0.0, breath=0.0):
    """Busto até `bottom` (cabeças), com a roupa do personagem; `turn` -1..1 encolhe o lado distante (3/4).
    O busto não gira com a cabeça (fica no eixo); fecha embaixo com uma linha (borda do quadro/recorte)."""
    ink = INK[who]
    male = who == "ren"
    sh = 0.63 if male else 0.55        # largura dos ombros
    wa = 0.50 if male else 0.40        # cintura
    nw = 0.105 if male else 0.085
    # 3/4 (rosto olhando para o lado de `turn`): o lado de lá (sg com o mesmo sinal de `turn`) estreita e o de cá
    # alarga um pouco; o meio do peito (gola, avental) desloca para onde o rosto olha. (Antes o sinal estava trocado e
    # quem estreitava era o lado de cá.)
    k = lambda sg: 1.0 - 0.40 * max(0.0, sg * turn) + 0.08 * max(0.0, -sg * turn)
    ox = 0.15 * turn
    X = lambda sg, x: ox + sg * x * k(sg)
    Xn = lambda sg: sg * nw * 1.03     # base do pescoço: fica no eixo da cabeça (o pescoço não desloca)
    out, seeds, shapes = [], [], []
    # respiração: na inspiração (`breath` > 0) ombros e peito sobem um pouco; o pescoço (y ≤ 1,12) fica
    lift = 0.014 * breath
    B = lambda y: y if y <= 1.12 or y >= bottom else y - lift * min(1.0, (y - 1.12) / 0.3)
    c = lambda pts, w=1.0, **kw: h.curve([(x, B(y), False) for x, y in pts], w, ink, **kw)
    for sg in (-1, 1):
        # trapézio → ombro → braço (contorno externo) até a borda de baixo
        out.append(c([(Xn(sg), 1.12), (X(sg, 0.30), 1.20), (X(sg, sh), 1.33), (X(sg, sh + 0.06), 1.50), (X(sg, sh + 0.08), 1.90), (X(sg, sh + 0.07), bottom)], 1.1, widths=[0.7, 1.0, 1.0]))
        # axila → lateral do tronco (contorno interno do braço / lado do corpo)
        out.append(c([(X(sg, sh - 0.06), 1.52), (X(sg, sh - 0.10), 1.75), (X(sg, wa + 0.02), 2.10), (X(sg, wa), bottom)], 1.0, widths=[0.5, 1.0, 1.0]))
    out.append(c([(X(-1, sh + 0.12), bottom), (X(1, sh + 0.12), bottom)], 0.6, geometry="polyline"))  # recorte
    if who == "hana":
        # blusa: gola redonda (duas abas) e manga com punho dobrado na borda
        for sg in (-1, 1):
            out.append(c([(X(sg, nw * 0.2), 1.14), (X(sg, 0.12), 1.22), (X(sg, 0.24), 1.24), (X(sg, 0.27), 1.17), (Xn(sg), 1.12)], 0.9, closed=True))  # fechada: separa a aba do pescoço
        out.append(c([(X(-1, nw * 0.25), 1.142), (X(1, nw * 0.25), 1.142)], 0.6, geometry="polyline"))  # decote: fecha pescoço × blusa
        # avental: babador com alças sobre os ombros, bolso
        bib = [(X(-1, 0.27), 1.62), (X(1, 0.27), 1.62)]
        out.append(c([(X(-1, 0.27), 1.62), (X(-1, 0.30), bottom)], 1.0))
        out.append(c([(X(1, 0.27), 1.62), (X(1, 0.30), bottom)], 1.0))
        out.append(c(bib, 1.0, geometry="polyline"))
        for sg in (-1, 1):  # alças
            # as alças passam por cima da linha do ombro (fecham a região da alça para o balde)
            out.append(c([(X(sg, 0.22), 1.62), (X(sg, 0.30), 1.40), (X(sg, 0.40), 1.215)], 0.9))
            out.append(c([(X(sg, 0.27), 1.62), (X(sg, 0.35), 1.42), (X(sg, 0.47), 1.235)], 0.9))
        out.append(c([(X(-1, 0.13), 1.95), (X(1, 0.13), 1.95), (X(1, 0.13), 2.18), (X(-1, 0.13), 2.18)], 0.8, closed=True, geometry="polyline"))  # bolso
        out.append(c([(X(0, 0.0), 1.95), (X(0, 0.0), 2.18)], 0.5, geometry="polyline"))
        seeds += [h.seed(ox, 1.80, "avental", False), h.seed(ox, 2.30, "avental", False), h.seed(ox + 0.06, 2.06, "avental", False), h.seed(ox - 0.06, 2.06, "avental", False),
                  h.seed(X(-1, 0.20), 1.45, "blusa", False), h.seed(X(1, 0.20), 1.45, "blusa", False), h.seed(X(-1, 0.42), 1.9, "blusa", False), h.seed(X(1, 0.42), 1.9, "blusa", False),
                  h.seed(X(-1, sh + 0.02), 2.0, "blusa", False), h.seed(X(1, sh + 0.02), 2.0, "blusa", False), h.seed(X(-1, 0.18), 1.19, "blusa", False), h.seed(X(1, 0.18), 1.19, "blusa", False),
                  h.seed(X(-1, 0.299), 1.48, "avental", False), h.seed(X(1, 0.299), 1.48, "avental", False),  # meio da alça
                  # fresta entre a borda de cima da aba da gola e a linha do ombro (a aba cruza o ombro)
                  h.seed(X(-1, 0.23), 1.156, "blusa", False), h.seed(X(1, 0.23), 1.156, "blusa", False),
                  h.seed(X(-1, 0.46), 1.42, "blusa", False), h.seed(X(1, 0.46), 1.42, "blusa", False)]  # ombros fora das alças
        for sg in (-1, 1):
            # alça do avental como forma entre as duas linhas: no 3/4 a do lado distante fica mais estreita que o
            # fechamento do balde e se juntava com a blusa
            strap = [(X(sg, 0.22), 1.62), (X(sg, 0.30), 1.40), (X(sg, 0.40), 1.215), (X(sg, 0.47), 1.235), (X(sg, 0.35), 1.42), (X(sg, 0.27), 1.62)]
            shapes.append(("avental", [h.p(x, B(y), None, False) for x, y in strap]))
            shapes.append(("blusa_sombra", [h.p(x, B(y), None, False) for x, y in [(X(sg, sh - 0.07), 1.52), (X(sg, sh - 0.10), 1.75), (X(sg, wa + 0.02), 2.1), (X(sg, wa), bottom), (X(sg, wa - 0.07), bottom), (X(sg, wa - 0.05), 2.05), (X(sg, sh - 0.16), 1.6)]]))
    else:
        # camisa azul com gola em V, avental creme com alça no pescoço, manga dobrada
        out.append(c([(Xn(-1), 1.12), (X(0, 0.0), 1.30), (Xn(1), 1.12)], 0.9, geometry="polyline"))
        for sg in (-1, 1):  # gola da camisa (abas)
            out.append(c([(Xn(sg), 1.12), (X(sg, 0.22), 1.22), (X(sg, 0.10), 1.30), (X(0, 0.0), 1.30)], 0.9, geometry="polyline"))
        out.append(c([(X(-1, 0.30), 1.58), (X(1, 0.30), 1.58)], 1.0, geometry="polyline"))  # topo do avental
        for sg in (-1, 1):
            out.append(c([(X(sg, 0.30), 1.58), (X(sg, 0.33), bottom)], 1.0))
            out.append(c([(X(sg, 0.23), 1.58), (X(sg, 0.13), 1.31)], 0.9))  # alça do pescoço
            out.append(c([(X(sg, sh + 0.075), 1.95), (X(sg, sh - 0.11), 1.95)], 0.8, geometry="polyline"))  # dobra da manga
        out.append(c([(X(-1, 0.20), 2.05), (X(1, 0.20), 2.05)], 0.7, geometry="polyline"))  # cordão do avental
        seeds += [h.seed(ox, 1.80, "avental", False), h.seed(ox, 2.30, "avental", False), h.seed(X(-1, 0.20), 1.45, "camisa", False), h.seed(X(1, 0.20), 1.45, "camisa", False),
                  h.seed(X(-1, 0.42), 1.9, "camisa", False), h.seed(X(1, 0.42), 1.9, "camisa", False), h.seed(X(-1, sh), 1.7, "camisa", False), h.seed(X(1, sh), 1.7, "camisa", False),
                  h.seed(X(-1, sh), 2.2, "camisa", False), h.seed(X(1, sh), 2.2, "camisa", False), h.seed(ox, 1.25, "pele", False), h.seed(X(-1, 0.15), 1.20, "camisa", False), h.seed(X(1, 0.15), 1.20, "camisa", False),
                  h.seed(ox, 1.45, "camisa", False)]
        for sg in (-1, 1):
            shapes.append(("camisa_sombra", [h.p(x, B(y), None, False) for x, y in [(X(sg, sh - 0.07), 1.52), (X(sg, sh - 0.10), 1.75), (X(sg, wa + 0.02), 2.1), (X(sg, wa), bottom), (X(sg, wa - 0.08), bottom), (X(sg, wa - 0.06), 2.05), (X(sg, sh - 0.17), 1.6)]]))
    return out, seeds, shapes


def bust(h, who, mood="neutral", open_=1.0, mouth_shape="closed", bottom=2.45, turn=None, breath=0.0):
    """Rosto + busto. `turn` padrão segue o giro da cabeça (metade)."""
    h.collar = False
    f, sd, sh = (face_hana if who == "hana" else face_ren)(h, mood, open_, mouth_shape)
    t = (math.degrees(h.yaw) / 60.0) if turn is None else turn
    b, bs, bsh = body_bust(h, who, bottom, t, breath)
    return f + b, sd + bs, sh + bsh


# ------------------------------------------------------------------------------------------------- figura inteira (model sheet)

LANDMARKS = {"hana": {"waist": 2.52, "hip": 2.92, "knee": 4.2, "feet": 5.5}, "ren": {"waist": 2.78, "hip": 3.18, "knee": 4.59, "feet": 6.0}}


def hand(h, ink, cx, top, scale=1.0, mirror=1):
    """Mão relaxada simples (vista de frente/lado), em unidades de cabeça a partir do punho (cx, top)."""
    sx = mirror * 0.11 * scale
    pts = [(cx - sx * 0.55, top), (cx - sx * 0.75, top + 0.10 * scale), (cx - sx * 0.6, top + 0.24 * scale), (cx - sx * 0.1, top + 0.30 * scale),
           (cx + sx * 0.45, top + 0.24 * scale), (cx + sx * 0.7, top + 0.12 * scale), (cx + sx * 0.55, top)]
    out = [h.curve([(x, y, False) for x, y in pts], 0.9, ink, widths=[0.7, 1.0, 0.7])]
    out.append(h.curve([(cx - sx * 0.3, top + 0.16 * scale, False), (cx - sx * 0.25, top + 0.26 * scale, False)], 0.5, ink, widths=[0.3, 1.0, 0.3]))  # dedos
    out.append(h.curve([(cx + sx * 0.1, top + 0.17 * scale, False), (cx + sx * 0.12, top + 0.27 * scale, False)], 0.5, ink, widths=[0.3, 1.0, 0.3]))
    return out, [h.seed(cx, top + 0.15 * scale, "pele", False)]


def figure(h, who, view="front"):
    """Figura inteira para o model sheet: view = front | threequarter | back. Devolve (traços, sementes, formas)."""
    L = LANDMARKS[who]
    ink = INK[who]
    male = who == "ren"
    turn = 0.5 if view == "threequarter" else 0.0
    if view == "back":
        strokes, seeds, shapes = back_head(h, who)
    else:
        h.collar = False
        strokes, seeds, shapes = (face_hana if who == "hana" else face_ren)(h, "neutral", 1.0, "closed")
    # busto até a cintura sem a linha de recorte
    b, bs, bsh = body_bust(h, who, L["waist"], turn)
    b = b[:4] + b[5:]  # tira a linha de recorte (5º traço)
    if view == "back":  # costas: sem frente da roupa (fica só o contorno + alças/cordão)
        b = b[:4]
        bs = [x for x in bs if x["name"] in ("blusa", "camisa")] + [h.seed(0.0, 1.8, "blusa" if not male else "camisa", False)]
        bsh = []
    strokes += b
    seeds += bs
    shapes += bsh
    sh = 0.63 if male else 0.55
    k = lambda sg: 1.0 - 0.35 * max(0.0, -sg * turn)
    ox = 0.10 * turn
    X = lambda sg, x: ox + sg * x * k(sg)
    c = lambda pts, w=1.0, **kw: h.curve([(x, y, False) for x, y in pts], w, ink, **kw)
    wa = 0.50 if male else 0.40
    wrist = L["hip"] + 0.02
    for sg in (-1, 1):
        # braço continua até o punho (o busto parou na cintura)
        c_out = [(X(sg, sh + 0.07), L["waist"]), (X(sg, sh + 0.05), wrist)]
        c_in = [(X(sg, wa), L["waist"]), (X(sg, wa - 0.01), L["waist"] + 0.05)]
        strokes.append(c(c_out, 1.0))
        # contorno interno do braço, da axila ao punho (separa braço e tronco)
        strokes.append(c([(X(sg, sh - 0.06), 1.52), (X(sg, sh - 0.10), 1.95), (X(sg, sh - 0.12), wrist)], 1.0))
        strokes.append(c([(X(sg, sh + 0.05), wrist), (X(sg, sh - 0.12), wrist)], 0.9, geometry="polyline"))  # punho
        hs, hsd = hand(h, ink, X(sg, sh - 0.035), wrist, 1.0, sg)
        strokes += hs
        seeds += hsd
        seeds.append(h.seed(X(sg, sh - 0.03), L["waist"] + 0.2, "blusa" if not male else "camisa", False))
        seeds.append(h.seed(X(sg, sh - 0.01), 1.6, "blusa" if not male else "camisa", False))
    if not male:
        # saia do avental (godê leve) da cintura até o joelho
        hem = L["knee"] - 0.25
        for sg in (-1, 1):
            strokes.append(c([(X(sg, wa), L["waist"]), (X(sg, wa + 0.06), L["hip"]), (X(sg, 0.56), hem)], 1.0))
        strokes.append(c([(X(-1, 0.56), hem), (X(-0.5, 0.28), hem + 0.04), (X(0, 0.0), hem), (X(0.5, 0.28), hem + 0.04), (X(1, 0.56), hem)], 1.0))
        strokes.append(c([(X(-1, wa), L["waist"]), (X(1, wa), L["waist"])], 0.9, geometry="polyline"))  # cintura
        if view != "back":
            strokes.append(c([(X(-1, 0.14), 3.05), (X(1, 0.14), 3.05), (X(1, 0.14), 3.35), (X(-1, 0.14), 3.35)], 0.8, closed=True, geometry="polyline"))
        seeds += [h.seed(ox, 3.6, "avental", False), h.seed(ox, 3.2, "avental", False), h.seed(ox - 0.3, 3.4, "avental", False), h.seed(ox + 0.3, 3.4, "avental", False)]
        leg_top, sock, foot = hem, L["feet"] - 0.55, L["feet"]
        lw = (0.095, 0.075)
        cols = ("pele", "bota")
    else:
        # avental comprido até o joelho por cima da calça; camisa por dentro da calça (linha da cintura)
        hem = L["knee"] - 0.05
        strokes.append(c([(X(-1, wa), L["waist"]), (X(1, wa), L["waist"])], 0.9, geometry="polyline"))
        seeds += [h.seed(X(-1, 0.42), L["waist"] - 0.2, "camisa", False), h.seed(X(1, 0.42), L["waist"] - 0.2, "camisa", False)]
        for sg in (-1, 1):
            if view != "back":
                strokes.append(c([(X(sg, 0.33), L["waist"]), (X(sg, 0.36), hem)], 1.0))
            strokes.append(c([(X(sg, wa), L["waist"]), (X(sg, wa), L["hip"]), (X(sg, 0.30), L["feet"] - 0.35)], 1.0))  # calça (fora)
        if view != "back":
            strokes.append(c([(X(-1, 0.36), hem), (X(1, 0.36), hem)], 1.0, geometry="polyline"))
            strokes.append(c([(X(0, 0.0), hem), (X(0, 0.0), L["feet"] - 0.35)], 1.0))  # entreperna
        else:
            strokes.append(c([(X(0, 0.0), L["hip"] + 0.25), (X(0, 0.0), L["feet"] - 0.35)], 1.0))
        strokes.append(c([(X(-1, 0.30), L["feet"] - 0.35), (X(1, 0.30), L["feet"] - 0.35)], 0.9, geometry="polyline"))  # barra
        seeds += [h.seed(ox, 3.6, "avental" if view != "back" else "calca", False), h.seed(X(-1, 0.43), 3.4, "calca", False), h.seed(X(1, 0.43), 3.4, "calca", False),
                  h.seed(X(-1, 0.16), hem + 0.3, "calca", False), h.seed(X(1, 0.16), hem + 0.3, "calca", False)]
        leg_top, sock, foot = L["feet"] - 0.35, L["feet"] - 0.35, L["feet"]
        lw = (0.13, 0.12)
        cols = (None, "sapato")
    for sg in (-1, 1):
        lx = X(sg, 0.17)
        if leg_top < sock:  # perna à mostra (Hana)
            strokes.append(c([(lx - sg * lw[0], leg_top), (lx - sg * lw[0] * 0.8, L["knee"]), (lx - sg * lw[1], sock)], 1.0))
            strokes.append(c([(lx + sg * lw[0], leg_top), (lx + sg * lw[0] * 0.9, L["knee"]), (lx + sg * lw[1], sock)], 1.0))
            seeds.append(h.seed(lx, (leg_top + sock) / 2, "pele", False))
        # bota/sapato
        boot = [(lx - sg * lw[1] * 1.15, sock), (lx - sg * lw[1] * 1.2, foot - 0.08), (lx - sg * lw[1] * 1.2, foot), (lx + sg * lw[1] * 2.0, foot),
                (lx + sg * lw[1] * 1.9, foot - 0.10), (lx + sg * lw[1] * 1.15, foot - 0.18), (lx + sg * lw[1] * 1.15, sock)]
        strokes.append(c(boot, 1.0, closed=True))
        seeds.append(h.seed(lx, foot - 0.12, cols[1], False))
    return strokes, seeds, shapes


def back_head(h, who):
    """Cabeça de costas: só a massa do cabelo (silhueta) e o pescoço."""
    ink = INK[who]
    out, seeds = [], []
    if who == "hana":
        sil = [(-0.43, 0.94), (-0.51, 0.70), (-0.54, 0.42), (-0.48, 0.15), (-0.30, -0.03), (0.02, -0.08), (0.31, -0.03), (0.49, 0.15),
               (0.55, 0.42), (0.52, 0.70), (0.43, 0.94)]
        out.append(h.curve([(x, y, False) for x, y in sil], 1.3, ink, widths=[0.5, 1.2, 0.5]))
        out.append(h.curve([(-0.43, 0.94, False), (-0.2, 0.98, False), (0.0, 0.95, False), (0.2, 0.98, False), (0.43, 0.94, False)], 1.1, ink))
        for x in (-0.25, 0.0, 0.25):
            out.append(h.curve([(x * 0.9, 0.05, False), (x, 0.5, False), (x * 1.05, 0.9, False)], 0.55, ink, widths=[0.2, 1.0, 0.2]))
        seeds.append(h.seed(0.0, 0.5, "cabelo", False))
        neck_top = 0.95
    else:
        sil = [(-0.375, 0.64), (-0.45, 0.60), (-0.47, 0.50), (-0.53, 0.40), (-0.49, 0.30), (-0.55, 0.18), (-0.45, 0.10), (-0.47, -0.02), (-0.33, 0.0),
               (-0.27, -0.12), (-0.15, -0.05), (-0.04, -0.15), (0.06, -0.06), (0.19, -0.13), (0.26, -0.02), (0.42, -0.06), (0.44, 0.08), (0.56, 0.14),
               (0.50, 0.28), (0.55, 0.40), (0.47, 0.50), (0.45, 0.60), (0.375, 0.64)]
        out.append(h.curve([(x, y, False) for x, y in sil], 1.3, ink, geometry="polyline", widths=[0.6, 1.2, 0.6]))
        out.append(h.curve([(-0.375, 0.64, False), (-0.2, 0.80, False), (0.0, 0.84, False), (0.2, 0.80, False), (0.375, 0.64, False)], 1.1, ink))
        # nó da bandana atrás
        out.append(h.curve([(-0.5, 0.22, False), (0.5, 0.22, False)], 1.0, ink))
        out.append(h.curve([(-0.5, 0.38, False), (0.5, 0.38, False)], 1.0, ink))
        out.append(h.curve([(-0.04, 0.30, False), (-0.10, 0.62, False), (0.02, 0.58, False), (0.04, 0.30, False), (0.14, 0.55, False), (0.22, 0.50, False), (0.04, 0.30, False)], 0.9, ink, closed=True))
        seeds += [h.seed(0.0, 0.08, "cabelo", False), h.seed(-0.3, 0.30, "bandana", False), h.seed(0.3, 0.30, "bandana", False), h.seed(-0.3, 0.55, "cabelo", False), h.seed(0.33, 0.55, "cabelo", False),
                  h.seed(-0.03, 0.50, "bandana", False), h.seed(0.12, 0.47, "bandana", False)]
        neck_top = 0.80
    nw = 0.105 if who == "ren" else 0.085
    for sg in (-1, 1):
        out.append(h.curve([(sg * nw, neck_top, False), (sg * nw * 1.03, 1.12, False)], 0.9, ink))
    out.append(h.curve([(-nw * 1.03, 1.12, False), (0.0, 1.135, False), (nw * 1.03, 1.12, False)], 0.9, ink))  # gola de trás
    seeds.append(h.seed(0.0, 1.05, "pele", False))
    return out, seeds, []
