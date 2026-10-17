"""Cor por corte (grade no topo de cada corte): luz quente de entardecer, um pouco mais de contraste no gag e um tom
mais dourado na varanda. Folha antes × depois do quadro do meio de cada corte."""
from curta import d, ids, OUT

G = {  # corte: (gain, temperatura, saturação, contraste)
    0: ([1.04, 1.0, 0.94], 0.12, 1.05, 1.05),
    1: ([1.04, 1.0, 0.95], 0.10, 1.06, 1.04),
    2: ([1.03, 1.0, 0.96], 0.08, 1.04, 1.04),
    3: ([1.03, 1.0, 0.96], 0.08, 1.04, 1.04),
    4: ([1.02, 1.0, 0.98], 0.02, 1.12, 1.10),
    5: ([1.06, 1.0, 0.92], 0.15, 1.04, 1.05),
    6: ([1.03, 1.0, 0.95], 0.08, 1.05, 1.04),
}
from PIL import Image
pairs = []
for i, c in enumerate(ids["cuts"]):
    dur = d.query({"kind": "node", "id": c})["data"]["duration"]
    f = dur // 2
    before = f"{OUT}/grade_c{i + 1}_antes.png"
    d.call("GRADE", {"cut": c, "effects": []})
    d.call("RENDER", {"cut": c, "frame": f, "out": before})
    gain, temp, sat, con = G[i]
    d.call("GRADE", {"cut": c, "effects": [{"effect": {"type": "grade", "gain": gain, "temperature": temp, "saturation": sat, "contrast": con, "pivot": 0.45}}]})
    after = f"{OUT}/grade_c{i + 1}_depois.png"
    d.call("RENDER", {"cut": c, "frame": f, "out": after})
    pairs.append((before, after))
sheet = Image.new("RGB", (960, 270 * len(pairs)))
for i, (a, b) in enumerate(pairs):
    sheet.paste(Image.open(a).convert("RGB").resize((480, 270)), (0, 270 * i))
    sheet.paste(Image.open(b).convert("RGB").resize((480, 270)), (480, 270 * i))
sheet.save(f"{OUT}/grade_antes_depois.png")
print(f"{OUT}/grade_antes_depois.png")
