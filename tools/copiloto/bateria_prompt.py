"""Bateria: o que faz o prompt do Copiloto pesar mais sobre um desenho já limpo (o rosto do usuário). Usa o código do
serviço (servico.load/generate). Uma grade por prompt: linhas = ajustes, colunas = imagem gerada | linhas finais.

Uso: python tools/copiloto/bateria_prompt.py <png do desenho, preto sobre branco> <pasta de saída>
"""
import json, os, sys, time
sys.path.insert(0, os.path.dirname(__file__))
import servico  # noqa: E402
from PIL import Image, ImageDraw  # noqa: E402

AJUSTES = [  # (nome, força do controle, fim do controle, cfg, passos)
    ("atual f0.8 fim1.0 cfg1 4p", 0.8, 1.0, 1.0, 4),
    ("f0.5 fim1.0 cfg1 4p", 0.5, 1.0, 1.0, 4),
    ("f0.8 fim0.5 cfg1 4p", 0.8, 0.5, 1.0, 4),
    ("f0.8 fim1.0 cfg2 4p", 0.8, 1.0, 2.0, 4),
    ("f0.6 fim0.6 cfg2 4p", 0.6, 0.6, 2.0, 4),
    ("f0.5 fim0.5 cfg2 6p", 0.5, 0.5, 2.0, 6),
    ("f0.4 fim0.4 cfg2.5 8p", 0.4, 0.4, 2.5, 8),
]
PROMPTS = ["boy, black hair", "girl, long hair, smiling", "boy with glasses and spiky hair"]


def main():
    src, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    pipe, a2s = servico.load()
    servico.generate(pipe, a2s, {"image": src, "out": os.path.join(out, "_aq.png"), "res": 512, "steps": 1})
    rows = []
    T = 256
    for pi, pr in enumerate(PROMPTS):
        W = Image.new("RGB", (T * 3 + 260, T * len(AJUSTES)), "white")
        d = ImageDraw.Draw(W)
        for ai, (nome, f, fim, cfg, passos) in enumerate(AJUSTES):
            raw, lin = os.path.join(out, f"p{pi}_a{ai}_raw.png"), os.path.join(out, f"p{pi}_a{ai}_lin.png")
            t = time.perf_counter()
            info = servico.generate(pipe, a2s, {"image": src, "out": lin, "raw_out": raw, "prompt": pr, "res": 512, "steps": passos,
                                                "strength": f, "control_end": fim, "cfg": cfg, "seed": 1})
            ms = round((time.perf_counter() - t) * 1000)
            rows.append({"prompt": pr, "ajuste": nome, "ms": ms, **info})
            print(json.dumps(rows[-1], ensure_ascii=False), flush=True)
            y = ai * T
            W.paste(Image.open(src).convert("RGB").resize((T, T)), (0, y))
            W.paste(Image.open(raw).convert("RGB").resize((T, T)), (T, y))
            W.paste(Image.open(lin).convert("RGB").resize((T, T)), (2 * T, y))
            d.text((3 * T + 8, y + 8), f"{nome}\n{ms} ms", fill="black")
        d.text((3 * T + 8, T * len(AJUSTES) - 20), f"prompt: {pr}", fill="red")
        W.save(os.path.join(out, f"grade_p{pi}.png"))
    json.dump(rows, open(os.path.join(out, "tempos.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)


if __name__ == "__main__":
    main()
