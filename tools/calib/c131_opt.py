"""Corte C: para cada linha da moldura, escolhe a VARIANTE DE MEDIÇÃO da própria estimativa do agente (meia altura × faixa de cor,
com/sem âncora nas bordas, busca longa × curta) que melhora a régua (F1 + SSIM do COMPARE no quadro 15). A geometria vem sempre
da medição perto da estimativa digitada (decisão 22); aqui só se escolhe COMO medir. Guarda em c131_variantes.json.
uso: python c131_opt.py [passadas] [prefixo]"""
import json, os, subprocess, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from c131_rosas import LINES

VAR_F = os.path.join(HERE, "c131_variantes.json")
ENV = dict(os.environ, PYTHONIOENCODING="utf-8")
OPTS = [["meia", True, False], ["faixa", True, False], ["faixa", False, False], ["meia", False, False], ["meia", True, True],
        ["faixa", False, True]]


def run(*a):
    r = subprocess.run([sys.executable, os.path.join(HERE, a[0]), *a[1:]], env=ENV, capture_output=True, text=True, encoding="utf-8")
    if r.returncode:
        raise SystemExit(r.stderr[-2000:])
    return r.stdout


def score():
    run("c131_lines.py")
    run("c131_fill.py")
    run("c131_compare.py", "15", "opt")
    rep = json.load(open(r"C:\AnimeIAI\sprints\registro\calibracao\C131\tentativas\opt_q15\q0015_relatorio.json", encoding="utf-8"))
    return rep["edges"]["f1"] + rep["ssim"], rep["edges"]["f1"], rep["ssim"], rep["flip"]["mean"]


passes = int(sys.argv[1]) if len(sys.argv) > 1 else 1
pref = sys.argv[2] if len(sys.argv) > 2 else ""
var = json.load(open(VAR_F)) if os.path.exists(VAR_F) else {}
json.dump(var, open(VAR_F, "w"), indent=0)
best = score()
print("início", [round(v, 4) for v in best], flush=True)
for p in range(passes):
    for name, (poly, w, closed, *opt) in LINES.items():
        if not name.startswith(pref) or (len(opt) > 2 and opt[2] == "raw"):
            continue
        cur = var.get(name)
        for o in OPTS:
            if o == cur or (cur is None and o == OPTS[0]):
                continue
            var[name] = o
            json.dump(var, open(VAR_F, "w"), indent=0)
            sc = score()
            if sc[0] > best[0] + 1e-4 and sc[3] < 0.2395:
                best, cur = sc, o
                print(name, o, [round(v, 4) for v in sc], flush=True)
        if cur is None:
            var.pop(name, None)
        else:
            var[name] = cur
        json.dump(var, open(VAR_F, "w"), indent=0)
print("fim", [round(v, 4) for v in best])
