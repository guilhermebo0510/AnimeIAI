"""Serviço gerador do Copiloto de traço: processo separado que carrega os modelos UMA vez e atende pedidos por
stdin/stdout, uma linha JSON por mensagem (o app nunca espera a carga nem a geração na thread da interface).

Cadeia: AnyLoRA (SD 1.5 de anime) + ControlNet lineart anime + LCM-LoRA + TAESD → Anime2Sketch (só linha).

GTX 1650: o fp16 dela é 5 a 7 vezes MAIS LENTO que o fp32 (0,34 × 1,64 TFLOPS em matriz; 0,54 × 3,9 em convolução) e é
onde nasce o NaN (imagem preta). Os pesos ficam em fp16 (o fp32 inteiro não cabe nos 4 GB) e a conta é feita em fp32
(`computo_fp32`). Resultado: 384 px, 4 passos, de ~9 s para ~2 s; 512 a 768 px sem NaN. Defina AIAI_COPILOTO_FP16=1
para voltar à conta em fp16 (placas com fp16 rápido).

Mensagens:
  ← {"ready": true, "load_s": ..., "device": "cuda"}                    (uma vez, quando os modelos carregam)
  → {"id": 7, "image": "<png de contexto: linha preta sobre branco>", "out": "<png de saída>", "prompt": "...",
     "width": 512, "height": 384 (ou "res": 384, quadrado), "steps": 4, "strength": 0.8, "seed": 1,
     "cfg": 1.0 (>1 = o prompt pesa mais, com prompt negativo; dobra o custo do passo),
     "control_end": 1.0 (fração dos passos em que o desenho controla; menos = mais liberdade), "raw_out": "<png opcional>"}
  ← {"id": 7, "ok": true, "out": "...", "ms": 7650, "gen_ms": ..., "lines_ms": ..., "tokens": 31, "truncated": false,
     "size": [W, H] (o tamanho gerado de fato), "reduced": false (true = a resolução baixou para caber na memória livre),
     "gpu": {"power_limit": W, "default_limit": W, "sm_mhz": MHz, "throttled": bool}}   ou   {"id": 7, "ok": false, "error": "..."}

Memória de vídeo (a GTX 1650 tem 4 GB, e o canvas do AnimeIAI, o Clip Studio e o navegador também usam): o codificador
de texto fica na CPU (com cache por prompt), cada pedido confere a memória livre antes de gerar e baixa a resolução se
não couber (o app avisa), e o cache do torch é liberado depois. Antes, 768 px com cfg>1 passava dos 4 GB, a placa
transbordava para a RAM (minutos por sugestão) e o app chegou a perder o dispositivo de vídeo.
  → {"quit": true}

Só para teste de viabilidade (modelos prontos baixados para a bancada; licenças em RELATORIO-BANCADA-1.md).
"""
import json, os, sys, time, traceback

HF = r"C:\AnimeIAI\calibracao\copiloto-modelos\hf"
os.environ.setdefault("HF_HOME", HF)
os.environ.setdefault("HF_HUB_OFFLINE", "1")  # nada de rede durante o uso: só o que a bancada já baixou
A2S = r"C:\AnimeIAI\calibracao\copiloto-modelos\Anime2Sketch"
NEG = ("color, colored, shading, gradient, gray fill, screentone, hatching, background, scenery, speed lines, radial lines, motion lines, "
       "frame, border, text, watermark, blurry, sketchy double lines, lowres, bad anatomy")
# curto de propósito: o CLIP lê só 77 tokens, e o pedido do artista vem primeiro
POS = "lineart, monochrome, clean anime line art, black lines on white background"
MAX_TOKENS = 77


def send(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def computo_fp32(root):
    """Pesos guardados em fp16, conta em fp32: cada camada sobe o próprio peso para fp32 na hora (custa pouco: a cópia é
    limitada pela memória da placa, não pela conta)."""
    import torch.nn as nn
    import torch.nn.functional as F

    def f32(t):
        return None if t is None else t.float()

    for m in root.modules():
        if isinstance(m, nn.Linear):
            m.forward = (lambda m: lambda x: F.linear(x.float(), m.weight.float(), f32(m.bias)))(m)
        elif isinstance(m, nn.Conv2d):
            m.forward = (lambda m: lambda x: m._conv_forward(x.float(), m.weight.float(), f32(m.bias)))(m)
        elif isinstance(m, nn.GroupNorm):
            m.forward = (lambda m: lambda x: F.group_norm(x.float(), m.num_groups, f32(m.weight), f32(m.bias), m.eps))(m)
        elif isinstance(m, nn.LayerNorm):
            m.forward = (lambda m: lambda x: F.layer_norm(x.float(), m.normalized_shape, f32(m.weight), f32(m.bias), m.eps))(m)


FP32 = os.environ.get("AIAI_COPILOTO_FP16", "") != "1"


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
    # o codificador de texto vai para a CPU (em fp32): libera ~250 MB da placa; os prompts ficam em cache
    pipe.text_encoder.to("cpu", torch.float32)
    if FP32:
        for root in (pipe.unet, pipe.controlnet, pipe.vae):
            computo_fp32(root)
    pipe.set_progress_bar_config(disable=True)
    sys.path.insert(0, A2S)
    cwd = os.getcwd()
    os.chdir(A2S)
    from model import create_model
    a2s = create_model("improved").cuda().eval()
    os.chdir(cwd)
    return pipe, a2s


_EMB = {}
_GPU = {"t": 0.0, "info": {}}


def encode(pipe, text):
    """Embeddings do prompt (na CPU, com cache). Devolve (embeds na GPU, tokens usados, cortado?)."""
    import torch
    if text in _EMB:
        return _EMB[text]
    ids = pipe.tokenizer(text, truncation=False).input_ids
    tok = pipe.tokenizer(text, padding="max_length", max_length=MAX_TOKENS, truncation=True, return_tensors="pt")
    with torch.no_grad():
        emb = pipe.text_encoder(tok.input_ids)[0]
    out = (emb.to("cuda"), len(ids), len(ids) > MAX_TOKENS)
    if len(_EMB) > 64:
        _EMB.clear()
    _EMB[text] = out
    return out


def prompt_of(pipe, user):
    """O pedido do artista primeiro; o fixo de linha depois. Se não couber nos 77 tokens, corta o do artista (o fixo
    fica) e avisa."""
    user = (user or "").strip()
    if not user:
        return POS, False
    pos_n = len(pipe.tokenizer(POS).input_ids)
    ids = pipe.tokenizer(user).input_ids
    room = MAX_TOKENS - pos_n - 1
    if len(ids) - 2 > room:
        cut = pipe.tokenizer.decode(ids[1:1 + room], skip_special_tokens=True)
        return cut + ", " + POS, True
    return user + ", " + POS, False


def gpu_info():
    """Limite de potência e clock da placa (a cada 30 s): um notebook na bateria ou quente limita a GPU e cada geração
    fica várias vezes mais lenta."""
    import subprocess
    if time.time() - _GPU["t"] < 30:
        return _GPU["info"]
    _GPU["t"] = time.time()
    try:
        # "power.limit" vem [N/A] neste notebook; "enforced.power.limit" é o limite em vigor (5 W na bateria, 50 W na tomada)
        out = subprocess.run(["nvidia-smi", "--query-gpu=enforced.power.limit,power.default_limit,clocks.sm", "--format=csv,noheader,nounits"],
                             capture_output=True, text=True, timeout=5, creationflags=0x08000000).stdout.strip().split(",")
        lim, dflt, mhz = float(out[0]), float(out[1]), float(out[2])
        _GPU["info"] = {"power_limit": lim, "default_limit": dflt, "sm_mhz": mhz, "throttled": lim < 0.5 * dflt}
    except Exception:
        _GPU["info"] = {}
    return _GPU["info"]


def fit_memory(W, H, cfg):
    """Baixa W×H (múltiplos de 64, mínimo 256) até a geração caber na memória livre da placa. O disponível é o livre do
    dispositivo mais o cache reaproveitável do torch. Pico a mais medido na GTX 1650 depois da carga (2,5 GB de modelos):
    512² +0,50 GB (cfg 1) / +0,56 GB (cfg>1); 768² +0,57 / +0,77 GB. Reta pela área, mais 0,15 GB de folga."""
    import torch
    free, _ = torch.cuda.mem_get_info()
    avail = free + max(0, torch.cuda.memory_reserved() - torch.cuda.memory_allocated())
    def need(w, h):
        a = (w * h) / (768 * 768)
        g = (0.389 + 0.378 * a) if cfg > 1.0 else (0.433 + 0.139 * a)
        return (g + 0.15) * 2**30
    w, h = W, H
    while need(w, h) > avail and max(w, h) > 256:
        w, h = max(256, (w - 64) // 64 * 64), max(256, (h - 64) // 64 * 64)
    if need(w, h) > avail:
        raise RuntimeError(f"memória de vídeo insuficiente ({avail / 2**20:.0f} MB livres): feche outros programas que usam a GPU")
    return w, h, (w, h) != (W, H)


def finite(t, etapa):
    import torch
    if not torch.isfinite(t).all():
        raise RuntimeError(f"a geração saiu com valores inválidos (NaN/Inf) na etapa {etapa}")


def generate(pipe, a2s, req):
    import torch
    from PIL import Image, ImageOps
    import torchvision.transforms as T
    # tamanho de geração: width × height (no formato da janela de contexto); `res` = quadrado (compatível)
    def r64(v):
        return max(256, min(768, int(round(int(v) / 64)) * 64))
    if "width" in req and "height" in req:
        W, H = r64(req["width"]), r64(req["height"])
    else:
        W = H = r64(req.get("res", 384))
    cfg = float(req.get("cfg", 1.0))
    W, H, reduced = fit_memory(W, H, cfg)
    src = Image.open(req["image"]).convert("L")
    # sem esticar: o desenho entra inteiro em W×H mantendo a proporção (o resto é branco) e sai pelo mesmo recorte
    sw, sh = src.size
    k = min(W / sw, H / sh)
    nw, nh = max(1, round(sw * k)), max(1, round(sh * k))
    ox, oy = (W - nw) // 2, (H - nh) // 2
    board = Image.new("L", (W, H), 255)
    board.paste(src.resize((nw, nh), Image.LANCZOS), (ox, oy))
    ctrl = ImageOps.invert(board).convert("RGB")
    prompt, cut = prompt_of(pipe, req.get("prompt", ""))
    emb, ntok, trunc = encode(pipe, prompt)
    trunc = trunc or cut
    g = torch.Generator("cpu").manual_seed(int(req.get("seed", 1)))
    torch.cuda.synchronize()
    t0 = time.perf_counter()
    kw = {"prompt_embeds": emb}
    if cfg > 1.0:
        kw["negative_prompt_embeds"] = encode(pipe, NEG)[0]
    try:
        lat = pipe(image=ctrl, num_inference_steps=int(req.get("steps", 4)), guidance_scale=cfg, width=W, height=H, generator=g,
                   controlnet_conditioning_scale=float(req.get("strength", 0.8)), control_guidance_end=float(req.get("control_end", 1.0)),
                   output_type="latent", **kw).images
        finite(lat, "latentes")
        with torch.no_grad():
            x = pipe.vae.decode((lat.float() if FP32 else lat) / pipe.vae.config.scaling_factor).sample
        finite(x, "decodificador")
        torch.cuda.synchronize()
        t1 = time.perf_counter()
        if req.get("raw_out"):
            Image.fromarray(((x[0].float().clamp(-1, 1) + 1) * 127.5).permute(1, 2, 0).round().byte().cpu().numpy()).save(req["raw_out"])
        # Anime2Sketch: tira preenchimento e sombra, fica a linha (no múltiplo de 256 que a rede pede, perto do gerado)
        img = ((x.float().clamp(-1, 1) + 1) / 2)  # 0..1, [1,3,H,W]
        aw, ah = max(512, -(-W // 256) * 256), max(512, -(-H // 256) * 256)
        a = T.Normalize((0.5, 0.5, 0.5), (0.5, 0.5, 0.5))(T.functional.resize(img, [ah, aw], antialias=True))
        with torch.no_grad():
            y = a2s(a.cuda())
        finite(y, "Anime2Sketch")
        torch.cuda.synchronize()
        t2 = time.perf_counter()
        lines = ((y[0, 0].clamp(-1, 1) + 1) * 127.5).round().byte().cpu().numpy()
    finally:
        torch.cuda.empty_cache()
    out = Image.fromarray(lines).resize((W, H), Image.BICUBIC).crop((ox, oy, ox + nw, oy + nh)).resize(src.size, Image.BICUBIC)
    out.save(req["out"])
    return {"gen_ms": round((t1 - t0) * 1000), "lines_ms": round((t2 - t1) * 1000), "tokens": ntok, "truncated": bool(trunc),
            "size": [W, H], "reduced": reduced, "gpu": gpu_info()}


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
    # aquecimento: a primeira geração paga a preparação da placa (~1,5 s); melhor aqui do que na primeira sugestão
    try:
        import torch
        from PIL import Image
        import tempfile
        import shutil
        tmp = tempfile.mkdtemp(prefix="copiloto-")
        try:
            Image.new("L", (384, 384), 255).save(os.path.join(tmp, "a.png"))
            generate(pipe, a2s, {"image": os.path.join(tmp, "a.png"), "out": os.path.join(tmp, "b.png"), "res": 384, "steps": 1})
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
    except Exception as e:
        send({"ready": False, "error": f"o aquecimento falhou: {type(e).__name__}: {e}"})
        return
    send({"ready": True, "load_s": round(time.perf_counter() - t0, 1), "device": "cuda", "compute": "fp32" if FP32 else "fp16"})
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
