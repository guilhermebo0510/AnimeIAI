"""Micro anime 「最後の一輪」: projeto e ids. Tudo passa pelo aiai-mcp (mesmo Command Bus das ferramentas MCP)."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "calib"))
from drive import Drive

ROOT = "C:/AnimeIAI/producoes/micro-anime"
PROJ = ROOT + "/ultima-rosa.aiai"
OUT = ROOT + "/saida"
IDS_F = ROOT + "/ids.json"
d = Drive(PROJ)
ids = json.load(open(IDS_F, encoding="utf-8")) if os.path.exists(IDS_F) else {}


def save_ids():
    json.dump(ids, open(IDS_F, "w", encoding="utf-8"), indent=1, ensure_ascii=False)


def node(key, name, data, parent=None):
    if key not in ids:
        ids[key] = d.create(name, data, parent)
        save_ids()
    return ids[key]


def rgba(h, a=1.0):
    return [int(h[i:i + 2], 16) / 255 for i in (1, 3, 5)] + [a]


def col(h, a=1.0):
    return {"type": "direct", "rgba": rgba(h, a)}


# cores de settei por personagem (nome da semente/forma → hex)
PALETTE = {
    "hana": {"pele": "#f6d5c3", "pele_sombra": "#e0ad9a", "cabelo": "#9b4a32", "cabelo_sombra": "#6e2f20", "olho": "#3f8a55",
             "olho_claro": "#7cc28a", "pupila": "#1d3d27", "branco_olho": "#fbf8f4", "boca": "#8a3434", "lingua": "#d9707a",
             "presilha": "#5fae6a", "blusa": "#f3efe4", "blusa_sombra": "#d6cfbd", "avental": "#5d7048", "bota": "#6b4a32", "brilho": "#ffffff"},
    "ren": {"pele": "#f3d0bb", "pele_sombra": "#d9a690", "cabelo": "#2b2a35", "cabelo_realce": "#4f5068", "olho": "#b07a2a",
            "olho_claro": "#d9a85a", "pupila": "#4a2e10", "branco_olho": "#fbf8f4", "boca": "#7a3030", "lingua": "#d06a72",
            "bandana": "#f7f5ef", "bandana_sombra": "#d8d2c4", "camisa": "#6f8fb3", "camisa_sombra": "#526f90", "avental": "#efe2c4",
            "calca": "#4a4752", "sapato": "#3a2c25", "brilho": "#ffffff"},
}
LAYER_KINDS = (("cor", "fill"), ("sombra", "raster"), ("olhos", "raster"), ("linha", "vector"), ("brilho", "raster"))


def ensure_layers(prefix, cut, existing=None):
    """Camadas de personagem num corte (cor ← linha, sombra recortada na cor, olhos, linha, brilho), na ordem certa.
    `existing`: {nome: id} de camadas já criadas (ex.: as do model sheet)."""
    L = dict(existing or {})
    for k, kind in LAYER_KINDS:
        if k not in L:
            L[k] = node(f"{prefix}_{k}", k, {"type": "layer", "kind": kind}, cut)
            node(f"{prefix}_{k}_c1", "1", {"type": "cel", "number": 1}, L[k])
    d.apply({"kind": "set_props", "id": L["cor"], "data": {"line_refs": [L["linha"]]}})
    d.apply({"kind": "set_props", "id": L["sombra"], "data": {"clip_to_below": True}})
    return L


def paint(cut, frame, L, drawings, gap=4):
    """Desenha [(quem, traços, sementes, formas, topo_y, H)] nas camadas L (um DRAW_CURVES, um FILL por personagem,
    formas pintadas, sobras pequenas preenchidas pela cor vizinha). Devolve o lint final."""
    import rig
    allc = [c for _, st, _, _, _, _ in drawings for c in st]
    d.call("DRAW_CURVES", {"cut": cut, "frame": frame, "layer": L["linha"], "curves": allc})
    for who, _, seeds, _, _, _ in drawings:
        pal = PALETTE[who]
        fs = [{"x": s["x"], "y": s["y"], "color": {"type": "direct", "rgba": rig.hexrgba(pal[s["name"]])}} for s in seeds if s["name"] in pal]
        d.call("FILL", {"cut": cut, "frame": frame, "layer": L["cor"], "seeds": fs, "gap": gap})
    for who, _, _, shapes, _, _ in drawings:
        pal = PALETTE[who]
        groups = {}
        for name, pts in shapes:
            if name in pal:
                layer = L["brilho"] if name == "brilho" else (L["sombra"] if name.endswith("_sombra") or name.endswith("_realce") else L["olhos"])
                groups.setdefault((layer, name), []).append({"points": pts})
        for (layer, name), shp in groups.items():
            d.call("FILL_SHAPE", {"cut": cut, "frame": frame, "layer": layer, "shapes": shp, "color": {"type": "direct", "rgba": rig.hexrgba(pal[name])}})
    lint = d.call("LINT", {"cut": cut, "frame": frame, "layer": L["cor"], "gap": gap, "min_area": 30})
    small = [u for u in lint["unpainted"] if u["area"] < 4000]
    if small:
        seeds = []
        for u in small:
            x, y = u["point"]
            # personagem mais perto (pelo topo da cabeça) decide; acima do queixo = cabelo, abaixo = roupa
            who, _, _, _, top, H = min(drawings, key=lambda dr: abs(dr[4] + dr[5] * 0.5 - y) + abs(x - dr[2][0]["x"] if dr[2] else 0) * 0.1)
            name = "cabelo" if y < top + H else ("blusa" if who == "hana" else "camisa")
            seeds.append({"x": x, "y": y, "color": {"type": "direct", "rgba": rig.hexrgba(PALETTE[who][name])}})
        d.call("FILL", {"cut": cut, "frame": frame, "layer": L["cor"], "seeds": seeds, "gap": gap})
        lint = d.call("LINT", {"cut": cut, "frame": frame, "layer": L["cor"], "gap": gap, "min_area": 30})
    return lint
