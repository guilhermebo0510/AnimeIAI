"""Corte C — letreiro "ゴ", passada 1 (coordenadas lidas na grade; refino por medição de borda depois)."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from drive import Drive

PROJ = r"C:\AnimeIAI\calibracao\projetos\c131.aiai"
ids = json.load(open(os.path.join(PROJ, "..", "c131_ids.json")))
d = Drive(PROJ)
RED = {"type": "direct", "rgba": [0.69, 0.08, 0.13, 1.0]}

GO = [(366,462),(410,448),(460,432),(510,413),(550,397),(590,375),(625,357),(645,343),(633,328),(632,318),(645,290),(652,276),
      (668,282),(692,295),(706,286),(722,250),(760,265),(775,278),(768,300),(757,322),(745,318),(722,305),(705,307),(700,318),
      (708,335),(725,343),(750,348),(757,360),(760,400),(767,435),(785,480),(802,535),(820,585),(845,650),(865,695),(865,700),
      (820,720),(770,732),(762,730),(755,722),(740,725),(700,732),(650,747),(600,765),(550,780),(515,800),(487,820),(470,737),
      (500,725),(550,710),(600,687),(650,667),(690,652),(722,640),(722,610),(705,570),(695,530),(675,485),(665,462),(645,454),
      (600,460),(550,474),(475,500),(420,520),(380,546),(368,505)]

if __name__ == "__main__":
    r = d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": 15, "layer": ids["letreiro"], "curves": [
        {"points": [list(p) for p in GO], "width": 2.0, "tip": "marker", "geometry": "polyline", "color": RED, "closed": True}]})
    print("linha:", r.get("description") if isinstance(r, dict) else r)
    r = d.call("FILL", {"cut": ids["cut"], "frame": 15, "layer": ids["letreiro"], "line_layers": [ids["letreiro"]],
                        "seeds": [{"x": 740, "y": 600, "color": RED}], "gap": 0, "expand": 2})
    print("balde:", r.get("description") if isinstance(r, dict) else r)
