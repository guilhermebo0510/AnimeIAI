"""Leitura: F1 de bordas (Canny replicado, 2 px) só dentro de uma caixa. uso: python boxf1.py ref.png render.png x0,y0,x1,y1 [...]"""
import sys
import numpy as np
from scipy.ndimage import distance_transform_edt
args = sys.argv[1:]
sys.argv = ["x", "a", "b", "0,0,1,1", "o"]
exec(open(__file__.replace("boxf1.py", "canny_pair.py"), encoding="utf-8").read().split("ref, ren, box")[0])
Fp, Rp = args[0], args[1]
mf, _, _ = grad(Fp)
high = max(float(np.percentile(mf, 95)), 10.0)
Er, _ = canny(Fp, high, 0.4 * high)
Et, _ = canny(Rp, high, 0.4 * high)
dr, dt = distance_transform_edt(~Er), distance_transform_edt(~Et)
for b in args[2:]:
    x0, y0, x1, y1 = (int(v) for v in b.split(","))
    s = (slice(y0, y1), slice(x0, x1))
    nr, nt = Er[s].sum(), Et[s].sum()
    rec = (Er[s] & (dt[s] <= 2)).sum() / max(1, nr)
    pre = (Et[s] & (dr[s] <= 2)).sum() / max(1, nt)
    print(b, "ref", int(nr), "render", int(nt), "recall", round(float(rec), 3), "precisão", round(float(pre), 3),
          "F1", round(float(2 * rec * pre / max(1e-9, rec + pre)), 3))
