"""Perfil do Copiloto de traço na placa do usuário: onde vai o tempo de cada passo (UNet × ControlNet) e onde nasce
o NaN em fp16 (GTX 16xx), para cada combinação de ajustes. Um processo por configuração (o estado do cuDNN é global).

Uso:
  python tools/copiloto/perfil.py --res 384,512 --passos 4 [--cudnn-bench] [--atencao sdpa|sdpa32|classica32]
         [--channels-last] [--vae taesd|kl16] [--rastrear-nan] [--saida <pasta>]
Imprime uma linha JSON por geração.
"""
import argparse, json, os, sys, time

HF = r"C:\AnimeIAI\calibracao\copiloto-modelos\hf"
os.environ.setdefault("HF_HOME", HF)
os.environ.setdefault("HF_HUB_OFFLINE", "1")
CASO = r"C:\AnimeIAI\calibracao\copiloto-modelos\casos-bancada\caso_menino_rascunho.png"
POS = "monochrome, greyscale, lineart, clean anime line art, black ink lines on pure white background, no color, crisp outlines, professional genga cleanup"


def sdpa_fp32():
    """A atenção em fp32 sem perder o SDPA (memória linear): q, k, v sobem para fp32 e o resultado volta ao tipo de
    entrada."""
    import torch
    import torch.nn.functional as F
    orig = F.scaled_dot_product_attention

    def f(q, k, v, *a, **kw):
        t = q.dtype
        if t == torch.float16:
            m = kw.get("attn_mask")
            if m is not None and m.dtype == torch.float16:
                kw["attn_mask"] = m.float()
            return orig(q.float(), k.float(), v.float(), *a, **kw).to(t)
        return orig(q, k, v, *a, **kw)

    F.scaled_dot_product_attention = f


from servico import computo_fp32  # noqa: E402  (a mesma do serviço)


def load(a):
    import torch
    from diffusers import StableDiffusionPipeline, StableDiffusionControlNetPipeline, ControlNetModel, LCMScheduler, AutoencoderTiny
    if a.cudnn_bench:
        torch.backends.cudnn.benchmark = True
    if a.atencao == "sdpa32":
        sdpa_fp32()
    dt = torch.float16
    sd = StableDiffusionPipeline.from_pretrained("Lykon/AnyLoRA", torch_dtype=dt, variant="fp16", safety_checker=None, requires_safety_checker=False)
    sd.load_lora_weights("latent-consistency/lcm-lora-sdv1-5", weight_name="pytorch_lora_weights.safetensors", adapter_name="lcm")
    sd.fuse_lora(adapter_names=["lcm"])
    sd.unload_lora_weights()
    cn = ControlNetModel.from_pretrained("lllyasviel/control_v11p_sd15s2_lineart_anime", torch_dtype=dt, variant="fp16")
    vae = AutoencoderTiny.from_pretrained("madebyollin/taesd", torch_dtype=dt) if a.vae == "taesd" else sd.vae
    pipe = StableDiffusionControlNetPipeline(vae=vae, text_encoder=sd.text_encoder, tokenizer=sd.tokenizer, unet=sd.unet,
                                             scheduler=LCMScheduler.from_config(sd.scheduler.config), controlnet=cn, safety_checker=None,
                                             feature_extractor=None, requires_safety_checker=False)
    if a.atencao == "classica32":
        from diffusers.models.attention_processor import AttnProcessor, Attention
        for m in list(pipe.unet.modules()) + list(pipe.controlnet.modules()):
            if isinstance(m, Attention):
                m.upcast_attention = True
                m.upcast_softmax = True
        pipe.unet.set_attn_processor(AttnProcessor())
        pipe.controlnet.set_attn_processor(AttnProcessor())
    if a.channels_last:
        pipe.unet.to(memory_format=torch.channels_last)
        pipe.controlnet.to(memory_format=torch.channels_last)
    pipe.to("cuda")
    if a.computo == "fp32":
        for root in (pipe.unet, pipe.controlnet, pipe.vae):
            computo_fp32(root)
    pipe.set_progress_bar_config(disable=True)
    return pipe


class Relogio:
    """Soma o tempo de GPU de cada chamada de um módulo (sincroniza antes e depois)."""

    def __init__(self, mod):
        import torch
        self.ms, self.n = 0.0, 0
        orig = mod.forward

        def fwd(*x, **kw):
            torch.cuda.synchronize()
            t = time.perf_counter()
            r = orig(*x, **kw)
            torch.cuda.synchronize()
            self.ms += (time.perf_counter() - t) * 1000
            self.n += 1
            return r

        mod.forward = fwd


def rastrear_nan(pipe, achados):
    """Guarda o primeiro módulo (na ordem de execução) cuja saída tem NaN ou infinito."""
    import torch

    def hook(nome):
        def h(m, i, o):
            if achados:
                return
            ts = o if isinstance(o, (tuple, list)) else [getattr(o, "sample", o)]
            for t in ts:
                if torch.is_tensor(t) and t.is_floating_point() and not torch.isfinite(t).all():
                    vals = [float(x.abs().max()) for x in i if torch.is_tensor(x) and x.is_floating_point() and torch.isfinite(x).all()]
                    achados.append({"modulo": nome, "tipo": type(m).__name__, "max_entrada": max(vals) if vals else None})
                    return
        return h

    for pref, root in (("controlnet", pipe.controlnet), ("unet", pipe.unet)):
        for nome, m in root.named_modules():
            if nome and not list(m.children()):
                m.register_forward_hook(hook(f"{pref}.{nome}"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--res", default="384,512")
    ap.add_argument("--passos", default="4")
    ap.add_argument("--cudnn-bench", action="store_true")
    ap.add_argument("--atencao", default="sdpa", choices=["sdpa", "sdpa32", "classica32"])
    ap.add_argument("--channels-last", action="store_true")
    ap.add_argument("--vae", default="taesd", choices=["taesd", "kl16"])
    ap.add_argument("--rastrear-nan", action="store_true")
    ap.add_argument("--computo", default="fp16", choices=["fp16", "fp32"])
    ap.add_argument("--repetir", type=int, default=2)
    ap.add_argument("--saida", default="")
    a = ap.parse_args()
    import torch
    from PIL import Image, ImageOps
    t0 = time.perf_counter()
    pipe = load(a)
    carga = round(time.perf_counter() - t0, 1)
    ru, rc = Relogio(pipe.unet), Relogio(pipe.controlnet)
    achados = []
    if a.rastrear_nan:
        rastrear_nan(pipe, achados)
    cfg = {"computo": a.computo, "cudnn_bench": a.cudnn_bench, "atencao": a.atencao, "channels_last": a.channels_last, "vae": a.vae}
    src = Image.open(CASO).convert("L")
    for res in [int(x) for x in a.res.split(",")]:
        ctrl = ImageOps.invert(src.resize((res, res), Image.LANCZOS)).convert("RGB")
        for passos in [int(x) for x in a.passos.split(",")]:
            for rep in range(a.repetir):
                ru.ms = rc.ms = 0.0
                ru.n = rc.n = 0
                achados.clear()
                torch.cuda.reset_peak_memory_stats()
                torch.cuda.synchronize()
                t = time.perf_counter()
                lat = pipe(prompt=POS, image=ctrl, num_inference_steps=passos, guidance_scale=1.0, width=res, height=res,
                           generator=torch.Generator("cpu").manual_seed(1), controlnet_conditioning_scale=0.8, output_type="latent").images
                torch.cuda.synchronize()
                tg = time.perf_counter()
                with torch.no_grad():
                    x = pipe.vae.decode((lat.float() if a.computo == "fp32" else lat) / pipe.vae.config.scaling_factor).sample
                torch.cuda.synchronize()
                td = time.perf_counter()
                row = {**cfg, "res": res, "passos": passos, "rep": rep, "carga_s": carga, "total_ms": round((td - t) * 1000),
                       "unet_ms_passo": round(ru.ms / max(ru.n, 1)), "cn_ms_passo": round(rc.ms / max(rc.n, 1)),
                       "decod_ms": round((td - tg) * 1000), "nan_latente": not bool(torch.isfinite(lat).all()),
                       "nan_imagem": not bool(torch.isfinite(x).all()), "vram_pico_mb": round(torch.cuda.max_memory_allocated() / 2**20),
                       "primeiro_nan": achados[0] if achados else None}
                if a.saida and rep == a.repetir - 1:
                    os.makedirs(a.saida, exist_ok=True)
                    im = ((torch.nan_to_num(x[0].float()).clamp(-1, 1) + 1) * 127.5).permute(1, 2, 0).round().byte().cpu().numpy()
                    nome = f"{a.computo}_{a.atencao}{'_bench' if a.cudnn_bench else ''}{'_cl' if a.channels_last else ''}_{a.vae}_{res}_{passos}.png"
                    Image.fromarray(im).save(os.path.join(a.saida, nome))
                    row["arquivo"] = nome
                print(json.dumps(row, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    main()
