#!/usr/bin/env python3
"""Medida de latencia do editor: tempo entre SOLTAR o mouse (evento de soltar chega ao egui) e a textura nova do canvas pronta,
num corte 1920x1080 com 5 camadas com conteudo (2 vetoriais, 2 raster, 1 de preenchimento). Mouse REAL (SendInput).

Cada ferramenta e repetida varias vezes; o app mede por dentro (ferramenta = montar o comando e aplicar; render = thread de fundo)
e publica em GET_UI_STATE.editor.latency_log. Saida: latencia-s03.json e latencia-s03.md.
"""
import json
import statistics
import time

import hx
from hx import check, data, state, ui

REPS = 6


def wait_latency(n_before, timeout=30):
    t0 = time.time()
    while len(state()["editor"]["latency_log"]) <= n_before:
        if time.time() - t0 > timeout:
            raise SystemExit("o canvas nao atualizou depois da edicao")
        time.sleep(0.03)


def count():
    return len(state()["editor"]["latency_log"])


def main():
    ui("foco; max; wait 700")
    x, y = hx.center("menu:file")
    ui(f"click {x} {y}; wait 300")
    hx.click("menu_item:file.new", wait=500)
    hx.click("project:quick_start", wait=900)
    st = state()
    cut = st["editor"]["cut"]
    check(st["editor"]["canvas"]["size"] == [1920, 1080], "corte 1920x1080")
    # 5 camadas: Linha (vetor), Preenchimento, Raster A, Raster B, Detalhe (vetor)
    for kind in ("vector", "fill", "raster", "raster", "vector"):
        hx.click("layers:add", wait=250)
        hx.click(f"layers:add.{kind}", wait=450)
    st = state()
    kinds = [l["kind"] for l in st["editor"]["layers"]]
    check(len(kinds) == 5, f"5 camadas: {kinds}")
    rows = {k: [i for i, l in enumerate(st["editor"]["layers"]) if l["kind"] == k] for k in set(kinds)}
    print(rows, [l["name"] for l in st["editor"]["layers"]])
    vec_rows, ras_rows, fill_row = rows["vector"], rows["raster"], rows["fill"][0]
    # conteudo: forma fechada na 1a camada vetorial, cor, preenchimento, tracos raster nas duas, traco no detalhe
    shape = [(700, 300), (1200, 280), (1450, 520), (1250, 800), (800, 780), (620, 540), (700, 300)]
    hx.click("tool:pen", wait=200)
    hx.click(f"layers:row:{vec_rows[-1]}", wait=250)
    n = count()
    hx.poly_canvas(shape, step=7, delay=8)
    wait_latency(n)
    data("UI_ACTION", {"action": "set_color", "color": "#e8a070"})
    hx.click("tool:fill", wait=200)
    n = count()
    x, y = hx.canvas_to_screen(960, 540)
    ui(f"click {x} {y}; wait 200")
    wait_latency(n)
    data("UI_ACTION", {"action": "set_color", "color": "#3a6fd0"})
    hx.click("tool:brush", wait=200)
    for r in ras_rows:
        hx.click(f"layers:row:{r}", wait=250)
        n = count()
        hx.poly_canvas([(200, 900 + 40 * ras_rows.index(r)), (600, 960), (1000, 900), (1500, 980), (1800, 920)], step=8, delay=8)
        wait_latency(n)
    data("UI_ACTION", {"action": "set_color", "color": "#101010"})
    hx.click("tool:pen", wait=200)
    hx.click(f"layers:row:{vec_rows[0]}", wait=250)
    n = count()
    hx.poly_canvas([(300, 200), (500, 160), (700, 220), (900, 160)], step=6, delay=8)
    wait_latency(n)
    hx.cap("20-latencia-cena-5-camadas.png")

    # medidas
    start = count()
    # caneta: tracos curtos na camada de detalhe
    hx.click("tool:pen", wait=200)
    hx.click(f"layers:row:{vec_rows[0]}", wait=250)
    for i in range(REPS):
        n = count()
        y0 = 120 + 30 * i
        hx.poly_canvas([(1300, y0), (1400, y0 + 20), (1500, y0), (1600, y0 + 20)], step=8, delay=8)
        wait_latency(n)
    # pincel: tracos na camada raster A
    hx.click("tool:brush", wait=200)
    hx.click(f"layers:row:{ras_rows[0]}", wait=250)
    for i in range(REPS):
        n = count()
        y0 = 850 + 15 * i
        hx.poly_canvas([(1100, y0), (1300, y0 + 30), (1500, y0)], step=8, delay=8)
        wait_latency(n)
    # balde: alterna a cor e clica dentro da forma
    hx.click("tool:fill", wait=200)
    hx.click(f"layers:row:{vec_rows[-1]}", wait=250)
    for i in range(REPS):
        data("UI_ACTION", {"action": "set_color", "color": ["#e8a070", "#70c0e8", "#a0e870"][i % 3]})
        n = count()
        x, y = hx.canvas_to_screen(960, 540)
        ui(f"click {x} {y}; wait 150")
        wait_latency(n)
    # borracha vetorial: atravessa o traco do detalhe
    hx.click("tool:eraser", wait=200)
    hx.click(f"layers:row:{vec_rows[0]}", wait=250)
    for i in range(3):
        n = count()
        hx.poly_canvas([(1450, 100 + 30 * i), (1450, 160 + 30 * i)], step=6, delay=8)
        # nem toda passada acerta um traco: espera um pouco e segue
        t0 = time.time()
        while count() <= n and time.time() - t0 < 2.5:
            time.sleep(0.05)
    st = state()
    log = st["editor"]["latency_log"][start:]
    by = {}
    for e in log:
        by.setdefault(e["label"], []).append(e)
    lines = ["# Latencia do editor (S03/B2): soltar o mouse -> canvas atualizado", "", f"Corte 1920x1080, 5 camadas com conteudo, mouse real (SendInput), {REPS} repeticoes por ferramenta. Tempos em ms.", "",
             "| Ferramenta | n | ferramenta (mediana) | render CPU (mediana) | total soltar->canvas (mediana) | total (max) |", "|---|---|---|---|---|---|"]
    summary = {}
    for label, es in by.items():
        tool = statistics.median(e["tool_ms"] for e in es)
        rend = statistics.median(e["render_ms"] for e in es)
        tot = statistics.median(e["total_ms"] for e in es)
        mx = max(e["total_ms"] for e in es)
        summary[label] = {"n": len(es), "tool_ms": tool, "render_ms": rend, "total_ms": tot, "max_total_ms": mx}
        lines.append(f"| {label} | {len(es)} | {tool:.0f} | {rend:.0f} | {tot:.0f} | {mx:.0f} |")
    (hx.EVID / "latencia-s03.json").write_text(json.dumps({"summary": summary, "log": log, "canvas": st["editor"]["canvas"]["size"], "layers": len(st["editor"]["layers"])}, indent=2), encoding="utf-8")
    (hx.EVID / "latencia-s03.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))
    hx.cap("21-latencia-final.png")


if __name__ == "__main__":
    main()
