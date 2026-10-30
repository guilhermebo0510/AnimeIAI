"""Relatório de tempo das gerações do Copiloto: Intensidade (0–100) × resolução, com o mesmo mapa do app
(`aiai_tools::copilot::change_params`). Usa o código do serviço (servico.load/generate); uma linha JSON por medida.

Uso: python tools/copiloto/tempos.py <png do desenho> <pasta de saída> [--repetir 2]
"""
import json, os, sys, time
sys.path.insert(0, os.path.dirname(__file__))
import servico  # noqa: E402

# o mesmo mapa de aiai-tools/src/copilot.rs (CHANGE_ANCHORS): (mudança, força, fim do controle, cfg, passos)
ANCHORS = [(0.0, 1.0, 1.0, 1.0, 4), (0.1, 0.85, 0.9, 1.0, 4), (0.5, 0.6, 0.6, 2.0, 4), (0.9, 0.45, 0.45, 2.3, 6), (1.0, 0.4, 0.4, 2.5, 8)]


def params(c):
    for a, b in zip(ANCHORS, ANCHORS[1:]):
        if c <= b[0]:
            t = (c - a[0]) / (b[0] - a[0])
            l = lambda x, y: x + (y - x) * t
            return l(a[1], b[1]), l(a[2], b[2]), l(a[3], b[3]), round(l(a[4], b[4]))
    return ANCHORS[-1][1:]


def main():
    src, out = sys.argv[1], sys.argv[2]
    rep = int(sys.argv[sys.argv.index("--repetir") + 1]) if "--repetir" in sys.argv else 2
    os.makedirs(out, exist_ok=True)
    t0 = time.perf_counter()
    pipe, a2s = servico.load()
    carga = time.perf_counter() - t0
    servico.generate(pipe, a2s, {"image": src, "out": os.path.join(out, "_aq.png"), "res": 384, "steps": 1})
    print(json.dumps({"carga_s": round(carga, 1)}), flush=True)
    rows = []
    for res in (384, 512, 768):
        for ch in (0.1, 0.5, 0.9):
            f, fim, cfg, passos = params(ch)
            ms = []
            for r in range(rep):
                t = time.perf_counter()
                servico.generate(pipe, a2s, {"image": src, "out": os.path.join(out, f"t_{res}_{int(ch*100)}.png"), "prompt": "boy, black hair",
                                            "width": res, "height": res, "steps": passos, "strength": f, "control_end": fim, "cfg": cfg, "seed": 1})
                ms.append(round((time.perf_counter() - t) * 1000))
            row = {"res": res, "intensidade": int(ch * 100), "passos": passos, "cfg": cfg, "ms": ms, "ms_min": min(ms)}
            rows.append(row)
            print(json.dumps(row), flush=True)
    json.dump({"carga_s": round(carga, 1), "medidas": rows}, open(os.path.join(out, "tempos.json"), "w"), indent=1)


if __name__ == "__main__":
    main()
