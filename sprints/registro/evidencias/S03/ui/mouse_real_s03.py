#!/usr/bin/env python3
"""Roteiro de mouse/teclado REAIS (SendInput) do S03/B2: criar projeto, camadas, desenhar com a caneta, balde, pincel,
visibilidade, desfazer/refazer, zoom/pan/rotacao do canvas, cels e exposicao na timeline, scrub, reproducao.

Pre-requisitos: app aberto com a bridge (tools\\relanca.ps1) e `tools\\harness\\watch_input.ps1` em paralelo (corrida contaminada
se aparecer evento FISICO). Todo clique/arrasto sai do SendInput; o estado e lido de volta por GET_UI_STATE/QUERY/RENDER.
Capturas em evidencias\\S03\\ui\\NN-*.png (antes/depois de cada passo).
"""
import json
import sys
import time

import hx
from hx import call, check, data, state, ui

CUT = None


def px(cut, frame, x, y):
    """Pixel do RENDER (CPU, o mesmo do agente) em coordenadas do desenho."""
    import base64
    import io

    from PIL import Image

    r = data("RENDER", {"cut": cut, "frame": frame, "inline": True})
    img = Image.open(io.BytesIO(base64.b64decode(r["png_base64"]))).convert("RGBA")
    return img.getpixel((x, y))


def strokes(cel):
    n = data("QUERY", {"query": {"kind": "node", "id": cel}})["data"]
    return n.get("strokes", [])


def layer_cel(layer, nth=0):
    return data("QUERY", {"query": {"kind": "tree", "id": layer}})["children"][nth]["id"]


def circle(cx, cy, rx, ry, n=48, wobble=0.0):
    import math

    pts = []
    for i in range(n + 1):
        a = 2 * math.pi * i / n
        k = 1.0 + wobble * math.sin(3 * a + 0.5)
        pts.append((round(cx + rx * k * math.cos(a)), round(cy + ry * k * math.sin(a))))
    return pts


def wait_latency(n_before, timeout=20):
    t0 = time.time()
    while len(state()["editor"]["latency_log"]) <= n_before:
        if time.time() - t0 > timeout:
            raise SystemExit("o canvas nao atualizou depois da edicao")
        time.sleep(0.05)


def main():
    global CUT
    ui("foco; max; wait 700")
    s0 = state()
    hx.LOG.append(f"Estado inicial: workspace={s0['workspace']}, janela={s0['window']['size_physical']}, ppp={s0['pixels_per_point']}, titulo={s0['window']['title']!r}")

    # ---- 1) projeto novo pela interface (Arquivo > Novo), nome e inicio da estrutura
    x, y = hx.center("menu:file")
    ui(f"click {x} {y}; wait 300")
    hx.click("menu_item:file.new", wait=500)
    ui("wait 200")
    hx.cap("00-projeto-vazio.png")
    check(state()["editor"]["cut"] is None, "projeto novo e vazio (sem corte)")
    x, y = hx.center("project:name")
    ui(f"click {x} {y}; key ctrl+a; type Teste de mouse S03; key enter; wait 300")
    check(state()["project"]["name"] == "Teste de mouse S03", f"nome do projeto digitado: {state()['project']['name']!r}")
    hx.click("project:quick_start", wait=900)
    st = state()
    CUT = st["editor"]["cut"]
    check(CUT is not None and st["editor"]["canvas"]["size"] == [1920, 1080], f"corte criado: canvas {st['editor']['canvas']['size']}")
    hx.cap("01-corte-criado.png")

    # ---- 2) camadas pelo menu: vetor, preenchimento, raster
    for kind in ("vector", "fill", "raster"):
        hx.click("layers:add", wait=300)
        hx.cap(f"02-menu-camadas.png") if kind == "vector" else None
        hx.click(f"layers:add.{kind}", wait=500)
    st = state()
    kinds = [l["kind"] for l in st["editor"]["layers"]]
    check(kinds == ["vector", "raster", "fill"], f"camadas criadas pelo menu: {kinds}")
    # renomear a camada vetorial por duplo clique
    x, y = hx.center("layers:row:0", st)
    ui(f"dclick {x - 40} {y}; wait 300; key ctrl+a; type Linha; key enter; wait 400")
    st = state()
    check([l["name"] for l in st["editor"]["layers"]][0] == "Linha", f"renomeada por duplo clique: {[l['name'] for l in st['editor']['layers']]}")
    vector, fill, raster = st["editor"]["layers"][0]["id"], st["editor"]["layers"][2]["id"], st["editor"]["layers"][1]["id"]
    hx.cap("03-camadas.png")

    # ---- 3) caneta: forma fechada por arrasto real
    hx.click("tool:pen", wait=300)
    hx.click("layers:row:0", wait=300)
    n0 = len(state()["editor"]["latency_log"])
    blob = circle(960, 540, 420, 300, n=44, wobble=0.12)
    hx.poly_canvas(blob, step=7, delay=10)
    wait_latency(n0)
    st = state()
    vcel = layer_cel(vector)
    sk = strokes(vcel)
    check(len(sk) == 1, f"um arrasto = um traco na camada Linha ({len(sk)} traco(s), {len(sk[0]['points'])} amostras)")
    first = sk[0]["points"][0]
    ex = hx.canvas_to_screen(blob[0][0], blob[0][1], st)
    check(abs(first["x"] - blob[0][0]) < 6 and abs(first["y"] - blob[0][1]) < 6, f"o traco comeca onde o cursor apertou: ({first['x']:.1f}, {first['y']:.1f}) vs alvo {blob[0]} (1 px da tela = {1 / (st['editor']['canvas']['zoom']):.1f} px do desenho)")
    hx.cap("04-caneta-forma.png")

    # ---- 4) paleta e cor: nova paleta, cor na paleta, amostra = cor atual (referencia)
    hx.click("colors:new_palette", wait=400)
    data("UI_ACTION", {"action": "set_color", "color": "#e8a070"})
    hx.click("colors:add_to_palette", wait=400)
    st = state()
    check(st["editor"]["color"] == "#e8a070ff", "cor atual = cor da paleta (ligada por referencia)")
    hx.click("colors:swatch:0", wait=300)
    hx.cap("05-paleta.png")

    # ---- 5) balde dentro da forma: vai para a camada de preenchimento ligada a linha
    hx.click("tool:fill", wait=300)
    n0 = len(state()["editor"]["latency_log"])
    x, y = hx.canvas_to_screen(960, 540)
    ui(f"click {x} {y}; wait 300")
    wait_latency(n0)
    p_in, p_out = px(CUT, 0, 960, 540), px(CUT, 0, 120, 120)
    check(p_in[:3] == (232, 160, 112) and p_in[3] == 255, f"o balde pintou o interior: {p_in}")
    check(p_out[3] == 0, f"fora da forma continua transparente: {p_out}")
    hx.cap("06-balde.png")

    # ---- 6) pincel raster com 2 tracos
    data("UI_ACTION", {"action": "set_color", "color": "#3a6fd0"})
    hx.click("layers:row:1", wait=300)
    hx.click("tool:brush", wait=300)
    n0 = len(state()["editor"]["latency_log"])
    hx.poly_canvas([(200, 900), (420, 960), (700, 900), (1000, 975), (1300, 905), (1700, 960)], step=7, delay=10)
    wait_latency(n0)
    p = px(CUT, 0, 420, 960)
    check(p[3] > 150 and p[2] > p[0], f"pincel azul no raster: {p}")
    hx.cap("07-pincel.png")

    # ---- 7) visibilidade e desfazer/refazer
    hx.click("layers:eye:2", wait=500)
    check(px(CUT, 0, 960, 540)[3] == 0, "olho fechado: o preenchimento some do render")
    hx.cap("08-camada-oculta.png")
    hx.click("layers:eye:2", wait=500)
    check(px(CUT, 0, 960, 540)[3] == 255, "olho aberto: o preenchimento volta")
    rev = state()["project"]["revision"]
    ui("foco; key ctrl+z; wait 500")
    check(px(CUT, 0, 420, 960)[3] == 0 or state()["project"]["revision"] != rev, "Ctrl+Z real desfez (o traco do pincel sumiu)")
    hx.cap("09-desfeito.png")
    ui("key ctrl+shift+z; wait 600")
    check(px(CUT, 0, 420, 960)[3] > 150, "Ctrl+Shift+Z real refez o pincel")

    # ---- 7b) conta-gotas, borracha vetorial, selecao + mover (Enter) e Esc, tudo com o mouse real
    hx.click("tool:eyedropper", wait=300)
    x, y = hx.canvas_to_screen(960, 540)
    ui(f"click {x} {y}; wait 400")
    check(state()["editor"]["color"] == "#e8a070ff", f"conta-gotas copiou a cor do canvas: {state()['editor']['color']}")
    hx.click("layers:row:0", wait=300)
    hx.click("tool:pen", wait=200)
    n0 = len(state()["editor"]["latency_log"])
    hx.poly_canvas([(1500, 700), (1560, 650), (1620, 700), (1680, 650), (1740, 700)], step=5, delay=10)
    wait_latency(n0)
    nvec = len(strokes(vcel))
    hx.cap("08b-traco-para-apagar.png")
    hx.click("tool:eraser", wait=300)
    n0 = len(state()["editor"]["latency_log"])
    hx.poly_canvas([(1620, 600), (1620, 760)], step=4, delay=10)
    wait_latency(n0)
    check(len(strokes(vcel)) == nvec - 1, f"borracha vetorial apagou o traco que o cursor atravessou ({nvec} -> {len(strokes(vcel))})")
    hx.cap("08c-borracha-vetorial.png")
    ui("key ctrl+z; wait 500")
    check(len(strokes(vcel)) == nvec, "Ctrl+Z devolve o traco apagado (um passo)")
    # selecionar um retangulo em volta do traco e mover com o mouse; Enter aplica; Esc cancela
    hx.click("tool:lasso", wait=300)
    hx.poly_canvas([(1440, 600), (1800, 760)], step=8, delay=10)
    check(state()["editor"]["selection"] is not None, "selecao retangular criada por arrasto")
    hx.cap("08d-selecao.png")
    hx.click("tool:move", wait=300)
    x1, y1 = hx.canvas_to_screen(1590, 680)
    x2, y2 = hx.canvas_to_screen(1590 - 300, 680 - 100)
    hx.poly([(x1, y1), (x2, y2)], step=6, delay=10)
    sel = state()["editor"]["selection"]
    check(abs(sel["offset"][0] + 300) < 12 and abs(sel["offset"][1] + 100) < 12, f"movimento pendente do mouse: {sel['offset']}")
    hx.cap("08e-mover-pendente.png")
    xs0 = strokes(vcel)[-1]["points"][0]["x"]
    ui("key enter; wait 800")
    xs1 = strokes(vcel)[-1]["points"][0]["x"]
    check(abs((xs1 - xs0) + 300) < 12, f"Enter aplicou o movimento ao traco: x {xs0:.0f} -> {xs1:.0f}")
    ui("key ctrl+z; wait 500")
    check(abs(strokes(vcel)[-1]["points"][0]["x"] - xs0) < 1, "Ctrl+Z desfaz o mover")
    ui("key esc; wait 300")
    check(state()["editor"]["selection"] is None, "Esc limpa a selecao")
    hx.cap("08f-apos-mover.png")

    # ---- 8) zoom (roda), pan (botao do meio e espaco), rotacao (R + arrastar), ajustar
    st = state()
    r = st["widgets"]["canvas:area"]["rect_screen"]
    cx, cy = round(r[0] + r[2] / 2), round(r[1] + r[3] / 2)
    z0 = st["editor"]["canvas"]["zoom"]
    ax, ay = hx.canvas_to_screen(960, 240, st)  # ponto do topo da forma
    ui(f"move {ax} {ay}; wheel 4; wait 500")
    st = state()
    z1 = st["editor"]["canvas"]["zoom"]
    bx, by = hx.canvas_to_screen(960, 240, st)
    check(z1 > z0 * 1.5 and abs(bx - ax) <= 2 and abs(by - ay) <= 2, f"zoom da roda ancorado no cursor: {z0:.2f} -> {z1:.2f}; o ponto sob o cursor andou ({bx - ax}, {by - ay}) px")
    hx.cap("10-zoom-roda.png")
    hx.poly([(cx, cy), (cx + 140, cy + 60)], step=10, button="middle")
    st = state()
    check(abs(hx.canvas_to_screen(960, 240, st)[0] - (bx + 140)) <= 3, "pan com o botao do meio")
    hx.poly([(cx, cy), (cx - 100, cy - 40)], step=10, keys="space")
    check(abs(hx.canvas_to_screen(960, 240, state())[0] - (bx + 40)) <= 4, "pan com espaco + arrastar")
    ui(f"move {cx} {cy}; wheel 10; wait 200; wheel 8; wait 500")
    z2 = state()["editor"]["canvas"]["zoom"]
    check(z2 > 6.0, f"zoom alto para ver a grade de pixels: {z2 * 100:.0f}%")
    hx.cap("11-grade-de-pixels.png")
    hx.click("canvas:fit", wait=400)
    hx.poly([(cx - 200, cy), (cx - 200, cy - 40), (cx - 200, cy - 80)], step=10, keys="r")
    rot = state()["editor"]["canvas"]["rotation_deg"]
    check(abs(rot) > 5, f"R + arrastar gira a visao: {rot:.1f} graus")
    hx.click("canvas:fit", wait=400)
    hx.cap("12-rotacao.png")
    # desenhar com a visao girada: o traco continua onde o cursor passa
    hx.click("tool:pen", wait=200)
    hx.click("layers:row:0", wait=200)
    n0 = len(state()["editor"]["latency_log"])
    hx.poly_canvas([(1500, 200), (1560, 260), (1620, 200)], step=4, delay=12)
    wait_latency(n0)
    last = strokes(vcel)[-1]["points"][0]
    check(abs(last["x"] - 1500) < 8 and abs(last["y"] - 200) < 8, f"com a visao girada o traco ainda cai no cursor: ({last['x']:.1f}, {last['y']:.1f}) vs (1500, 200)")
    hx.click("canvas:fit", wait=300)
    st = state()
    # zera a rotacao digitando no campo de graus
    x, y = hx.center("canvas:rotation", st)
    ui(f"dclick {x} {y}; key ctrl+a; type 0; key enter; wait 300")
    hx.click("canvas:flip", wait=400)
    hx.cap("13-espelhado.png")
    check(state()["editor"]["canvas"]["flip"], "espelhar a visao")
    n0 = len(state()["editor"]["latency_log"])
    hx.poly_canvas([(1500, 320), (1560, 380), (1620, 320)], step=4, delay=12)
    wait_latency(n0)
    last = strokes(vcel)[-1]["points"][0]
    check(abs(last["x"] - 1500) < 8 and abs(last["y"] - 320) < 8, f"com a visao espelhada o traco cai no cursor: ({last['x']:.1f}, {last['y']:.1f}) vs (1500, 320)")
    hx.click("canvas:flip", wait=400)

    # ---- 9) cels e linha do tempo: 2o cel, exposicao por clique, scrub, desenhar no cel 2, reproduzir
    st = state()
    hx.click("layers:row:0", wait=300)
    hx.click("cels:add", wait=400)
    check(state()["editor"]["cel_number"] == 2, "2o cel criado e escolhido na lista")
    hx.cap("14-cel2.png")
    st = state()
    area = st["widgets"]["timeline:area"]["rect_logical"]
    cw, hdr = st["editor"]["timeline"]["cell_w"], st["editor"]["timeline"]["header_w"]

    def tl_xy(frame, row):
        rr = st["widgets"][f"timeline:row:{row}"]["rect_logical"]
        return hx.to_screen(st, area[0] + hdr + (frame + 0.5) * cw, rr[1] + rr[3] / 2)

    for f in (6, 12):
        x, y = tl_xy(f, 0)
        ui(f"click {x} {y}; wait 400")
    xs = data("QUERY", {"query": {"kind": "xsheet", "cut": CUT}})
    tr = json.dumps(xs)
    check("number" in tr and "2" in tr, f"timesheet com exposicoes do cel 2 nos quadros 6 e 12: {tr[:160]}")
    cel2 = layer_cel(vector, 1)
    check(cel2 in json.dumps(data("QUERY", {"query": {"kind": "exposure", "cut": CUT, "frame": 7}})), "o quadro 7 expoe o cel 2")
    hx.cap("15-timeline-exposicao.png")
    # scrub arrastando na regua
    ruler_y = area[1] + 9
    x0, y0 = hx.to_screen(st, area[0] + hdr + 1 * cw, ruler_y)
    x1, _ = hx.to_screen(st, area[0] + hdr + 14.5 * cw, ruler_y)
    ui(f"foco; click {x0} {y0}; wait 300")
    check(state()["editor"]["frame"] == 1, "clique na regua leva o cabecote ao quadro 2")
    ui(f"drag {x0} {y0} {x1} {y0} 20; wait 500")
    f_end = state()["editor"]["frame"]
    check(f_end >= 12, f"arrastar na regua faz scrub: quadro {f_end}")
    hx.cap("16-scrub.png")
    # desenhar no quadro 8: vai para o cel 2
    xq, yq = hx.to_screen(st, area[0] + hdr + 8.5 * cw, ruler_y)
    ui(f"click {xq} {yq}; wait 400")
    data("UI_ACTION", {"action": "set_color", "color": "#3a6fd0"})
    hx.click("tool:pen", wait=200)
    n0 = len(state()["editor"]["latency_log"])
    hx.poly_canvas([(600, 700), (780, 640), (960, 700), (1140, 640), (1320, 700)], step=7, delay=10)
    wait_latency(n0)
    check(len(strokes(cel2)) == 1 and len(strokes(vcel)) == 4, f"o traco do quadro 9 entrou no cel 2 (cel1: {len(strokes(vcel))}, cel2: {len(strokes(cel2))})")
    dark8 = [py for py in range(660, 720) if px(CUT, 8, 960, py)[:3] == (58, 111, 208)]
    dark2 = [py for py in range(660, 720) if px(CUT, 2, 960, py)[:3] == (58, 111, 208)]
    check(len(dark8) >= 2 and not dark2, f"o render do quadro 9 tem o traco novo (azul em y={dark8[:3]}...); o do quadro 3 mostra so o preenchimento")
    hx.cap("17-desenho-no-cel2.png")
    # tocar e pausar com cliques reais
    hx.click("timeline:step_back", wait=200)
    ui("key home; wait 200")
    hx.click("timeline:play", wait=1500)
    pb = state()["editor"]["playback"]
    check(pb["playing"], f"reproduzindo: {pb['fps_real']} fps reais")
    hx.cap("18-reproduzindo.png")
    hx.click("timeline:play", wait=400)
    check(not state()["editor"]["playback"]["playing"], "pausou")

    # ---- final
    st = state()
    lat = st["editor"]["latency_log"]
    hx.LOG.append("Latencias (solta o mouse -> canvas atualizado), todas as ferramentas desta corrida: " + json.dumps(lat))
    hx.cap("19-final.png")
    (hx.EVID / "mouse-real.log.md").write_text(
        "# S03/B2 - roteiro de mouse/teclado reais (SendInput)\n\nGerado por `mouse_real_s03.py`; vigia: `mouse-real-watch.json`.\n\n" + "\n".join(hx.LOG) + "\n", encoding="utf-8"
    )
    print("OK: roteiro completo")


if __name__ == "__main__":
    main()
