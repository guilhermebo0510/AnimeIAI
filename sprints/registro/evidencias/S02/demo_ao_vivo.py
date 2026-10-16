#!/usr/bin/env python3
"""Demo ao vivo da S02: o agente opera o AnimeIAI Studio aberto (--bridge) so por `aiai-mcp call` (CLI, modo app).

Cria projeto, episodio/cena/corte 1920x1080, camadas BG (raster) / A-cor (fill -> A-linha) / A-linha (vetor) com cel 1,
degrade de ceu, cabeca de personagem por curvas, LINT + FILL por paleta, RENDER inline salvo em PNG, SCREENSHOT da janela,
UNDO/REDO confirmados por RENDER (hash do PNG). Escreve o log (comandos e saidas) em demo_ao_vivo.log.md.

Uso: python demo_ao_vivo.py   (o app precisa estar aberto: tools\\relanca.ps1)
"""
import hashlib
import json
import subprocess
import sys
import time
from pathlib import Path

MCP = r"C:\AnimeIAI\app\target\debug\aiai-mcp.exe"
EVID = Path(r"C:\AnimeIAI\sprints\registro\evidencias\S02")
PROJ = r"C:\AnimeIAI\producoes\s02-ao-vivo.aiai"
LOG = []


def call(action, params=None, png=None, label=None, timeout=240):
    args = [MCP, "call", action, json.dumps(params or {}, ensure_ascii=False)]
    if png:
        args += ["--png", str(png)]
    t0 = time.time()
    p = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", timeout=timeout)
    dt = time.time() - t0
    LOG.append(f"### {label or action}\n\n```\naiai-mcp call {action} '{json.dumps(params or {}, ensure_ascii=False)[:600]}'" + (f" --png {png}" if png else "") + f"\n```\n\n({dt:.2f} s, exit {p.returncode})\n\n```json\n{(p.stdout or p.stderr).strip()[:1500]}\n```\n")
    if p.returncode != 0:
        print(p.stderr, file=sys.stderr)
        raise SystemExit(f"{action} falhou: {p.stderr.strip()}")
    return json.loads(p.stdout)


def node(name, data, parent=None):
    c = {"kind": "create_node", "name": name, "data": data}
    if parent:
        c["parent"] = parent
    return call("APPLY_COMMAND", {"command": c}, label=f"criar {name}")["created"][0]


def rgba(hexcolor):
    h = hexcolor.lstrip("#")
    return [int(h[i:i + 2], 16) / 255 for i in (0, 2, 4)] + [1.0]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()[:16]


def main():
    st = call("GET_STATUS", label="status do app")
    assert st["mode"] == "app", st
    call("NEW_PROJECT", {"name": "S02 ao vivo", "path": PROJ, "overwrite": True, "settings": {"width": 1920, "height": 1080}})
    ep = node("EP01", {"type": "episode", "number": 1})
    sc = node("Cena 1", {"type": "scene", "number": 1}, ep)
    cut = node("C001", {"type": "cut", "number": 1, "duration": 48}, sc)
    # filhos de baixo para cima: BG, cor, linha
    bg = node("BG", {"type": "layer", "kind": "raster"}, cut)
    cor = node("A-cor", {"type": "layer", "kind": "fill"}, cut)
    linha = node("A-linha", {"type": "layer", "kind": "vector"}, cut)
    for lid, nm in ((bg, "BG1"), (cor, "A-cor1"), (linha, "A-linha1")):
        node(nm, {"type": "cel", "number": 1}, lid)
    call("APPLY_COMMAND", {"command": {"kind": "set_props", "id": cor, "data": {"line_refs": [linha]}}}, label="line_refs de A-cor -> A-linha")

    colors = [("linha", "#3a2b2b"), ("pele", "#fbdcc4"), ("cabelo", "#e8b23a"), ("branco", "#ffffff"), ("olho", "#2f6fd6"), ("boca", "#d9534f")]
    pal = node("Settei", {"type": "palette", "colors": [{"id": i, "name": i, "group": i, "role": "normal", "rgba": rgba(h)} for i, h in colors], "active_variant": ""})
    P = lambda c: {"type": "palette", "palette": pal, "color": c}

    # fundo: ceu em degrade + brilho de sol (radial) + colina pintada com pincel guache
    call("GRADIENT", {"cut": cut, "frame": 0, "layer": bg, "kind": "linear", "from": [0, 0], "to": [0, 760], "stops": [[0.0, "#2f6fd6ff"], [1.0, "#cfe9ffff"]]}, label="GRADIENT ceu")
    call("GRADIENT", {"cut": cut, "frame": 0, "layer": bg, "kind": "radial", "from": [1560, 230], "to": [1560, 520], "opacity": 0.85, "stops": [[0.0, "#fff3c0ff"], [1.0, "#fff3c000"]]}, label="GRADIENT brilho do sol")
    call("PAINT", {"cut": cut, "frame": 0, "layer": bg, "preset": "Guache seco", "preset_overrides": {"size": 220}, "color": {"type": "direct", "rgba": rgba("#5aa469")},
                   "strokes": [{"points": [[-40, 1000], [420, 930], [940, 1000], [1460, 940], [1960, 1000]]}, {"points": [[-40, 1070], [600, 1040], [1300, 1080], [1960, 1050]]}], "seed": 7}, label="PAINT colina (preset por nome)")
    r_bg = call("RENDER", {"cut": cut, "frame": 0, "background": "#ffffff"}, png=EVID / "ao-vivo-1-fundo.png", label="RENDER so fundo")

    # cabeca: cabelo (fechado), rosto, olhos (espelhados), pupilas, boca, nariz e sobrancelhas
    ink = P("linha")
    hair = {"points": [[770, 470], [756, 340], [830, 220], [960, 176], [1090, 220], [1164, 340], [1150, 470], [1100, 418], [1040, 472], [960, 410], [880, 472], [820, 418]],
            "closed": True, "width": 7, "widths": [0.6, 1.0, 0.6], "color": ink}
    face = {"points": [[770, 470], [790, 610], [860, 730], [960, 786], [1060, 730], [1130, 610], [1150, 470]], "width": 7, "widths": [0.7, 1.0, 0.7], "color": ink}
    eye = {"points": [[842, 568], [888, 538], [936, 566], [888, 604]], "closed": True, "width": 5, "color": ink}
    pupil = {"points": [[876, 560], [890, 552], [904, 562], [890, 582]], "closed": True, "width": 4, "color": ink}
    brow = {"points": [[836, 520], [886, 500], [940, 516]], "width": 6, "widths": [0.4, 1.0, 0.5], "color": ink}
    mouth = {"points": [[914, 700], [960, 712], [1006, 700], [960, 736]], "closed": True, "width": 5, "color": ink}
    nose = {"points": [[962, 600], [950, 648], [972, 652]], "width": 4, "color": ink}
    neck = [{"points": [[900, 756], [890, 860]], "width": 6, "color": ink}, {"points": [[1020, 756], [1030, 860]], "width": 6, "color": ink}]
    call("DRAW_CURVES", {"cut": cut, "frame": 0, "layer": linha, "curves": [hair, face, mouth, nose] + neck}, label="DRAW_CURVES cabelo, rosto, boca, nariz, pescoco")
    call("DRAW_CURVES", {"cut": cut, "frame": 0, "layer": linha, "mirror_x": 960, "curves": [eye, pupil, brow]}, label="DRAW_CURVES olhos, pupilas e sobrancelhas (mirror_x=960)")
    call("RENDER", {"cut": cut, "frame": 0, "layers": [linha], "background": "#ffffff"}, png=EVID / "ao-vivo-2-linha.png", label="RENDER so a linha")

    # LINT -> regioes sem cor -> FILL com a paleta
    lint = call("LINT", {"cut": cut, "frame": 0, "layer": cor, "gap": 3, "min_area": 40}, label="LINT antes de pintar")
    regs = lint["unpainted"]
    print("regioes sem cor:", len(regs))
    seeds = []
    eyes = {}
    for r in regs:
        x0, y0, w, h = r["bounds"]
        cx = x0 + w / 2
        seeds_color = None
        if y0 < 330:
            seeds_color, role = "cabelo", "cabelo"
        elif r["area"] > 20000:
            seeds_color, role = "pele", "rosto"
        elif y0 > 660:
            seeds_color, role = "boca", "boca"
        else:
            side = "esq" if cx < 960 else "dir"
            eyes.setdefault(side, []).append(r)
            continue
        seeds.append({"x": r["point"][0], "y": r["point"][1], "color": P(seeds_color)})
        print("  ", role, r["area"], r["bounds"])
    for side, lst in eyes.items():
        lst.sort(key=lambda r: -r["area"])
        seeds.append({"x": lst[0]["point"][0], "y": lst[0]["point"][1], "color": P("branco")})
        for r in lst[1:]:
            seeds.append({"x": r["point"][0], "y": r["point"][1], "color": P("olho")})
        print("   olho", side, [r["area"] for r in lst])
    call("FILL", {"cut": cut, "frame": 0, "layer": cor, "seeds": seeds, "gap": 3, "expand": 2}, label="FILL de todas as regioes (paleta)")
    after = call("LINT", {"cut": cut, "frame": 0, "layer": cor, "gap": 3, "min_area": 40}, label="LINT depois de pintar")
    print("sem cor depois do balde:", len(after["unpainted"]), "pintadas:", after["painted"])
    call("RENDER", {"cut": cut, "frame": 0, "background": "#ffffff"}, png=EVID / "ao-vivo-3-final.png", label="RENDER final")
    h_final = sha(EVID / "ao-vivo-3-final.png")

    # janela do app (framebuffer do egui)
    call("UI_ACTION", {"action": "set_workspace", "workspace": "pintura"}, label="UI_ACTION set_workspace pintura")
    time.sleep(0.8)
    call("SCREENSHOT", {}, png=EVID / "ao-vivo-4-janela.png", label="SCREENSHOT da janela")
    st = call("GET_STATUS", label="status depois do desenho")

    # UNDO/REDO confirmados pelo render
    call("UNDO", label="UNDO (o balde)")
    call("RENDER", {"cut": cut, "frame": 0, "background": "#ffffff"}, png=EVID / "ao-vivo-5-undo.png", label="RENDER depois do UNDO")
    h_undo = sha(EVID / "ao-vivo-5-undo.png")
    call("REDO", label="REDO")
    call("RENDER", {"cut": cut, "frame": 0, "background": "#ffffff"}, png=EVID / "ao-vivo-6-redo.png", label="RENDER depois do REDO")
    h_redo = sha(EVID / "ao-vivo-6-redo.png")
    call("SAVE_PROJECT", label="SAVE_PROJECT")
    verdict = f"hash final={h_final} | apos UNDO={h_undo} | apos REDO={h_redo} | undo muda a imagem: {h_undo != h_final} | redo restitui: {h_redo == h_final}"
    print(verdict)
    LOG.append(f"## Veredito (hash dos PNG renderizados)\n\n{verdict}\n\nStatus final do app: `{json.dumps(st, ensure_ascii=False)[:700]}`\n")
    (EVID / "demo_ao_vivo.log.md").write_text("# S02 - demo ao vivo (aiai-mcp call -> app aberto)\n\nGerado por `demo_ao_vivo.py`.\n\n" + "\n".join(LOG), encoding="utf-8")


if __name__ == "__main__":
    main()
