"""Desenha a folha de teste com os nossos pincéis pelo calibrador offline (exemplo `calib_pens` do aiai-tools), que
simula o caminho da mão no app (amostras a cada 8 ms, entregues por quadro de ~16,7 ms, estabilizador do pincel, mesmos
pontos e spline do commit). Saída: `<caneta>_s<t>.png` em `--out`, para o `calib_compare.py`.

Uso:
    python tools/csp/calib_render.py --out calibracao/csp/ours/antes --baseline "Redondo duro"
    python tools/csp/calib_render.py --out calibracao/csp/ours/depois --factory           (presets de fábrica S25)
    python tools/csp/calib_render.py --out DIR --presets candidatos.json                   (presets à mão, por caneta)
"""
import argparse
import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(__file__))
from csp_sheet import sheet  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
EXE = os.path.join(ROOT, "app", "target", "release", "examples", "calib_pens.exe")
PENS = ["g-pen", "real-g-pen", "mapping-pen", "turnip-pen", "calligraphy", "textured-pen", "caneta-rugosa", "milli-pen",
        "felt-pen", "dot-pen", "flat-marker"]
# caneta da referência -> preset de fábrica do AnimeIAI (S25)
FACTORY = {
    "g-pen": "Pena G",
    "real-g-pen": "Pena G real",
    "mapping-pen": "Pena de mapeamento",
    "turnip-pen": "Pena nabo",
    "calligraphy": "Caligrafia",
    "textured-pen": "Caneta texturizada",
    "caneta-rugosa": "Caneta rugosa",
    "milli-pen": "Caneta milimétrica",
    "felt-pen": "Caneta de feltro",
    "dot-pen": "Caneta de pixel",
    "flat-marker": "Marcador chato",
}
# os outros grupos do S25: chave da referência -> preset de fábrica, pasta e tamanhos da folha
GROUPS = {
    "caneta": ("caneta", FACTORY, "3,10,30"),
    "lapis": ("lapis", {"pencil": "Lápis", "mechanical-pencil": "Lapiseira", "charcoal": "Carvão", "crayon": "Giz de cera", "pastel": "Pastel",
                        "chalk": "Giz"}, "3,10,30"),
    "aerografo": ("aerografo", {"soft": "Aerógrafo suave", "spray": "Spray", "droplet": "Gotículas", "noise": "Ruído", "running-color-spray": "Spray colorido"},
                  "10,30,60"),
    "borracha": ("borracha", {"hard": "Borracha dura", "soft-eraser": "Borracha macia", "kneaded": "Borracha limpa-tipos", "rough-eraser": "Borracha áspera",
                              "vector-eraser": "Borracha vetorial", "multiple-layers": "Borracha de várias camadas", "snap-eraser": "Borracha de encaixe",
                              "borracha-dinamica": "Borracha dinâmica"}, "10,30,60"),
}
CADENCE = 0.008  # o injetor manda um ponto a cada 8 ms


def strokes_for(size, quant=None):
    """Traços com tempo. `quant` = deslocamento da grade inteira (o Clip Studio recebeu a caneta sintética como mouse, em
    pixels inteiros da tela: `round(x - q) + q`); `None` = posições exatas."""
    out = []
    for st in sheet(size):
        pts = []
        for i, (x, y, p) in enumerate(st["points"]):
            if quant is not None:
                x, y = round(x - quant) + quant, round(y - quant) + quant
            pts.append([x, y, p, round(i * CADENCE, 4)])
        out.append(pts)
    return out


def run(jobs, tmp, calib=None):
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump({"jobs": jobs, **({"calib": calib} if calib else {})}, f)
    r = subprocess.run([EXE, tmp], capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit(f"calib_pens falhou:\n{r.stdout}\n{r.stderr}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--group", default="caneta", choices=list(GROUPS))
    ap.add_argument("--pens", default=None)
    ap.add_argument("--sizes", default=None)
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--baseline", help="um preset de fábrica para todas as canetas (o 'antes')")
    g.add_argument("--factory", action="store_true", help="o preset de fábrica S25 de cada caneta")
    g.add_argument("--presets", help="JSON {caneta: BrushPreset}")
    ap.add_argument("--frame-ms", type=float, default=16.667)
    ap.add_argument("--quant", type=float, default=None, help="posições em grade inteira (mouse), deslocada deste valor")
    ap.add_argument("--calib", help="JSON da tabela do antisserrilhado em teste: [[w0,s0],[w1,s1],[w2,s2],[w3,s3],[semieixo,0]]")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    _, factory, sizes0 = GROUPS[a.group]
    a.pens = a.pens or ",".join(factory)
    a.sizes = a.sizes or sizes0
    cand = {}
    if a.presets:
        with open(a.presets, encoding="utf-8") as f:
            cand = json.load(f)
    jobs = []
    for pen in [p for p in a.pens.split(",") if p]:
        for s in [int(x) for x in a.sizes.split(",")]:
            job = {"strokes": strokes_for(s, a.quant), "out": os.path.join(a.out, f"{pen}_s{s}.png"), "frame_ms": a.frame_ms, "seed": 1}
            if a.group == "borracha":
                job["base"] = "black"
            if a.baseline:
                job["preset_name"] = a.baseline
                job["size"] = 1 if pen == "dot-pen" else s
            elif a.factory:
                job["preset_name"] = factory[pen]
                job["size"] = 1 if pen == "dot-pen" else s
            else:
                if pen not in cand:
                    continue
                p = dict(cand[pen])
                p["size"] = 1 if pen == "dot-pen" else s
                job["preset"] = p
            jobs.append(job)
    run(jobs, os.path.join(a.out, "_trabalhos.json"), json.loads(a.calib) if a.calib else None)
    print(f"{len(jobs)} folhas em {a.out}")


if __name__ == "__main__":
    main()
