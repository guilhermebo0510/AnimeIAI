"""Ensaio do rig: desenha um rosto grande num corte de teste e renderiza. uso: python ensaio.py hana [mood] [mouth] [yaw]
Roda num projeto à parte (calibracao/tmp/ensaio-rig.aiai), nunca no arquivo de produção do curta."""
import os, sys
os.environ.setdefault("AIAI_CURTA_PROJ", "C:/AnimeIAI/calibracao/tmp/ensaio-rig.aiai")
os.environ.setdefault("AIAI_CURTA_IDS", "C:/AnimeIAI/calibracao/tmp/ensaio-rig.ids.json")
os.environ.setdefault("AIAI_CURTA_OUT", "C:/AnimeIAI/calibracao/tmp/ensaio")
os.makedirs(os.environ["AIAI_CURTA_OUT"], exist_ok=True)
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
if os.environ.get("BLUSH"):
    h.blush = float(os.environ["BLUSH"])
h.sparkle = os.environ.get("SPARKLE") == "1"
strokes, seeds, shapes = rig.bust(h, who, mood, 1.0, mouth, breath=float(os.environ.get("BREATH", "0"))) if BUST else getattr(rig, "face_" + who)(h, mood, 1.0, mouth)
# a mesma pintura da produção (curta.paint): sementes de reserva só onde não há tinta, frestas pelo próprio balde
from curta import paint
lint = paint(cut, 0, L, [(who, strokes, seeds, shapes, h.top, h.H)])
print("regiões sem cor:", len(lint["unpainted"]), [u["point"] for u in lint["unpainted"]][:10])
print(d.call("RENDER", {"cut": cut, "frame": 0, "background": "#e8e4dc", "out": f"{OUT}/ensaio_{who}_{mood}_{mouth}_{int(yaw)}.png"})["path"])
