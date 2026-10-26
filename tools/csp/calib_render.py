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
from csp_sheet import sheet, mix_sheet, COR_A, COR_B  # noqa: E402

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
    # pincéis que misturam (S25): folha normal em preto + folha de mistura (faixas COR_A, verticais COR_B) no 30
    "pincel": ("pincel", {"brush-pen": "Caneta pincel", "dry-ink": "Nanquim seco", "watery-ink": "Nanquim aguado",
                          "wet-blotting-ink": "Nanquim de mancha úmida", "round-watercolor": "Aquarela redonda",
                          "flat-watercolor": "Aquarela chata", "soft-bleed": "Transbordo suave", "wet-wash": "Lavagem molhada",
                          "rough-wash": "Lavagem áspera", "watercolor-splash": "Respingos de aquarela",
                          "round-mixing": "Pincel redondo de mistura", "gouache": "Guache", "dry-gouache": "Guache seco",
                          "thin-gouache": "Guache fino", "oil-paint": "Óleo", "thick-oil-paint": "Óleo grosso",
                          "pointillism": "Pontilhismo", "paint-and-apply": "Pintar e aplicar"}, "10,30,60"),
    # misturadores: a folha normal no 30 por cima da grade COR_A/COR_B feita com a Pena G 30 (como no Clip Studio)
    "mistura": ("mistura", {"blend": "Misturar", "blur": "Desfocar", "finger-tip": "Ponta do dedo",
                            "painterly-blender": "Misturador de pintura", "wet-bleed-blender": "Misturador molhado",
                            "textured-blender": "Misturador texturizado", "watery-brush": "Pincel aquoso",
                            "gouache-blender": "Misturador de guache"}, "30"),
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


def timed(points, quant=None):
    pts = []
    for i, (x, y, p) in enumerate(points):
        if quant is not None:
            x, y = round(x - quant) + quant, round(y - quant) + quant
        pts.append([x, y, p, round(i * CADENCE, 4)])
    return pts


def mix_strokes(size, quant=None, preset_name=None, preset_size=None):
    """Folha de mistura: faixas na COR_A e verticais na COR_B (com `preset_name`, a base dos misturadores)."""
    base, cruz = mix_sheet(size)
    out = []
    for group, color in ((base, COR_A), (cruz, COR_B)):
        for st in group:
            o = {"pts": timed(st["points"], quant), "color": list(color)}
            if preset_name:
                o["preset_name"] = preset_name
                o["size"] = preset_size
            out.append(o)
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
    ap.add_argument("--patch", help="com --factory: JSON {caneta: {campos}} por cima do preset de fábrica")
    ap.add_argument("--frame-ms", type=float, default=16.667)
    ap.add_argument("--quant", type=float, default=None, help="posições em grade inteira (mouse), deslocada deste valor")
    ap.add_argument("--no-mix", action="store_true", help="grupo pincel: sem a folha de mistura")
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
    patches = {}
    if a.patch:
        with open(a.patch, encoding="utf-8") as f:
            patches = json.load(f)
    jobs = []
    for pen in [p for p in a.pens.split(",") if p]:
        for s in [int(x) for x in a.sizes.split(",")]:
            job = {"strokes": strokes_for(s, a.quant), "out": os.path.join(a.out, f"{pen}_s{s}.png"), "frame_ms": a.frame_ms, "seed": 1}
            if a.group == "borracha":
                job["base"] = "black"
            if a.group == "mistura":
                job["strokes"] = mix_strokes(30, a.quant, "Pena G", 30) + job["strokes"]
            if a.baseline:
                job["preset_name"] = a.baseline
                job["size"] = 1 if pen == "dot-pen" else s
            elif a.factory:
                job["preset_name"] = factory[pen]
                job["size"] = 1 if pen == "dot-pen" else s
                if patches.get(pen):
                    job["patch"] = patches[pen]
            else:
                if pen not in cand:
                    continue
                p = dict(cand[pen])
                p["size"] = 1 if pen == "dot-pen" else s
                job["preset"] = p
            jobs.append(job)
            if a.group == "pincel" and s == 30 and not a.no_mix:
                mj = dict(job)
                mj["strokes"] = mix_strokes(30, a.quant)
                mj["out"] = os.path.join(a.out, f"{pen}_mix_s30.png")
                jobs.append(mj)
    run(jobs, os.path.join(a.out, "_trabalhos.json"), json.loads(a.calib) if a.calib else None)
    print(f"{len(jobs)} folhas em {a.out}")


if __name__ == "__main__":
    main()
