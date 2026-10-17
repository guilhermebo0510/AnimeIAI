"""C007 (LS, contra-plongée): a rajada leva pétalas da roseira para o céu enquanto a câmera inclina. Partículas de
pétalas (preset `petalas` ajustado: sobem das roseiras, vento para a direita, rajada no quadro 18), brilho nas pétalas e
camada de efeito com difusão, god rays do sol e vinheta (satsuei)."""
from curta import d, ids, node, OUT

cut = ids["cuts"][6]


def layer(key, kind, index):
    lid = node(f"c7_{key}", key, {"type": "layer", "kind": kind}, cut)
    d.apply({"kind": "move_node", "id": lid, "parent": cut, "index": index})
    return lid


pt = layer("petalas", "particles", 3)
fx = layer("satsuei", "effect", 4)
d.call("PARTICLES", {"layer": pt, "preset": "petalas", "seed": 7, "overrides": {
    "emitter": {"type": "rect", "rect": [0, 1000, 1920, 80]},
    "rate": 20, "bursts": [[18, 45], [40, 20]],
    "speed": [140, 260], "direction": 268, "spread": 30,
    "gravity": [0, -25], "wind": [70, 0], "turbulence": [90, 180], "drag": 0.15,
    "lifetime": [4.5, 6.5], "size": [24, 40], "prewarm": 0.8,
    "colors": [[0.77, 0.18, 0.24, 1.0], [0.88, 0.32, 0.36, 1.0], [0.62, 0.10, 0.16, 1.0], [0.95, 0.55, 0.55, 1.0]]}})
d.call("EFFECTS", {"op": "set", "layer": pt, "effects": [{"effect": {"type": "glow", "threshold": 0.3, "radius": 10, "intensity": 0.35}, "enabled": True}]})
d.call("EFFECTS", {"op": "set", "layer": fx, "effects": [
    {"effect": {"type": "god_rays", "center": [1180, 250], "threshold": 0.85, "length": 0.35, "intensity": 0.45}, "enabled": True},
    {"effect": {"type": "diffusion", "radius": 18, "amount": 0.22, "threshold": 0.55}, "enabled": True},
    {"effect": {"type": "vignette", "amount": 0.28, "softness": 0.6}, "enabled": True}]})
for f in (0, 24, 60, 100, 131):
    print(d.call("RENDER", {"cut": cut, "frame": f, "out": f"{OUT}/c007_q{f}.png"})["path"])
