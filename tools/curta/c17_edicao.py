"""Edição do curta: 7 cortes (abre do preto, flash no gag, dissolve para a varanda), legendas em português nas falas
(posicionadas pela linha do tempo real da edição + início de cada fala no corte), créditos com as vozes VOICEVOX,
fade no fim. Confere com aiai_edit_preview (legendas) e aiai_edit_mix (loudness ARIB -24)."""
import os
from curta import d, ids, save_ids, ROOT

cuts = ids["cuts"]
FPS = 24000 / 1001
clips = [{"cut": c} for c in cuts]
clips[0]["transition_in"] = {"kind": "fade_black", "frames": 12}
clips[4]["transition_in"] = {"kind": "flash", "frames": 3}
clips[5]["transition_in"] = {"kind": "dissolve", "frames": 8}
credits = ["最後の一輪 — A última rosa", "",
           "Roteiro, storyboard, desenho, animação, cor e direção", "Claude, no AnimeIAI Studio", "",
           "Música original e efeitos sonoros", "compostos e sintetizados no AnimeIAI", "",
           "Vozes", "VOICEVOX:春日部つむぎ (Hana)", "VOICEVOX:白上虎太郎 (Ren)", "",
           "Texturas", "procedurais do AnimeIAI (nenhuma imagem externa)", "",
           "Nenhuma imagem gerada por IA generativa", "2026"]
base = {"name": "最後の一輪 — edição", "clips": clips, "credits": credits, "credits_seconds": 7.0, "fade_out_end": 12}
if "edit" in ids:
    base["id"] = ids["edit"]
r = d.call("EDIT_SET", base)
ids["edit"] = r["edit"]
save_ids()
tl = r["timeline"]
print("linha do tempo:", {k: v for k, v in tl.items() if k != "clips"}, tl.get("clips", tl.get("items")))

# legendas: início do clipe na edição + início da fala no corte
items = tl.get("clips") or tl.get("items")
def clip_start(i):
    it = items[i]
    return it["start"] if isinstance(it, dict) else it[0]

LINES = [(2, "tts_f3", "Eu vi primeiro!"), (3, "tts_f4", "Eu também quero dar pra vovó."),
         (4, "tts_f5", "…Vovó?"), (5, "tts_f6", "Então vamos entregar juntos.")]
subs = []
for ci, key, text in LINES:
    data = d.query({"kind": "node", "id": cuts[ci]})["data"]
    pl = next(a for a in data["audio"] if a.get("id") == key)
    s0 = clip_start(ci) + pl["start_frame"]
    dur = round(ids[key + "_s"] * FPS)
    subs.append({"start_frame": s0 - 2, "end_frame": s0 + max(dur, 34) + 6, "text": text})
base["id"] = ids["edit"]
base["subtitles"] = subs
r = d.call("EDIT_SET", base)
print("legendas:", subs)
total = r["timeline"]["total_frames"]
print("total:", total, "quadros =", round(total / FPS, 2), "s")
pv = d.call("EDIT_PREVIEW", {"edit": ids["edit"], "frames": [s["start_frame"] + 12 for s in subs] + [10, total - 60], "out": f"{ROOT}/saida/edicao_previa.png"})
print("prévia:", pv.get("path") if isinstance(pv, dict) else pv)
mx = d.call("EDIT_MIX", {"edit": ids["edit"], "out": f"{ROOT}/saida/mix_arib.wav", "preset": "arib"})
print("mix:", {k: mx.get(k) for k in ("duration_s", "loudness", "normalize", "path")})
