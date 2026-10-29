"""Bancada do Copiloto de traço: roda o SwiftSketch (SIGGRAPH 2025, imagem → desenho vetorial) pré-treinado numa
imagem, sem o diffvg (só desenha a saída) e sem o recorte de fundo do RMBG (a imagem já vem em fundo branco). Mede o
tempo e salva os traços (JSON com as curvas cúbicas) e uma prévia PNG desenhada aqui.

Só para TESTE de viabilidade (pesos sem licença declarada).

Uso: python tools/copiloto/swiftsketch_run.py --imagem <png/jpg> --saida <pasta> [--tracos 32] [--sem-refino]
"""
import argparse, json, os, sys, time, types

REPO = r"C:\AnimeIAI\calibracao\copiloto-modelos\SwiftSketch\SwiftSketch"
SAVE = os.path.join(REPO, "save")


def stubs():
    import importlib.machinery as mach
    # o diffvg só desenha/salva SVG; o wandb é registro de treino: nenhum dos dois é usado na geração
    d = types.ModuleType("pydiffvg")
    for n in ("Path", "ShapeGroup", "save_svg", "RenderFunction", "svg_to_scene", "set_use_gpu", "set_device", "get_device", "device"):
        setattr(d, n, None)
    d.__spec__ = mach.ModuleSpec("pydiffvg", None)
    sys.modules["pydiffvg"] = d
    # cairosvg precisa da biblioteca nativa do cairo (não há no Windows); só converte SVG em imagem
    c = types.ModuleType("cairosvg")
    c.svg2png = None
    c.__spec__ = mach.ModuleSpec("cairosvg", None)
    sys.modules["cairosvg"] = c
    w = types.ModuleType("wandb")
    w.init = w.finish = w.log = lambda *a, **k: None
    w.__spec__ = mach.ModuleSpec("wandb", None)
    sys.modules["wandb"] = w


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--imagem", required=True)
    ap.add_argument("--saida", required=True)
    ap.add_argument("--tracos", type=int, default=32)
    ap.add_argument("--sem-refino", action="store_true")
    ap.add_argument("--seed", type=int, default=10)
    a = ap.parse_args()
    stubs()
    sys.path.insert(0, REPO)
    os.chdir(REPO)
    import numpy as np, torch
    from PIL import Image, ImageDraw
    model_path = os.path.join(SAVE, "sketch-diffusion", "model000450000.pt")
    refine_path = os.path.join(SAVE, "refinement-network", "model000430000.pt")
    sys.argv = ["generate", "--model_path", model_path, "--refine_model_path", refine_path, "--input_data", a.imagem, "--output_dir", a.saida,
                "--seed", str(a.seed)]
    from utils.fixseed import fixseed
    from utils.parser_util import generate_args
    from refine_model.utils_refine.parser_util import generate_args as refine_generate_args
    from utils.model_util import create_model_and_diffusion, load_model_wo_clip, create_model
    from utils import dist_util
    from model.cfg_sampler import ClassifierFreeSampleModel
    import utils.sketch_utils as su
    import model.image_features_models as models

    args = generate_args()
    fixseed(args.seed)
    dist_util.setup_dist(args.device)
    t_load = time.perf_counter()
    model, diffusion = create_model_and_diffusion(args)
    load_model_wo_clip(model, torch.load(args.model_path, map_location="cpu", weights_only=False))
    if args.guidance_param != 1:
        model = ClassifierFreeSampleModel(model)
    model.to(dist_util.dev()).eval()
    refine = None
    if not a.sem_refino:
        rargs = refine_generate_args()
        refine = create_model(rargs)
        load_model_wo_clip(refine, torch.load(rargs.refine_model_path, map_location="cpu", weights_only=False))
        refine.to(dist_util.dev()).eval()
    feats = models.CLIPMidlleFeutures(args.device, 3).to(args.device)
    load_s = time.perf_counter() - t_load

    # máscara do objeto: o que não é fundo branco (a imagem da bancada já vem recortada sobre branco)
    im = Image.open(a.imagem).convert("RGB")
    arr = np.asarray(im).astype(np.float32) / 255.0
    mask = (arr.min(axis=2) < 0.94).astype(np.float32)
    masked = su.create_masked_image(im, mask)
    masked = su.fix_image_scale(masked)
    torch.cuda.synchronize()
    t0 = time.perf_counter()
    with torch.no_grad():
        f = torch.stack([feats(masked).to(args.device)])
        scale = torch.ones(1, device=dist_util.dev()) * args.guidance_param if args.guidance_param != 1 else None
        sample = diffusion.p_sample_loop(model, (1, args.num_paths, model.ncpoints, model.nfeats), noise=None, clip_denoised=False, image_features=f,
                                         scale=scale, progress=False, skip_timesteps=0, init_image=None, dump_steps=None, const_noise=False)
        pts = sample
        if refine is not None:
            t = torch.zeros(1, dtype=torch.long, device=dist_util.dev())
            pts = refine(x=sample, timesteps=t, image_features=f)
        pts = su.denormalize_points(pts, args.scaling_factor, args.canvas_width)
    torch.cuda.synchronize()
    gen_s = time.perf_counter() - t0
    P = pts[0].detach().cpu().numpy().reshape(args.num_paths, -1, 2)  # [traço, 4 pontos de controle, xy] em 224×224

    os.makedirs(a.saida, exist_ok=True)
    base = os.path.splitext(os.path.basename(a.imagem))[0]
    json.dump({"canvas": [args.canvas_width, args.canvas_height], "strokes": P.tolist(), "load_s": round(load_s, 2), "gen_s": round(gen_s, 3),
               "refine": refine is not None}, open(os.path.join(a.saida, base + "_swiftsketch.json"), "w"))
    # prévia: cúbicas desenhadas a 4× (896 px), ao lado da imagem de entrada
    S = 4
    prev = Image.new("RGB", (args.canvas_width * S, args.canvas_height * S), "white")
    dr = ImageDraw.Draw(prev)
    for c in P:
        q = []
        for k in range(41):
            u = k / 40
            b = (1 - u) ** 3 * c[0] + 3 * (1 - u) ** 2 * u * c[1] + 3 * (1 - u) * u ** 2 * c[2] + u ** 3 * c[3]
            q.append((b[0] * S, b[1] * S))
        dr.line(q, fill=(20, 20, 20), width=3)
    side = Image.new("RGB", (prev.width * 2 + 10, prev.height), "white")
    side.paste(masked.resize(prev.size), (0, 0))
    side.paste(prev, (prev.width + 10, 0))
    side.save(os.path.join(a.saida, base + "_swiftsketch.png"))
    print(json.dumps({"carregar_s": round(load_s, 1), "gerar_s": round(gen_s, 2), "tracos": len(P), "refino": refine is not None}))


if __name__ == "__main__":
    main()
