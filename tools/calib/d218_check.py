"""Verifica quadros do corte D contra a régua (decisão 24) e resume. uso: python d218_check.py TAG quadro [quadro ...]"""
import json, os, subprocess, sys

HERE = os.path.dirname(__file__)
tag, frames = sys.argv[1], [int(a) for a in sys.argv[2:]]
env = dict(os.environ, PYTHONIOENCODING="utf-8")
bad = []
for f in frames:
    out = subprocess.run([sys.executable, os.path.join(HERE, "d218_compare.py"), str(f), tag], env=env, capture_output=True, text=True,
                         encoding="utf-8").stdout
    j = json.loads(out[:out.index("arquivos")])
    rep = json.load(open(rf"C:\AnimeIAI\sprints\registro\calibracao\D218\tentativas\{tag}_q{f:02d}\q{f:04d}_relatorio.json", encoding="utf-8"))
    th = rep["thresholds"]
    line = (f"q{f:02d} {'ok ' if j['pass'] else 'FALHA'} flip {rep['flip']['mean']:.4f}/{th['flip_mean_max']:.4f} "
            f"ssim {rep['ssim']:.4f}/{th['ssim_min']:.4f} F1 {rep['edges']['f1']:.4f}/{th['edge_f1_min']:.4f}")
    print(line, flush=True)
    if not j["pass"]:
        bad.append(f)
print("falhas:", bad)
