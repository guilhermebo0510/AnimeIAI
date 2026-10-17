"""Cenário 2 (C006): varanda da casa de madeira ao entardecer. A geometria vem de uma cena 3D de blocos (aiai_guide3d →
camada de guia); as massas pintadas (FILL_SHAPE / PATTERN_FILL em perspectiva) usam a MESMA câmera, projetada aqui
como no motor (glam look_at_rh + perspective_rh, lente em mm sobre 36 mm). Reexecutável."""
import math
from curta import d, ids, node, OUT, rgba

cut = ids["cuts"][5]
W, H = 1920, 1080
CAM = {"position": [0.0, 1.35, 4.0], "target": [0.0, 1.25, 0.0], "focal_mm": 48.0}

# ---------- projeção (igual a aiai-guide3d/camera.rs) ----------
def sub(a, b): return [a[i] - b[i] for i in range(3)]
def dot(a, b): return sum(a[i] * b[i] for i in range(3))
def cross(a, b): return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
def norm(a):
    l = math.sqrt(dot(a, a)); return [v / l for v in a]

eye, tgt = CAM["position"], CAM["target"]
fwd = norm(sub(tgt, eye)); right = norm(cross(fwd, [0, 1, 0])); up = cross(right, fwd)
FPX = (W / 2) * CAM["focal_mm"] / 18.0

def P(p):
    v = sub(p, eye)
    x, y, z = dot(v, right), dot(v, up), dot(v, fwd)
    return [W / 2 + FPX * x / z, H / 2 - FPX * y / z]

def quad(*pts):
    return [P(p) for p in pts]

# ---------- camadas ----------
_call = d.call
d.call = lambda a, p=None, **kw: _call(a, {"frame": 0, **(p or {})} if a != "RENDER" else p, **kw)

def layer(key, kind="raster"):
    lid = node(f"c6_{key}", key, {"type": "layer", "kind": kind}, cut)
    cel = node(f"c6_{key}_c1", "1", {"type": "cel", "number": 1}, lid)
    d.apply({"kind": "replace_data", "id": cel, "data": {"type": "cel", "number": 1}})
    return lid

LAYERS = [("bg_parede", "raster"), ("bg_varanda", "raster"), ("guia3d", "guide")]
L = {k: layer(k, kind) for k, kind in LAYERS}
for i, (k, _) in enumerate(LAYERS):
    d.apply({"kind": "move_node", "id": L[k], "parent": cut, "index": i + 1})

def C(h, a=1.0): return {"type": "direct", "rgba": rgba(h, a)}

def fill(lay, pts_list, color, alpha=1.0, **kw):
    pts_list = [p for p in pts_list if max(q[0] for q in p) > 0 and min(q[0] for q in p) < W and max(q[1] for q in p) > 0 and min(q[1] for q in p) < H]
    if not pts_list:
        return
    d.call("FILL_SHAPE", {"cut": cut, "layer": L[lay], "shapes": [{"points": p, "geometry": "polyline"} for p in pts_list], "color": C(color, alpha), **kw})

def wood(lay, q, dark, light, size, seed=2, rings=0.25, scale=6):
    d.call("PATTERN_FILL", {"cut": cut, "layer": L[lay], "texture": {"spec": {"type": "wood", "scale": scale, "rings": rings}, "dark": dark, "light": light, "seed": seed, "contrast": 0.6},
                            "mapping": {"type": "perspective", "quad": q, "size": size}})

# ---------- cena 3D (guia) ----------
ZW, ZP = -1.2, 0.6          # parede do fundo, linha dos pilares
EAVE, RAIL = 1.85, 0.75     # viga do beiral (base), corrimão (topo)
scene = {"objects": [
    {"shape": {"type": "box", "size": [8, 2.8, 0.1]}, "position": [0, 1.4, ZW - 0.05]},
    {"shape": {"type": "box", "size": [8, 0.1, 1.9]}, "position": [0, -0.05, (ZW + ZP) / 2]},
    {"shape": {"type": "box", "size": [0.10, EAVE, 0.10]}, "position": [-1.2, EAVE / 2, ZP]},
    {"shape": {"type": "box", "size": [0.10, EAVE, 0.10]}, "position": [1.2, EAVE / 2, ZP]},
    {"shape": {"type": "box", "size": [8, 0.22, 0.2]}, "position": [0, EAVE + 0.11, ZP]},
    {"shape": {"type": "box", "size": [2.24, 0.08, 0.12]}, "position": [0, RAIL - 0.04, ZP]},
    {"shape": {"type": "box", "size": [2.0, 1.8, 0.04]}, "position": [0, 0.9, ZW + 0.02]},
], "mannequins": [
    {"position": [-0.42, 0, 0], "pose": "standing"},
    {"position": [0.42, 0, 0], "pose": "standing"},
]}
g = d.call("GUIDE3D", {"cut": cut, "layer": L["guia3d"], "scene": scene, "camera": CAM})

# ---------- parede do fundo ----------
wall = quad([-4, 2.8, ZW], [4, 2.8, ZW], [4, 0, ZW], [-4, 0, ZW])
wood("bg_parede", wall, [0.42, 0.27, 0.19], [0.62, 0.42, 0.29], [1600, 560], seed=3)
# tábuas verticais (juntas)
fill("bg_parede", [quad([x, 2.8, ZW], [x + 0.012, 2.8, ZW], [x + 0.012, 0, ZW], [x, 0, ZW]) for x in [i * 0.24 - 4 for i in range(34)] if abs(x) > 1.05], "#3a251b", 0.7)
# shoji (duas folhas de correr, papel aceso por dentro)
SX0, SX1, SY1 = -1.0, 1.0, 1.8
zs = ZW + 0.01
fill("bg_parede", [quad([SX0 - 0.06, SY1 + 0.08, zs], [SX1 + 0.06, SY1 + 0.08, zs], [SX1 + 0.06, 0, zs], [SX0 - 0.06, 0, zs])], "#4a2f22")
fill("bg_parede", [quad([SX0, SY1, zs], [SX1, SY1, zs], [SX1, 0.02, zs], [SX0, 0.02, zs])], "#f7e3b2")
d.call("PATTERN_FILL", {"cut": cut, "layer": L["bg_parede"], "texture": {"spec": {"type": "paper", "scale": 4}, "dark": [0.93, 0.80, 0.55], "light": [1.0, 0.93, 0.74], "seed": 6, "contrast": 0.5, "opacity": 0.6},
                        "mapping": {"type": "perspective", "quad": quad([SX0, SY1, zs], [SX1, SY1, zs], [SX1, 0.02, zs], [SX0, 0.02, zs]), "size": [600, 540]}})
# kumiko: grade de ripas (colunas e linhas), quadro central e travessa de baixo
zk = zs + 0.005
bars = []
for i in range(7):
    x = SX0 + i * (SX1 - SX0) / 6
    w = 0.035 if i in (0, 3, 6) else 0.012
    bars.append(quad([x - w / 2, SY1, zk], [x + w / 2, SY1, zk], [x + w / 2, 0.02, zk], [x - w / 2, 0.02, zk]))
for j in range(10):
    y = 0.32 + j * (SY1 - 0.32) / 9
    bars.append(quad([SX0, y + 0.006, zk], [SX1, y + 0.006, zk], [SX1, y - 0.006, zk], [SX0, y - 0.006, zk]))
bars.append(quad([SX0, 0.32, zk], [SX1, 0.32, zk], [SX1, 0.02, zk], [SX0, 0.02, zk]))
fill("bg_parede", bars, "#6b4a32")
# luz quente do entardecer entrando pela esquerda (faixa) e sombra do beiral no alto da parede
d.call("GRADIENT", {"cut": cut, "layer": L["bg_parede"], "kind": "linear", "from": [0, 0], "to": [1920, 0],
                    "stops": [[0, "#ffb26a55"], [0.45, "#ff9a5a18"], [1, "#00000000"]], "blend": "screen"})
fill("bg_parede", [quad([-4, 2.8, ZW + 0.02], [4, 2.8, ZW + 0.02], [4, 2.05, ZW + 0.02], [-4, 2.25, ZW + 0.02])], "#2a1a2a", 0.45)

# manchas de sol baixo entrando pela esquerda (paralelogramos) e sombra do pilar esquerdo na parede
zl = ZW + 0.03
fill("bg_parede", [quad([-3.2, 1.75, zl], [-1.6, 1.75, zl], [-0.4, 0.0, zl], [-2.0, 0.0, zl]), quad([-0.9, 1.75, zl], [0.5, 1.75, zl], [1.7, 0.0, zl], [0.3, 0.0, zl])], "#ffb070", 0.22)
fill("bg_parede", [quad([-1.05, 1.75, zl], [-0.92, 1.75, zl], [0.28, 0.0, zl], [0.15, 0.0, zl])], "#2a1a2a", 0.28)

# ---------- varanda: beiral, pilares, corrimão ----------
def box_faces(cx, cy, cz, sx, sy, sz):
    x0, x1, y0, y1, z0, z1 = cx - sx / 2, cx + sx / 2, cy - sy / 2, cy + sy / 2, cz - sz / 2, cz + sz / 2
    f = {"front": quad([x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]),
         "bottom": quad([x0, y0, z1], [x1, y0, z1], [x1, y0, z0], [x0, y0, z0]),
         "top": quad([x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]),
         "left": quad([x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]),
         "right": quad([x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1])}
    return f

beam = box_faces(0, EAVE + 0.11, ZP, 8, 0.22, 0.2)
fill("bg_varanda", [beam["bottom"]], "#3a2620")
wood("bg_varanda", beam["front"], [0.38, 0.24, 0.17], [0.58, 0.38, 0.26], [1800, 60], seed=8)
# telhado acima do beiral (massa escura até o topo do quadro)
fill("bg_varanda", [[[-20, -20], [1940, -20], P([4, EAVE + 0.22, ZP])[:1] + [P([4, EAVE + 0.22, ZP])[1]], P([-4, EAVE + 0.22, ZP])]], "#2e2230")
for sx in (-1.2, 1.2):
    post = box_faces(sx, EAVE / 2, ZP, 0.10, EAVE, 0.10)
    side = post["right"] if sx < 0 else post["left"]
    fill("bg_varanda", [side], "#4a3024")
    wood("bg_varanda", post["front"], [0.45, 0.29, 0.20] if sx > 0 else [0.55, 0.36, 0.24], [0.66, 0.45, 0.30] if sx > 0 else [0.80, 0.56, 0.38], [60, 900], seed=9, rings=0.1)
rail = box_faces(0, RAIL - 0.04, ZP, 2.24, 0.08, 0.12)
fill("bg_varanda", [rail["top"]], "#c58c5c")
wood("bg_varanda", rail["front"], [0.45, 0.29, 0.20], [0.66, 0.45, 0.30], [900, 40], seed=10)
# borda de luz no lado esquerdo do pilar direito e no corrimão
pr = box_faces(1.2, EAVE / 2, ZP, 0.10, EAVE, 0.10)["front"]
fill("bg_varanda", [[pr[0], [pr[0][0] + 6, pr[0][1]], [pr[3][0] + 6, pr[3][1]], pr[3]]], "#ffcf9a", 0.8)
# sino de vento (furin) pendurado no beiral, à esquerda
fx, fy = P([-0.75, EAVE, ZP - 0.05])
d.call("PAINT", {"cut": cut, "layer": L["bg_varanda"], "preset": "Redondo duro", "preset_overrides": {"size": 2}, "color": C("#2a1a14"),
                 "strokes": [{"points": [[fx, fy, 1], [fx, fy + 60, 1]]}], "seed": 1})
fill("bg_varanda", [[[fx - 22, fy + 88], [fx - 18, fy + 66], [fx - 8, fy + 58], [fx + 8, fy + 58], [fx + 18, fy + 66], [fx + 22, fy + 88]]], "#cfe8ef", 0.9)
fill("bg_varanda", [[[fx - 4, fy + 88], [fx + 4, fy + 88], [fx + 3, fy + 128], [fx - 3, fy + 128]]], "#f3efe4")
fill("bg_varanda", [[[fx - 12, fy + 74], [fx - 6, fy + 66], [fx, fy + 74], [fx - 6, fy + 82]]], "#d24a4a", 0.9)

print("guia:", {k: v for k, v in (g or {}).items() if k != "outcome"} if isinstance(g, dict) else g)
print(_call("RENDER", {"cut": cut, "frame": 0, "out": f"{OUT}/c006_bg.png"})["path"])
