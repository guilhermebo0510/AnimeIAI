"""Bancada do Copiloto de traço: Anime2Sketch (Xiang et al., MIT) transforma a imagem gerada (cinza, com preenchimento)
em linhas pretas sobre branco, antes do vetorizador. Versão "improved" (sem artefatos em imagens escuras).

Uso: python tools/copiloto/anime2sketch_run.py --entrada <png ou pasta> --saida <pasta> [--tam 512]
"""
import argparse, glob, os, sys, time

REPO = r"C:\AnimeIAI\calibracao\copiloto-modelos\Anime2Sketch"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--entrada", required=True)
    ap.add_argument("--saida", required=True)
    ap.add_argument("--tam", type=int, default=512)
    ap.add_argument("--filtro", default="*.png")
    a = ap.parse_args()
    sys.path.insert(0, REPO)
    cwd = os.getcwd()
    os.chdir(REPO)
    import torch
    from PIL import Image
    import torchvision.transforms as T
    from model import create_model
    net = create_model("improved").cuda().eval()
    os.chdir(cwd)
    arqs = [a.entrada] if os.path.isfile(a.entrada) else sorted(glob.glob(os.path.join(a.entrada, a.filtro)))
    os.makedirs(a.saida, exist_ok=True)
    tf = T.Compose([T.Resize((a.tam, a.tam), T.InterpolationMode.BICUBIC), T.ToTensor(), T.Normalize((0.5, 0.5, 0.5), (0.5, 0.5, 0.5))])
    for f in arqs:
        im = Image.open(f).convert("RGB")
        x = tf(im).unsqueeze(0).cuda()
        torch.cuda.synchronize()
        t0 = time.perf_counter()
        with torch.no_grad():
            y = net(x)
        torch.cuda.synchronize()
        ms = (time.perf_counter() - t0) * 1000
        out = ((y[0, 0].clamp(-1, 1) + 1) * 127.5).round().byte().cpu().numpy()
        Image.fromarray(out).resize(im.size, Image.BICUBIC).save(os.path.join(a.saida, os.path.basename(f)))
        print(f"{os.path.basename(f)} {ms:.0f} ms")


if __name__ == "__main__":
    main()
