"""Ensaio do rig: desenha um rosto grande num corte de teste e renderiza. uso: python ensaio.py hana [mood] [mouth] [yaw]
Roda num projeto à parte (calibracao/tmp/ensaio-rig.aiai), nunca no arquivo de produção do curta."""
import os, sys
os.environ.setdefault("AIAI_CURTA_PROJ", "C:/AnimeIAI/calibracao/tmp/ensaio-rig.aiai")
os.environ.setdefault("AIAI_CURTA_IDS", "C:/AnimeIAI/calibracao/tmp/ensaio-rig.ids.json")
if not os.path.exists(os.environ["AIAI_CURTA_PROJ"] + "/project.json"):
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "calib"))
    from drive import Drive
    Drive(os.environ["AIAI_CURTA_PROJ"]).call("NEW_PROJECT", {"name": "Ensaio do rig", "path": os.environ["AIAI_CURTA_PROJ"]})
from curta import d, ids, node, save_ids, OUT
import rig

COLORS = {"hana": {"pele": "#f6d5c3", "cabelo": "#9b4a32", "olho": "#3f8a55", "pupila": "#1d3d27", "branco_olho": "#fbf8f4",
                   "boca": "#8a3434", "lingua": "#d9707a", "presilha": "#5fae6a", "olho_claro": "#7cc28a", "brilho": "#ffffff",
                   "cabelo_sombra": "#6e2f20", "pele_sombra": "#e0ad9a", "blusa": "#f3efe4", "blusa_sombra": "#d6cfbd", "avental": "#5d7048"},
          "ren": {"pele": "#f3d0bb", "cabelo": "#2b2a35", "olho": "#b07a2a", "pupila": "#4a2e10", "branco_olho": "#fbf8f4",
                  "boca": "#7a3030", "lingua": "#d06a72", "bandana": "#f7f5ef", "olho_claro": "#d9a85a", "brilho": "#ffffff",
                  "cabelo_realce": "#4f5068", "bandana_sombra": "#d8d2c4", "pele_sombra": "#d9a690", "camisa": "#6f8fb3", "camisa_sombra": "#526f90", "avental": "#efe2c4"}}
who = sys.argv[1] if len(sys.argv) > 1 else "hana"
mood = sys.argv[2] if len(sys.argv) > 2 else "neutral"
mouth = sys.argv[3] if len(sys.argv) > 3 else "closed"
yaw = float(sys.argv[4]) if len(sys.argv) > 4 else 0.0
ep = node("ep", "EP01", {"type": "episode", "number": 1})
sc = node("sc_ensaio", "Ensaio", {"type": "scene", "number": 99}, ep)
cut = node("cut_ensaio", "Ensaio do rig", {"type": "cut", "number": 999, "duration": 1}, sc)
L = {}
for k, kind in (("cor", "fill"), ("sombra", "raster"), ("olhos", "raster"), ("linha", "vector"), ("brilho", "raster")):
    L[k] = node("ens_" + k, k, {"type": "layer", "kind": kind}, cut)
    node("ens_" + k + "_c1", "1", {"type": "cel", "number": 1}, L[k])
d.apply({"kind": "set_props", "id": L["cor"], "data": {"line_refs": [L["linha"]]}})
d.apply({"kind": "set_props", "id": L["sombra"], "data": {"clip_to_below": True}})
for i, k in enumerate(L):
    d.apply({"kind": "move_node", "id": L[k], "parent": cut, "index": i})
for k in L:
    d.apply({"kind": "replace_data", "id": ids["ens_" + k + "_c1"], "data": {"type": "cel", "number": 1}})
BUST = os.environ.get("BUSTO") == "1"
h = rig.Head(960, 80, 400, yaw) if BUST else rig.Head(960, 120, 760, yaw)
strokes, seeds, shapes = rig.bust(h, who, mood, 1.0, mouth) if BUST else getattr(rig, "face_" + who)(h, mood, 1.0, mouth)
d.call("DRAW_CURVES", {"cut": cut, "frame": 0, "layer": L["linha"], "curves": strokes})
pal = COLORS[who]
fs = [{"x": s["x"], "y": s["y"], "color": {"type": "direct", "rgba": rig.hexrgba(pal[s["name"]])}} for s in seeds if s["name"] in pal]
r = d.call("FILL", {"cut": cut, "frame": 0, "layer": L["cor"], "seeds": fs, "gap": 4})
for name, shape in shapes:
    layer = L["brilho"] if name == "brilho" else (L["sombra"] if name.endswith("_sombra") or name.endswith("_realce") else L["olhos"])
    d.call("FILL_SHAPE", {"cut": cut, "frame": 0, "layer": layer, "shapes": [{"points": shape}], "color": {"type": "direct", "rgba": rig.hexrgba(pal[name])}})
lint = d.call("LINT", {"cut": cut, "frame": 0, "layer": L["cor"], "gap": 4, "min_area": 30})
# sobras pequenas entre mechas: cor do cabelo
small = [u for u in lint["unpainted"] if u["area"] < 4000]
if small:
    roupa = "blusa" if who == "hana" else "camisa"
    d.call("FILL", {"cut": cut, "frame": 0, "layer": L["cor"], "gap": 4, "seeds": [{"x": u["point"][0], "y": u["point"][1], "color": {"type": "direct", "rgba": rig.hexrgba(pal["cabelo" if u["point"][1] < h.top + h.H else roupa])}} for u in small]})
    lint = d.call("LINT", {"cut": cut, "frame": 0, "layer": L["cor"], "gap": 4, "min_area": 30})
print("regiões sem cor:", len(lint["unpainted"]), [u["point"] for u in lint["unpainted"]][:10])
print(d.call("RENDER", {"cut": cut, "frame": 0, "background": "#e8e4dc", "out": f"{OUT}/ensaio_{who}_{mood}_{mouth}_{int(yaw)}.png"})["path"])
