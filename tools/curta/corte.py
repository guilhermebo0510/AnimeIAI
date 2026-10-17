"""Corte de rosto falando: desenha o personagem (rig) em cels por combinação olho × boca e monta o timesheet de todas as
camadas a partir do lip-sync (aiai_lipsync sobre a fala do corte) e de um roteiro de piscadas.

Numeração dos cels: 10 * olho + boca, olho 0 = aberto, 1 = meio, 2 = fechado; boca 1..6 = closed, a, i, u, e, o.
"""
from curta import d, ids, node, save_ids, ensure_layers, paint
import rig

MOUTHS = ["closed", "a", "i", "u", "e", "o"]
EYES = [1.0, 0.45, 0.0]
LAYER_ORDER = ["cor", "sombra", "olhos", "linha", "brilho"]


def cel_no(eye, mouth):
    return 10 * eye + MOUTHS.index(mouth) + 1


def place_dialogue(cut, key, start_frame, character):
    """Põe a fala `ids[key]` (asset do TTS) na trilha de diálogo do corte, uma vez."""
    data = d.query({"kind": "node", "id": cut})["data"]
    audio = [a for a in data.get("audio", []) if a.get("id") != key]
    audio.append({"id": key, "asset": ids[key], "start_frame": start_frame, "character": ids[character], "track": "dialogue"})
    d.apply({"kind": "set_props", "id": cut, "data": {"audio": audio}})
    return key


def set_track(cut, layer, name, kind, cells):
    """Substitui a trilha da camada no timesheet (uma por camada) por `cells` {quadro: nº do cel}."""
    xs = d.query({"kind": "node", "id": cut})["data"].get("xsheet", {"tracks": []})
    for t in xs["tracks"]:
        if t.get("layer") == layer:
            d.apply({"kind": "remove_track", "cut": cut, "track": t["id"]})
    d.apply({"kind": "add_track", "cut": cut, "track": {"id": f"t_{layer[-12:]}", "name": name, "kind": kind, "layer": layer,
                                                         "cells": {str(f): {"type": "cel", "number": n} for f, n in cells.items()}}})


def face_cut(cut, prefix, who, head, mood, combos, base_index=1, bust=True, bottom=2.45, draw=True):
    """Desenha `combos` [(olho, boca)] nas camadas do personagem do corte (criadas se faltarem). `head`: (cx, top, H, yaw, roll).
    Devolve as camadas {nome: id}."""
    L = ensure_layers(prefix, cut)
    for i, k in enumerate(LAYER_ORDER):
        d.apply({"kind": "move_node", "id": L[k], "parent": cut, "index": base_index + i})
    nums = [cel_no(e, m) for e, m in combos]
    if not draw:
        return L
    # cels zerados com os números das combinações
    for k in LAYER_ORDER:
        kids = d.query({"kind": "node", "id": L[k]})["children"]
        have = {}
        for c in kids:
            n = d.query({"kind": "node", "id": c})["data"].get("number")
            have[n] = c
        for n in nums:
            if n not in have:
                have[n] = d.create(str(n), {"type": "cel", "number": n}, L[k])
            d.apply({"kind": "replace_data", "id": have[n], "data": {"type": "cel", "number": n}})
    # timesheet de rascunho: quadro i mostra o cel da combinação i
    for k in LAYER_ORDER:
        set_track(cut, L[k], k, "drawing", {i: n for i, n in enumerate(nums)})
    cx, top, H, yaw, roll = head
    for i, (e, m) in enumerate(combos):
        h = rig.Head(cx, top, H, yaw, roll)
        if bust:
            st, sd, sh = rig.bust(h, who, mood, EYES[e], m, bottom=bottom)
        else:
            st, sd, sh = (rig.face_hana if who == "hana" else rig.face_ren)(h, mood, EYES[e], m)
        lint = paint(cut, i, L, [(who, st, sd, sh, top, H)])
        print(f"  cel {nums[i]} ({e},{m}): sem cor {len(lint['unpainted'])}")
    return L


def lipsync_cells(cut, placement, layer, duration, blinks=(), lead=1):
    """Exposições finais {quadro: cel}: boca do lip-sync × piscadas. `blinks`: quadros iniciais (meio, fechado, meio)."""
    mapping = {m: MOUTHS.index(m) + 1 for m in MOUTHS}
    r = d.call("LIPSYNC", {"cut": cut, "placement": placement, "mouth_layer": layer, "mapping": mapping, "lead_frames": lead})
    # a trilha de boca criada pelo lip-sync é só o meio de leitura: as exposições vão para as trilhas das camadas
    xs = d.query({"kind": "node", "id": cut})["data"]["xsheet"]
    for t in xs["tracks"]:
        if t.get("kind") == "mouth":
            d.apply({"kind": "remove_track", "cut": cut, "track": t["id"]})
    mouth = {int(f): int(n) for f, n in r["exposures"]}
    eye = {}
    for b in blinks:
        eye.update({b: 1, b + 1: 2, b + 2: 1, b + 3: 0})
    cur_m, cur_e, out = 1, 0, {}
    for f in range(duration):
        cur_m = mouth.get(f, cur_m)
        cur_e = eye.get(f, cur_e)
        out[f] = 10 * cur_e + cur_m
    # só as mudanças
    prev, cells = None, {}
    for f in range(duration):
        if out[f] != prev:
            cells[f] = out[f]
            prev = out[f]
    return cells, r
