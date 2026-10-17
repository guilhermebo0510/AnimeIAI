"""Teste das falas com os perfis de voz (assets sem posição): duração real de cada fala para o storyboard."""
from curta import d, ids, save_ids
LINES = {
    "f3": ("hana", "それ、私が先に見つけたの！", None),
    "f4": ("ren", "僕だって、ばあちゃんに渡したいんだ。", None),
    "f5": ("hana", "…ばあちゃん？", None),
    "f6": ("hana", "じゃあ、一緒に渡そう。", None),
}
for k, (who, text, sp) in LINES.items():
    key = "tts_" + k
    if key not in ids:
        r = d.call("TTS_LINE", {"text": text, "character": ids["char_" + who], "name": f"Fala {k} ({who})"})
        ids[key] = r["asset"]; ids[key + "_s"] = r["duration_s"]; save_ids()
    print(k, who, text, round(ids[key + "_s"], 2), "s")
