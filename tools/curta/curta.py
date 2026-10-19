"""Micro anime 「最後の一輪」: projeto e ids. Tudo passa pelo aiai-mcp (mesmo Command Bus das ferramentas MCP)."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "calib"))
from drive import Drive

ROOT = "C:/AnimeIAI/producoes/micro-anime"
# AIAI_CURTA_PROJ aponta para outro projeto (ensaios do rig ficam fora do arquivo de produção)
PROJ = os.environ.get("AIAI_CURTA_PROJ", ROOT + "/ultima-rosa.aiai")
OUT = ROOT + "/saida"
IDS_F = os.environ.get("AIAI_CURTA_IDS", ROOT + "/ids.json")
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


# frestas (cantos de olho, entre dedos, junções) que o balde fecha com a cor em volta (FILL close_leftovers);
# regiões maiores sem tinta ficam no lint e pedem semente explícita no rig
LEFTOVER_PX = 300


def paint(cut, frame, L, drawings, gap=4):
    """Desenha [(quem, traços, sementes, formas, topo_y, H)] nas camadas L (um DRAW_CURVES, um FILL por personagem,
    formas pintadas). Frestas de até LEFTOVER_PX px recebem a cor em volta pelo próprio balde; o resto que sobrar
    sem tinta volta no lint (nada de adivinhar a cor por posição). Devolve o lint final."""
    import rig
    allc = [c for _, st, _, _, _, _ in drawings for c in st]
    d.call("DRAW_CURVES", {"cut": cut, "frame": frame, "layer": L["linha"], "curves": allc})
    for k, (who, _, seeds, _, _, _) in enumerate(drawings):
        pal = PALETTE[who]
        fs = [{"x": s["x"], "y": s["y"], "color": {"type": "direct", "rgba": rig.hexrgba(pal[s["name"]])}} for s in seeds
              if s["name"] in pal and 0 <= s["x"] < 1920 and 0 <= s["y"] < 1080]  # partes cortadas pelo quadro ficam de fora
        req = {"cut": cut, "frame": frame, "layer": L["cor"], "seeds": fs, "gap": gap}
        if k == len(drawings) - 1:
            req["close_leftovers"] = LEFTOVER_PX  # depois de todas as sementes
        d.call("FILL", req)
    for who, _, _, shapes, _, _ in drawings:
        pal = PALETTE[who]
        groups = {}
        for name, pts in shapes:
            if name in pal:
                layer = L["brilho"] if name == "brilho" else (L["sombra"] if name.endswith("_sombra") or name.endswith("_realce") else L["olhos"])
                groups.setdefault((layer, name), []).append({"points": pts})
        for (layer, name), shp in groups.items():
            d.call("FILL_SHAPE", {"cut": cut, "frame": frame, "layer": layer, "shapes": shp, "color": {"type": "direct", "rgba": rig.hexrgba(pal[name])}})
    return d.call("LINT", {"cut": cut, "frame": frame, "layer": L["cor"], "gap": gap, "min_area": 30})
