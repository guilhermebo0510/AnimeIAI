"""Bancada de modelos do Copiloto de traço (protótipo, passo 2): mede tempo e qualidade de modelos JÁ TREINADOS na
placa do usuário, para cada função (Melhorar o meu traço, Completar), em várias resoluções e números de passos.

Nada aqui entra no app: gera imagens de lineart (preto sobre branco) e uma tabela de tempos. O vetorizador do app
lê essas imagens depois (aiai_vectorize).

Modelos (baixados só para teste; licenças no relatório):
  - base: Stable Diffusion 1.5 (stable-diffusion-v1-5/stable-diffusion-v1-5, fp16)
  - controle pelo rascunho: ControlNet lineart anime (lllyasviel/control_v11p_sd15s2_lineart_anime, fp16)
                            T2I-Adapter sketch (TencentARC/t2iadapter_sketch_sd15v2)
  - aceleração: LCM-LoRA (latent-consistency/lcm-lora-sdv1-5)

Uso:
  python tools/copiloto/bancada.py --casos <pasta com caso_*.png> --saida <pasta> [--modelos cn,t2i] [--res 384,512] [--passos 4,6]
Cada caso: `caso_<nome>.png` (o rascunho/desenho parcial, preto sobre branco) e, opcional, `caso_<nome>.txt` (o prompt).
"""
import argparse, json, os, time

HF = r"C:\AnimeIAI\calibracao\copiloto-modelos\hf"
os.environ.setdefault("HF_HOME", HF)

NEG = "color, colored, colorful, blue, hair color, skin color, shading, screentone, gradient, gray fill, background, scenery, text, watermark, blurry, sketchy double lines"
POS_BASE = "monochrome, greyscale, lineart, clean anime line art, black ink lines on pure white background, no color, crisp outlines, professional genga cleanup"


def load_pipe(which):
    """Uma pipeline por vez (4 GB). O LCM-LoRA entra fundido na UNet; a do T2I-Adapter reusa essa UNet (a pipeline
    do adaptador não carrega LoRA nesta versão do diffusers)."""
    import torch
    from diffusers import StableDiffusionPipeline, StableDiffusionControlNetPipeline, ControlNetModel, StableDiffusionAdapterPipeline, T2IAdapter, LCMScheduler
    dt = torch.float16
    base = "stable-diffusion-v1-5/stable-diffusion-v1-5"
    sd = StableDiffusionPipeline.from_pretrained(base, torch_dtype=dt, variant="fp16", safety_checker=None, requires_safety_checker=False)
    sd.load_lora_weights("latent-consistency/lcm-lora-sdv1-5")
    sd.fuse_lora()
    sd.unload_lora_weights()
    comp = dict(vae=sd.vae, text_encoder=sd.text_encoder, tokenizer=sd.tokenizer, unet=sd.unet, scheduler=LCMScheduler.from_config(sd.scheduler.config),
                safety_checker=None, feature_extractor=None, requires_safety_checker=False)
    if which == "cn":
        cn = ControlNetModel.from_pretrained("lllyasviel/control_v11p_sd15s2_lineart_anime", torch_dtype=dt, variant="fp16")
        p = StableDiffusionControlNetPipeline(controlnet=cn, **comp)
    else:
        ad = T2IAdapter.from_pretrained("TencentARC/t2iadapter_sketch_sd15v2", torch_dtype=dt)
        p = StableDiffusionAdapterPipeline(adapter=ad, **comp)
    # GTX 1650: a VAE em fp16 dá NaN (imagem preta); em fp32 decodifica certo em ~1 s. UNet e controle ficam em fp16.
    from diffusers import AutoencoderKL
    p.vae = AutoencoderKL.from_pretrained(base, subfolder="vae", torch_dtype=torch.float32, variant="fp16")
    p.to("cuda")
    p.vae.to(torch.float32)
    p.set_progress_bar_config(disable=True)
    return p


def control_image(path, res, kind):
    """O rascunho na resolução de geração. ControlNet lineart espera linha BRANCA sobre preto; o T2I sketch também."""
    from PIL import Image, ImageOps
    im = Image.open(path).convert("L").resize((res, res), Image.LANCZOS)
    inv = ImageOps.invert(im)
    return inv.convert("RGB") if kind == "cn" else inv  # o adaptador de sketch quer 1 canal


def run(args):
    import torch
    from PIL import Image
    casos = sorted(f for f in os.listdir(args.casos) if f.startswith("caso_") and f.endswith(".png"))
    os.makedirs(args.saida, exist_ok=True)
    rows = []
    for nome_m in args.modelos.split(","):
        pipe = load_pipe(nome_m)
        for res in [int(x) for x in args.res.split(",")]:
            for passos in [int(x) for x in args.passos.split(",")]:
                for c in casos:
                    base = c[:-4]
                    txt = os.path.join(args.casos, base + ".txt")
                    prompt = POS_BASE + (", " + open(txt, encoding="utf-8").read().strip() if os.path.exists(txt) else "")
                    ctrl = control_image(os.path.join(args.casos, c), res, nome_m)
                    g = torch.Generator("cpu").manual_seed(args.seed)
                    kw = dict(prompt=prompt, negative_prompt=NEG, num_inference_steps=passos, guidance_scale=1.5, width=res, height=res, generator=g)
                    if nome_m == "cn":
                        kw.update(image=ctrl, controlnet_conditioning_scale=args.forca)
                    else:
                        kw.update(image=ctrl, adapter_conditioning_scale=args.forca)
                    torch.cuda.synchronize()
                    t0 = time.perf_counter()
                    img = pipe(**kw).images[0]
                    torch.cuda.synchronize()
                    ms = (time.perf_counter() - t0) * 1000
                    out = os.path.join(args.saida, f"{base}__{nome_m}_{res}_{passos}.png")
                    img.save(out)
                    rows.append({"caso": base, "modelo": nome_m, "res": res, "passos": passos, "ms": round(ms), "arquivo": out,
                                 "vram_pico_mb": round(torch.cuda.max_memory_allocated() / 2**20)})
                    print(json.dumps(rows[-1], ensure_ascii=False), flush=True)
                    torch.cuda.reset_peak_memory_stats()
        del pipe
        torch.cuda.empty_cache()
    json.dump(rows, open(os.path.join(args.saida, "tempos.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--casos", required=True)
    ap.add_argument("--saida", required=True)
    ap.add_argument("--modelos", default="cn,t2i")
    ap.add_argument("--res", default="384,512")
    ap.add_argument("--passos", default="4,6")
    ap.add_argument("--forca", type=float, default=0.8)
    ap.add_argument("--seed", type=int, default=1)
    run(ap.parse_args())
