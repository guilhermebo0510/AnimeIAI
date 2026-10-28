"""Prévia de intercalação de um quadro (R16 MVP-0), ponta a ponta, num projeto de teste próprio (calibracao/tmp/ibp-*):

Parte H — MCP headless de vida longa (servidor stdio com AIAI_HEADLESS_PROJECT), AT-13:
  prévia → render com prévia → descartar → prévia → aceitar → segundo aceite falha → log com `direct_tool_call` e o
  registro `inbetween_origin` no cel; prévia num servidor, servidor some, aceite no servidor novo = erro explícito.

Parte A — app (debug) aberto no projeto, AT-01/03/13/14/15 e a tela:
  prévia pedida pela bridge aparece na tela (modo diretor) com a barra; Corrigir pares por cliques injetados deixa a
  proposta desatualizada (aceite pela bridge falha), Atualizar refaz; Aceitar por clique injetado grava
  `bridge_injected`; a prévia cai no mesmo lugar do cel aceito (camada com movimento em ease e o quadro alvo exposto
  com transformação de cel); mudar a chave com a prévia aberta derruba a prévia com motivo.

Uso: python tools/ibp/ibp_e2e.py [--keep]   (evidências em sprints/registro/evidencias/S26/)
"""
import argparse
import base64
import json
import math
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "csp"))
sys.path.insert(0, os.path.join(HERE, "..", "calib"))
import calib_app as ca  # noqa: E402
import aiai  # noqa: E402
from drive import Drive, DriveError  # noqa: E402

ROOT = ca.ROOT
TMP = ca.TMP
EVID = os.path.join(ROOT, "sprints", "registro", "evidencias", "S26")
MCP = os.path.join(ROOT, "app", "target", "debug", "aiai-mcp.exe")
STUDIO = os.path.join(ROOT, "app", "target", "debug", "aiai-studio.exe")
ALLOWED = r"C:\AnimeIAI\producoes;C:\AnimeIAI\calibracao;C:\AnimeIAI\sprints\registro\evidencias"


def pts(points):
    return [{"x": round(x, 2), "y": round(y, 2), "pressure": 1.0} for x, y in points]


def stroke(sid, points, closed=False, width=4.0):
    return {"id": sid, "points": pts(points), "width": width, "closed": closed, "geometry": "spline", "tip": "g_pen",
            "color": {"type": "direct", "rgba": [0.12, 0.1, 0.1, 1.0]}}


def face(cx, cy, rot_deg, prefix):
    """Cabeça (contorno fechado), dois olhos e boca, girados `rot_deg` em volta de (cx, cy)."""
    r = math.radians(rot_deg)

    def T(x, y):
        return (cx + x * math.cos(r) - y * math.sin(r), cy + x * math.sin(r) + y * math.cos(r))
    head = [T(110 * math.cos(a), 130 * math.sin(a)) for a in [i / 28 * 2 * math.pi for i in range(28)]]
    eye_l = [T(-45 + 8 * k, -20 - 10 * math.sin(k / 4 * math.pi)) for k in range(5)]
    eye_r = [T(13 + 8 * k, -20 - 10 * math.sin(k / 4 * math.pi)) for k in range(5)]
    mouth = [T(-30 + 15 * k, 55 + 12 * math.sin(k / 4 * math.pi)) for k in range(5)]
    return [stroke(prefix + "head", head, True), stroke(prefix + "eyeL", eye_l, width=3.0), stroke(prefix + "eyeR", eye_r, width=3.0),
            stroke(prefix + "mouth", mouth, width=3.0)]


def build(dst):
    """Projeto de teste: corte de 24 quadros, camada vetorial "A" com faixa: A1 (chave) em 0, A3 (intercalação, com
    transformação de cel) em 6, A2 (chave, rosto girado e deslocado) em 12; a camada anda em x com ease."""
    if os.path.exists(dst):
        shutil.rmtree(dst)
    d = Drive(dst, exe=MCP)
    d.call("NEW_PROJECT", {"path": dst, "name": "ibp-teste", "settings": {"width": 1280, "height": 720}})
    ep = d.create("EP", {"type": "episode"})
    sc = d.create("S", {"type": "scene"}, ep)
    cut = d.create("C001", {"type": "cut", "duration": 24}, sc)
    layer = d.create("A", {"type": "layer", "kind": "vector"}, cut)
    d.create("A1", {"type": "cel", "number": 1, "role": "key", "strokes": face(420, 360, 0, "a_")}, layer)
    d.create("A2", {"type": "cel", "number": 2, "role": "key", "strokes": face(700, 330, 18, "b_")}, layer)
    d.create("A3", {"type": "cel", "number": 3, "role": "inbetween", "transform": [40, 0, 1, 0], "strokes": face(560, 345, 9, "c_")}, layer)
    d.apply({"kind": "add_track", "cut": cut, "track": {"id": "trk-a", "name": "A", "kind": "drawing", "layer": layer, "cells": {}}})
    d.apply({"kind": "set_cells", "cut": cut, "track": "trk-a", "cells": [
        {"frame": 0, "cell": {"type": "cel", "number": 1}}, {"frame": 6, "cell": {"type": "cel", "number": 3}}, {"frame": 12, "cell": {"type": "cel", "number": 2}}]})
    d.call("LAYER_MOTION", {"op": "keys", "layer": layer, "property": "offset_x", "keys": [{"frame": 0, "value": 0.0, "ease": "ease_in_out"}, {"frame": 12, "value": 90.0, "ease": "hold"}]})
    return cut, layer


class Mcp:
    """Servidor MCP stdio de vida longa, headless no projeto."""

    def __init__(self, project):
        env = dict(os.environ, AIAI_HEADLESS_PROJECT=project, AIAI_MCP_ALLOWED_DIRS=ALLOWED)
        self.p = subprocess.Popen([MCP], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env)
        self.n = 0
        self.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "ibp_e2e", "version": "1"}})

    def rpc(self, method, params):
        self.n += 1
        self.p.stdin.write((json.dumps({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params}) + "\n").encode())
        self.p.stdin.flush()
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("o servidor MCP fechou")
            m = json.loads(line)
            if m.get("id") == self.n:
                return m

    def tool(self, name, args):
        m = self.rpc("tools/call", {"name": name, "arguments": args})
        if "error" in m:
            return False, m["error"].get("message")
        res = m["result"]
        text = next((c["text"] for c in res.get("content", []) if c.get("type") == "text"), "")
        if res.get("isError"):
            return False, text
        try:
            return True, json.loads(text)
        except json.JSONDecodeError:
            return True, text

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=20)


def bbox(png_path, test):
    from PIL import Image
    im = Image.open(png_path).convert("RGBA")
    w, h = im.size
    px = im.load()
    xs, ys = [], []
    for y in range(0, h):
        for x in range(0, w):
            if test(px[x, y]):
                xs.append(x)
                ys.append(y)
    return (min(xs), min(ys), max(xs), max(ys)) if xs else None


def magenta(p):
    r, g, b, a = p
    return a > 128 and r > 170 and b > 190 and g < 120


def dark(p):
    r, g, b, a = p
    return a > 128 and r < 90 and g < 90 and b < 90


def part_h(res):
    dst = os.path.join(TMP, "ibp-h.aiai")
    cut, layer = build(dst)
    m = Mcp(dst)
    try:
        ok, p = m.tool("aiai_inbetween_preview", {"cut": cut, "layer": layer, "frame": 6})
        assert ok, p
        res["h_preview"] = {k: p[k] for k in ("proposal_id", "frame_a", "frame_b", "key_a", "key_b", "t", "acceptable", "accept_label", "counts")}
        assert (p["frame_a"], p["frame_b"], p["key_a"], p["key_b"]) == (0, 12, 1, 2) and p["acceptable"], p
        out = os.path.join(EVID, "h_with_preview.png")
        ok, r = m.tool("aiai_render", {"with_preview": p["proposal_id"], "out": out})
        assert ok, r
        ok, r = m.tool("aiai_inbetween_discard", {"proposal_id": p["proposal_id"]})
        assert ok, r
        ok, e = m.tool("aiai_inbetween_accept", {"proposal_id": p["proposal_id"]})
        res["h_accept_after_discard"] = e
        assert not ok
        ok, p = m.tool("aiai_inbetween_preview", {"cut": cut, "layer": layer, "frame": 6})
        ok, a = m.tool("aiai_inbetween_accept", {"proposal_id": p["proposal_id"]})
        assert ok, a
        res["h_accept"] = a
        ok, e = m.tool("aiai_inbetween_accept", {"proposal_id": p["proposal_id"]})
        res["h_second_accept"] = e
        assert not ok and "já usada" in e, e
        ok, log = m.tool("aiai_log", {"from": 0, "limit": 1000})
        last = log["entries"][-1]
        res["h_log_last"] = {k: last.get(k) for k in ("description", "author", "source")}
        assert last.get("source") == "direct_tool_call", last
        ok, q = m.tool("aiai_query", {"query": {"kind": "node", "id": a["cel"]}})
        node = q.get("node", q)
        origin = node["data"].get("inbetween_origin")
        res["h_origin"] = origin
        assert origin and origin["source"] == "direct_tool_call" and origin["frame"] == 6, origin
        # dono some: prévia num servidor, aceite noutro
        ok, p = m.tool("aiai_inbetween_preview", {"cut": cut, "layer": layer, "frame": 3})
        assert ok, p
    finally:
        m.close()
    m2 = Mcp(dst)
    try:
        ok, e = m2.tool("aiai_inbetween_accept", {"proposal_id": p["proposal_id"]})
        res["h_lost_owner"] = e
        assert not ok and "não encontrada nesta sessão" in e, e
    finally:
        m2.close()


def widgets():
    return aiai.call("GET_UI_STATE").get("widgets", {})


def click(name):
    w = widgets()[name]
    r = w.get("rect_logical") or w.get("rect")
    x, y = r[0] + r[2] / 2, r[1] + r[3] / 2
    aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y}, {"kind": "mouse_up", "x": x, "y": y}, {"kind": "wait", "frames": 3}]})


def click_xy(x, y):
    aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y}, {"kind": "mouse_up", "x": x, "y": y}, {"kind": "wait", "frames": 3}]})


def part_a(res, keep):
    dst = os.path.join(TMP, "ibp-a.aiai")
    cut, layer = build(dst)
    proc = ca.launch_hidden([STUDIO, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--project", dst, "--workspace", "animacao"])
    try:
        ca.wait_ready(proc)
        ca.settle(30)
        ca.click_widget_if("warn:ok")
        st = aiai.call("GET_UI_STATE")
        assert os.path.normcase(os.path.abspath(st["project"]["path"])) == os.path.normcase(os.path.abspath(dst)), st["project"]
        # prévia pela bridge: aparece na tela (modo diretor)
        p = aiai.call("INBETWEEN_PREVIEW", {"cut": cut, "layer": layer, "frame": 6})
        ca.settle(8)
        ed = aiai.call("GET_UI_STATE")["editor"]
        res["a_ui_after_bridge_preview"] = {"frame": ed.get("frame"), "ibp": ed.get("ibp")}
        assert ed["ibp"] and ed["ibp"]["proposal_id"] == p["proposal_id"], ed.get("ibp")
        aiai.call("SCREENSHOT", {"out": os.path.join(EVID, "a_preview_bar.png")})
        # AT-14: onde a prévia cai = onde o cel aceito cai
        wp = os.path.join(EVID, "a_with_preview.png")
        aiai.call("RENDER", {"with_preview": p["proposal_id"], "out": wp})
        # Corrigir pares: olho esquerdo de A com o direito de B (par errado de propósito)
        click("ibp:pairs")
        cv = aiai.call("GET_UI_STATE")["editor"]["canvas"]
        # placement no quadro 6: offset_x com ease no meio do caminho (as chaves estão no espaço do cel)
        ox = 45.0  # ease_in_out no meio = metade de 90

        def at(x, y):
            return ca.to_screen(cv, x + ox, y)
        ax, ay = 420 - 45 + 16, 360 - 20 - 10  # olho esquerdo de A (ponto do meio)
        r18 = math.radians(18)
        bx_l, by_l = 13 + 16, -20 - 10  # olho direito de B, local
        bx, by = 700 + bx_l * math.cos(r18) - by_l * math.sin(r18), 330 + bx_l * math.sin(r18) + by_l * math.cos(r18)
        click_xy(*at(ax, ay))
        click_xy(*at(bx, by))
        ed = aiai.call("GET_UI_STATE")["editor"]
        res["a_after_pair_clicks"] = ed.get("ibp")
        assert ed["ibp"]["stale"] and ed["ibp"]["manual_pairs"] == [["a_eyeL", "b_eyeR"]], ed.get("ibp")
        aiai.call("SCREENSHOT", {"out": os.path.join(EVID, "a_pairs_stale.png")})
        try:
            aiai.call("INBETWEEN_ACCEPT", {"proposal_id": ed["ibp"]["proposal_id"]})
            raise SystemExit("aceitou proposta desatualizada")
        except Exception as e:
            res["a_stale_accept_error"] = str(e)
        click("ibp:refresh")
        ed = aiai.call("GET_UI_STATE")["editor"]
        res["a_after_refresh"] = ed.get("ibp")
        assert not ed["ibp"]["stale"] and ed["ibp"]["proposal_id"] != p["proposal_id"], ed.get("ibp")
        aiai.call("SCREENSHOT", {"out": os.path.join(EVID, "a_pairs_refreshed.png")})
        # tira o par errado: clica no traço de A do par manual e Delete
        if not ed["ibp"]["pairs_mode"]:
            click("ibp:pairs")
        click_xy(*at(ax, ay))
        aiai.call("INPUT", {"wait": True, "events": [{"kind": "key", "key": "Delete"}, {"kind": "wait", "frames": 3}]})
        ed = aiai.call("GET_UI_STATE")["editor"]
        res["a_after_delete"] = ed.get("ibp")
        assert ed["ibp"]["manual_pairs"] == [] and ed["ibp"]["stale"], ed.get("ibp")
        click("ibp:refresh")
        click("ibp:pairs")
        ed = aiai.call("GET_UI_STATE")["editor"]
        pid = ed["ibp"]["proposal_id"]
        wp = os.path.join(EVID, "a_with_preview.png")
        aiai.call("RENDER", {"with_preview": pid, "out": wp})
        # Aceitar por clique injetado
        log0 = aiai.call("GET_LOG", {"from": 0, "limit": 1000})["total"]
        click("ibp:accept")
        ca.settle(4)
        log = aiai.call("GET_LOG", {"from": 0, "limit": 1000})
        last = log["entries"][-1]
        res["a_log_last"] = {k: last.get(k) for k in ("description", "author", "source")}
        assert log["total"] == log0 + 1 and last.get("source") == "bridge_injected", last
        cel = aiai.call("QUERY", {"query": {"kind": "node", "id": layer}})
        kids = cel.get("node", cel)["children"]
        newest = None
        for k in kids:
            n = aiai.call("QUERY", {"query": {"kind": "node", "id": k}})
            n = n.get("node", n)
            if n["data"].get("inbetween_origin"):
                newest = n
        res["a_origin"] = newest["data"]["inbetween_origin"] if newest else None
        assert newest and newest["data"]["inbetween_origin"]["source"] == "bridge_injected"
        aiai.call("SCREENSHOT", {"out": os.path.join(EVID, "a_accepted.png")})
        # AT-14: o cel aceito renderizado sozinho cai onde a prévia caiu
        lp = os.path.join(EVID, "a_accepted_layer.png")
        aiai.call("RENDER", {"cut": cut, "frame": 6, "layers": [layer], "camera": False, "out": lp})
        b1, b2 = bbox(wp, magenta), bbox(lp, dark)
        res["a_bbox_preview"], res["a_bbox_accepted"] = b1, b2
        assert b1 and b2 and max(abs(u - v) for u, v in zip(b1, b2)) <= 3, (b1, b2)
        # quadro 7 continua mostrando A3 (entrada restauradora)
        q = aiai.call("QUERY", {"query": {"kind": "node", "id": cut}})
        cells = q.get("node", q)["data"]["xsheet"]["tracks"][0]["cells"]
        res["a_cells"] = cells
        assert cells.get("7") == {"type": "cel", "number": 3} or cells.get(7) == {"type": "cel", "number": 3}, cells
        # AT-03: prévia aberta e a chave muda → a prévia cai com motivo
        p = aiai.call("INBETWEEN_PREVIEW", {"cut": cut, "layer": layer, "frame": 3})
        ca.settle(4)
        key2 = None
        for k in kids:
            n = aiai.call("QUERY", {"query": {"kind": "node", "id": k}})
            n = n.get("node", n)
            if n["data"].get("number") == 2:
                key2 = k
        aiai.call("APPLY_COMMAND", {"command": {"kind": "set_props", "id": key2, "name": "A2b", "data": {}}})
        ca.settle(6)
        st = aiai.call("GET_UI_STATE")
        res["a_after_key_change"] = {"ibp": st["editor"].get("ibp"), "toast": st.get("toast") or st["editor"].get("toast")}
        assert st["editor"].get("ibp") is None
        aiai.call("SCREENSHOT", {"out": os.path.join(EVID, "a_invalidated.png")})
        # "Gerar de novo" refaz a prévia com o mesmo pedido
        click("ibp:again")
        ca.settle(4)
        ed = aiai.call("GET_UI_STATE")["editor"]
        res["a_again"] = ed.get("ibp")
        assert ed.get("ibp") and ed["ibp"]["frame"] == 3, ed.get("ibp")
    finally:
        if not keep:
            proc.kill()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--only", choices=["h", "a"])
    a = ap.parse_args()
    os.makedirs(EVID, exist_ok=True)
    res = {}
    try:
        if a.only in (None, "h"):
            part_h(res)
        if a.only in (None, "a"):
            part_a(res, a.keep)
        res["ok"] = True
    finally:
        with open(os.path.join(EVID, "ibp_e2e.json"), "w", encoding="utf-8") as f:
            json.dump(res, f, ensure_ascii=False, indent=1)
        print(json.dumps(res, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
