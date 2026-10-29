"""Bancada de modelos do Copiloto de traço (protótipo, passo 2): mede tempo e qualidade de modelos JÁ TREINADOS na
placa do usuário, para cada função (Melhorar o meu traço, Completar), em várias resoluções e números de passos.

Nada aqui entra no app: gera imagens de lineart (preto sobre branco) e uma tabela de tempos. O vetorizador do app
lê essas imagens depois (aiai_vectorize).

Modelos (baixados só para teste; licenças no relatório):
  - base: Stable Diffusion 1.5 (creativeml-openrail-m) ou AnyLoRA de Lykon (anime, creativeml-openrail-m)
  - controle pelo rascunho: ControlNet lineart anime (lllyasviel, openrail) | T2I-Adapter sketch (TencentARC, apache-2.0)
  - aceleração: LCM-LoRA (openrail++); estilo opcional: LoRA de sketch SD 1.5 (creativeml-openrail-m)
  - decodificador: VAE do modelo em fp32 ou TAESD (madebyollin, MIT)

GTX 1650 (4 GB), achados: a VAE em fp16 dá NaN (imagem preta); acima de ~3,4 GB a placa transborda para a RAM e
cada passo leva segundos. Por isso: decodificador em fp32 ou TAESD, atenção em fp32 opcional, tudo na placa.

Uso:
  python tools/copiloto/bancada.py --casos <pasta> --saida <pasta> [--base anylora] [--modelos cn,t2i] [--res 384,512]
         [--passos 4,6] [--cfg 1.0] [--vae taesd] [--sketch-lora 0.6] [--upcast]
Cada caso: `caso_<nome>.png` (rascunho/desenho parcial, preto sobre branco) e, opcional, `caso_<nome>.txt` (prompt).
"""
import argparse, json, os, time

HF = r"C:\AnimeIAI\calibracao\copiloto-modelos\hf"
os.environ.setdefault("HF_HOME", HF)

NEG = "color, colored, colorful, blue, hair color, skin color, shading, screentone, gradient, gray fill, background, scenery, text, watermark, blurry, sketchy double lines"
POS_BASE = "monochrome, greyscale, lineart, clean anime line art, black ink lines on pure white background, no color, crisp outlines, professional genga cleanup"
BASES = {"sd15": "stable-diffusion-v1-5/stable-diffusion-v1-5", "anylora": "Lykon/AnyLoRA"}


def load_pipe(which, a):
    """Uma pipeline por vez (4 GB). O LCM-LoRA (e o de sketch) entra fundido na UNet; a do T2I-Adapter reusa essa
    UNet (a pipeline do adaptador não carrega LoRA nesta versão do diffusers)."""
    import torch
    from diffusers import StableDiffusionPipeline, StableDiffusionControlNetPipeline, ControlNetModel, StableDiffusionAdapterPipeline, T2IAdapter, LCMScheduler
    dt = torch.float16
    base = BASES[a.base]
    sd = StableDiffusionPipeline.from_pretrained(base, torch_dtype=dt, variant="fp16", safety_checker=None, requires_safety_checker=False)
    sd.load_lora_weights("latent-consistency/lcm-lora-sdv1-5", adapter_name="lcm")
    names, weights = ["lcm"], [1.0]
    if a.sketch_lora > 0:
        sd.load_lora_weights("jordanhilado/sd-1-5-sketch-lora", adapter_name="sketch")
        names.append("sketch")
        weights.append(a.sketch_lora)
    sd.set_adapters(names, adapter_weights=weights)
    sd.fuse_lora(adapter_names=names)
    sd.unload_lora_weights()
    comp = dict(vae=sd.vae, text_encoder=sd.text_encoder, tokenizer=sd.tokenizer, unet=sd.unet, scheduler=LCMScheduler.from_config(sd.scheduler.config),
                safety_checker=None, feature_extractor=None, requires_safety_checker=False)
    if which == "cn":
        cn = ControlNetModel.from_pretrained("lllyasviel/control_v11p_sd15s2_lineart_anime", torch_dtype=dt, variant="fp16")
        p = StableDiffusionControlNetPipeline(controlnet=cn, **comp)
    else:
        ad = T2IAdapter.from_pretrained("TencentARC/t2iadapter_sketch_sd15v2", torch_dtype=dt)
        p = StableDiffusionAdapterPipeline(adapter=ad, **comp)
    if a.vae == "taesd":
        from diffusers import AutoencoderTiny
        p.vae = AutoencoderTiny.from_pretrained("madebyollin/taesd", torch_dtype=dt)
    else:
        from diffusers import AutoencoderKL
        p.vae = AutoencoderKL.from_pretrained(base, subfolder="vae", torch_dtype=torch.float32, variant="fp16")
    if a.upcast:
        # GTX 16xx: atenção em fp32 (a soma do softmax em fp16 estoura em resoluções maiores)
        from diffusers.models.attention_processor import Attention
        for m in p.unet.modules():
            if isinstance(m, Attention):
                m.upcast_attention = True
                m.upcast_softmax = True
    p.to("cuda")
    if a.vae != "taesd":
        p.vae.to(torch.float32)
    p.set_progress_bar_config(disable=True)
    return p


def decode(pipe, lat):
    import torch
    from PIL import Image
    with torch.no_grad():
        if pipe.vae.__class__.__name__ == "AutoencoderTiny":
            x = pipe.vae.decode(lat / pipe.vae.config.scaling_factor).sample
        else:
            x = pipe.vae.decode(lat.float() / pipe.vae.config.scaling_factor).sample
    nan = bool(torch.isnan(x).any())
    x = torch.nan_to_num(x.float())
    img = Image.fromarray(((x[0].clamp(-1, 1) + 1) * 127.5).permute(1, 2, 0).round().byte().cpu().numpy())
    return img, nan


def control_image(path, res, kind):
    """O rascunho na resolução de geração. ControlNet lineart e T2I sketch esperam linha BRANCA sobre preto."""
    from PIL import Image, ImageOps
    im = Image.open(path).convert("L").resize((res, res), Image.LANCZOS)
    inv = ImageOps.invert(im)
    return inv.convert("RGB") if kind == "cn" else inv  # o adaptador de sketch quer 1 canal


def run(a):
    import torch
    casos = sorted(f for f in os.listdir(a.casos) if f.startswith("caso_") and f.endswith(".png"))
    os.makedirs(a.saida, exist_ok=True)
    rows = []
    tag = f"{a.base}{'_sk' if a.sketch_lora > 0 else ''}_{a.vae}_cfg{a.cfg:g}{'_up' if a.upcast else ''}"
    for nome_m in a.modelos.split(","):
        pipe = load_pipe(nome_m, a)
        for res in [int(x) for x in a.res.split(",")]:
            for passos in [int(x) for x in a.passos.split(",")]:
                for c in casos:
                    base = c[:-4]
                    txt = os.path.join(a.casos, base + ".txt")
                    prompt = POS_BASE + (", " + open(txt, encoding="utf-8").read().strip() if os.path.exists(txt) else "")
                    ctrl = control_image(os.path.join(a.casos, c), res, nome_m)
                    g = torch.Generator("cpu").manual_seed(a.seed)
                    kw = dict(prompt=prompt, num_inference_steps=passos, guidance_scale=a.cfg, width=res, height=res, generator=g)
                    if a.cfg > 1.0:
                        kw["negative_prompt"] = NEG
                    if nome_m == "cn":
                        kw.update(image=ctrl, controlnet_conditioning_scale=a.forca)
                    else:
                        kw.update(image=ctrl, adapter_conditioning_scale=a.forca)
                    torch.cuda.reset_peak_memory_stats()
                    torch.cuda.synchronize()
                    t0 = time.perf_counter()
                    lat = pipe(output_type="latent", **kw).images
                    lat_nan = bool(torch.isnan(lat).any())
                    img, img_nan = decode(pipe, lat)
                    torch.cuda.synchronize()
                    ms = (time.perf_counter() - t0) * 1000
                    out = os.path.join(a.saida, f"{base}__{nome_m}_{res}_{passos}__{tag}.png")
                    img.save(out)
                    rows.append({"caso": base, "modelo": nome_m, "res": res, "passos": passos, "config": tag, "ms": round(ms), "nan_latente": lat_nan,
                                 "nan_imagem": img_nan, "arquivo": out, "vram_pico_mb": round(torch.cuda.max_memory_allocated() / 2**20)})
                    print(json.dumps(rows[-1], ensure_ascii=False), flush=True)
        del pipe
        torch.cuda.empty_cache()
    path = os.path.join(a.saida, "tempos.json")
    old = json.load(open(path, encoding="utf-8")) if os.path.exists(path) else []
    json.dump(old + rows, open(path, "w", encoding="utf-8"), ensure_ascii=False, indent=1)


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--casos", required=True)
    ap.add_argument("--saida", required=True)
    ap.add_argument("--base", default="anylora", choices=list(BASES))
    ap.add_argument("--modelos", default="cn,t2i")
    ap.add_argument("--res", default="384,512")
    ap.add_argument("--passos", default="4,6")
    ap.add_argument("--forca", type=float, default=0.8)
    ap.add_argument("--cfg", type=float, default=1.0)
    ap.add_argument("--vae", default="taesd", choices=["taesd", "fp32"])
    ap.add_argument("--sketch-lora", type=float, default=0.0)
    ap.add_argument("--upcast", action="store_true")
    ap.add_argument("--seed", type=int, default=1)
    run(ap.parse_args())
