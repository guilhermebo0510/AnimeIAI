"""Corte C — letreiro "ン", passada 1 (coordenadas lidas na grade)."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from drive import Drive

PROJ = r"C:\AnimeIAI\calibracao\projetos\c131.aiai"
ids = json.load(open(os.path.join(PROJ, "..", "c131_ids.json")))
d = Drive(PROJ)
RED = {"type": "direct", "rgba": [0.69, 0.08, 0.13, 1.0]}

DASH = [(1200,375),(1290,385),(1350,400),(1390,382),(1408,410),(1425,455),(1390,490),(1365,525),(1345,500),(1290,480),(1170,455),(1185,420)]
HOOK = [(1150,632),(1165,655),(1190,665),(1220,680),(1290,695),(1390,698),(1440,680),(1470,650),(1490,600),(1510,540),(1520,500),
        (1530,480),(1550,485),(1545,520),(1520,600),(1500,660),(1485,720),(1465,760),(1440,788),(1390,805),(1340,807),(1290,800),
        (1240,788),(1190,780),(1130,770),(1110,740),(1100,700),(1120,670),(1145,645)]
curves = [{"points": [list(p) for p in c], "width": 2.0, "tip": "marker", "geometry": "polyline", "color": RED, "closed": True} for c in (DASH, HOOK)]
print(d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": 15, "layer": ids["letreiro"], "curves": curves}).get("description"))
print(d.call("FILL", {"cut": ids["cut"], "frame": 15, "layer": ids["letreiro"], "line_layers": [ids["letreiro"]],
                      "seeds": [{"x": 1300, "y": 440, "color": RED}, {"x": 1300, "y": 750, "color": RED}], "gap": 0, "expand": 2}).get("description"))
