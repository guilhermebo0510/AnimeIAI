"""Serviço gerador do Copiloto de traço: processo separado que carrega os modelos UMA vez e atende pedidos por
stdin/stdout, uma linha JSON por mensagem (o app nunca espera a carga nem a geração na thread da interface).

Cadeia: AnyLoRA (SD 1.5 de anime) + ControlNet lineart anime + LCM-LoRA + TAESD → Anime2Sketch (só linha).

Mensagens:
  ← {"ready": true, "load_s": ..., "device": "cuda"}                    (uma vez, quando os modelos carregam)
  → {"id": 7, "image": "<png de contexto: linha preta sobre branco>", "out": "<png de saída>", "prompt": "...",
     "res": 384, "steps": 4, "strength": 0.8, "seed": 1}
  ← {"id": 7, "ok": true, "out": "...", "ms": 7650, "gen_ms": ..., "lines_ms": ...}   ou   {"id": 7, "ok": false, "error": "..."}
  → {"quit": true}

Só para teste de viabilidade (modelos prontos baixados para a bancada; licenças em RELATORIO-BANCADA-1.md).
"""
import json, os, sys, time, traceback

HF = r"C:\AnimeIAI\calibracao\copiloto-modelos\hf"
os.environ.setdefault("HF_HOME", HF)
os.environ.setdefault("HF_HUB_OFFLINE", "1")  # nada de rede durante o uso: só o que a bancada já baixou
A2S = r"C:\AnimeIAI\calibracao\copiloto-modelos\Anime2Sketch"
POS = "monochrome, greyscale, lineart, clean anime line art, black ink lines on pure white background, no color, crisp outlines, professional genga cleanup"


def send(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def load():
    import torch
    from diffusers import StableDiffusionPipeline, StableDiffusionControlNetPipeline, ControlNetModel, LCMScheduler, AutoencoderTiny
    dt = torch.float16
    sd = StableDiffusionPipeline.from_pretrained("Lykon/AnyLoRA", torch_dtype=dt, variant="fp16", safety_checker=None, requires_safety_checker=False)
    sd.load_lora_weights("latent-consistency/lcm-lora-sdv1-5", weight_name="pytorch_lora_weights.safetensors", adapter_name="lcm")
    sd.fuse_lora(adapter_names=["lcm"])
    sd.unload_lora_weights()
    cn = ControlNetModel.from_pretrained("lllyasviel/control_v11p_sd15s2_lineart_anime", torch_dtype=dt, variant="fp16")
    pipe = StableDiffusionControlNetPipeline(vae=AutoencoderTiny.from_pretrained("madebyollin/taesd", torch_dtype=dt), text_encoder=sd.text_encoder,
                                             tokenizer=sd.tokenizer, unet=sd.unet, scheduler=LCMScheduler.from_config(sd.scheduler.config), controlnet=cn,
                                             safety_checker=None, feature_extractor=None, requires_safety_checker=False)
    pipe.to("cuda")
    pipe.set_progress_bar_config(disable=True)
    sys.path.insert(0, A2S)
    cwd = os.getcwd()
    os.chdir(A2S)
    from model import create_model
    a2s = create_model("improved").cuda().eval()
    os.chdir(cwd)
    return pipe, a2s


def generate(pipe, a2s, req):
    import torch
    from PIL import Image, ImageOps
    import torchvision.transforms as T
    res = int(req.get("res", 384))
    res = max(256, min(768, (res // 64) * 64))
    src = Image.open(req["image"]).convert("L")
    ctrl = ImageOps.invert(src.resize((res, res), Image.LANCZOS)).convert("RGB")
    prompt = POS + (", " + req["prompt"].strip() if req.get("prompt", "").strip() else "")
    g = torch.Generator("cpu").manual_seed(int(req.get("seed", 1)))
    torch.cuda.synchronize()
    t0 = time.perf_counter()
    lat = pipe(prompt=prompt, image=ctrl, num_inference_steps=int(req.get("steps", 4)), guidance_scale=1.0, width=res, height=res, generator=g,
               controlnet_conditioning_scale=float(req.get("strength", 0.8)), output_type="latent").images
    with torch.no_grad():
        x = pipe.vae.decode(lat / pipe.vae.config.scaling_factor).sample
    if torch.isnan(x).any():
        raise RuntimeError("a geração saiu com valores inválidos (NaN) nesta resolução")
    torch.cuda.synchronize()
    t1 = time.perf_counter()
    # Anime2Sketch: tira preenchimento e sombra, fica a linha
    img = ((x.float().clamp(-1, 1) + 1) / 2)  # 0..1, [1,3,H,W]
    a = T.Normalize((0.5, 0.5, 0.5), (0.5, 0.5, 0.5))(T.functional.resize(img, [512, 512], antialias=True))
    with torch.no_grad():
        y = a2s(a.cuda())
    torch.cuda.synchronize()
    t2 = time.perf_counter()
    lines = ((y[0, 0].clamp(-1, 1) + 1) * 127.5).round().byte().cpu().numpy()
    out = Image.fromarray(lines).resize(src.size, Image.BICUBIC)
    out.save(req["out"])
    return {"gen_ms": round((t1 - t0) * 1000), "lines_ms": round((t2 - t1) * 1000)}


def main():
    # o app lê UTF-8 (no Windows o padrão do console é outro)
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stdin.reconfigure(encoding="utf-8")
    t0 = time.perf_counter()
    try:
        pipe, a2s = load()
    except Exception as e:
        send({"ready": False, "error": f"não consegui carregar os modelos: {e}"})
        return
    send({"ready": True, "load_s": round(time.perf_counter() - t0, 1), "device": "cuda"})
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError as e:
            send({"ok": False, "error": f"pedido inválido: {e}"})
            continue
        if req.get("quit"):
            break
        t = time.perf_counter()
        try:
            info = generate(pipe, a2s, req)
            send({"id": req.get("id"), "ok": True, "out": req["out"], "ms": round((time.perf_counter() - t) * 1000), **info})
        except Exception as e:
            send({"id": req.get("id"), "ok": False, "error": f"{type(e).__name__}: {e}", "trace": traceback.format_exc()[-600:]})


if __name__ == "__main__":
    main()
