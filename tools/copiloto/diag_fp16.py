"""Diagnóstico: onde aparece NaN em fp16 na GTX 1650 (UNet/ControlNet ou VAE)?"""
import os, time
os.environ.setdefault("HF_HOME", r"C:\AnimeIAI\calibracao\copiloto-modelos\hf")
import torch
from diffusers import StableDiffusionPipeline, StableDiffusionControlNetPipeline, ControlNetModel, LCMScheduler, AutoencoderKL
from PIL import Image, ImageOps

base = "stable-diffusion-v1-5/stable-diffusion-v1-5"
dt = torch.float16
sd = StableDiffusionPipeline.from_pretrained(base, torch_dtype=dt, variant="fp16", safety_checker=None, requires_safety_checker=False)
sd.load_lora_weights("latent-consistency/lcm-lora-sdv1-5")
sd.fuse_lora()
sd.unload_lora_weights()
cn = ControlNetModel.from_pretrained("lllyasviel/control_v11p_sd15s2_lineart_anime", torch_dtype=dt, variant="fp16")
p = StableDiffusionControlNetPipeline(vae=sd.vae, text_encoder=sd.text_encoder, tokenizer=sd.tokenizer, unet=sd.unet, scheduler=LCMScheduler.from_config(sd.scheduler.config),
                                      controlnet=cn, safety_checker=None, feature_extractor=None, requires_safety_checker=False)
p.to("cuda")
p.set_progress_bar_config(disable=True)
print("vram após carregar MB", torch.cuda.memory_allocated() // 2**20)
img = Image.open(r"C:\AnimeIAI\calibracao\copiloto-modelos\casos-bancada\caso_menino_rascunho.png").convert("L").resize((384, 384))
ctrl = ImageOps.invert(img).convert("RGB")
for rodada in range(2):
    torch.cuda.synchronize(); t0 = time.perf_counter()
    lat = p(prompt="clean anime lineart, black ink lines on white background, monochrome", image=ctrl, num_inference_steps=4, guidance_scale=1.5,
            width=384, height=384, output_type="latent", generator=torch.Generator("cpu").manual_seed(1)).images
    torch.cuda.synchronize(); t1 = time.perf_counter()
    print("latentes: nan?", bool(torch.isnan(lat).any()), "min/max", float(lat.float().min()), float(lat.float().max()), "tempo UNet", round(t1 - t0, 2), "s")
# VAE em fp32
vae32 = AutoencoderKL.from_pretrained(base, subfolder="vae", torch_dtype=torch.float32, variant="fp16").to("cuda")
with torch.no_grad():
    t0 = time.perf_counter()
    x = vae32.decode(lat.float() / vae32.config.scaling_factor).sample
    torch.cuda.synchronize()
    print("vae fp32: nan?", bool(torch.isnan(x).any()), "tempo", round(time.perf_counter() - t0, 2), "s")
    x16 = sd.vae.decode(lat / sd.vae.config.scaling_factor).sample
    print("vae fp16: nan?", bool(torch.isnan(x16).any()))
im = ((x[0].clamp(-1, 1) + 1) * 127.5).permute(1, 2, 0).byte().cpu().numpy()
Image.fromarray(im).save(r"C:\AnimeIAI\calibracao\copiloto-modelos\diag.png")
print("vram pico MB", torch.cuda.max_memory_allocated() // 2**20)
