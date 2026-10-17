"""Exporta o curta: a edição inteira (legendas queimadas, créditos) em H.264 com a mixagem normalizada ARIB e o .srt
de entrega ao lado. Confere quadros pedidos × quadros do arquivo pelo ffprobe."""
import json
from curta import d, ids, ROOT

r = d.call("EXPORT_VIDEO", {"edit": ids["edit"], "out": f"{ROOT}/ultima-rosa.mp4", "preset": "arib", "sidecar_srt": True, "crf": 18}, timeout=3600)
print(json.dumps({k: r.get(k) for k in ("path", "frames", "expected_frames", "size", "probe", "audio", "warnings")}, ensure_ascii=False, indent=1))
