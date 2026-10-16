# Latencia do editor (S03/B2): soltar o mouse -> canvas atualizado

Corte 1920x1080, 5 camadas com conteudo, mouse real (SendInput), 6 repeticoes por ferramenta. Tempos em ms.

| Ferramenta | n | ferramenta (mediana) | render CPU (mediana) | total soltar->canvas (mediana) | total (max) |
|---|---|---|---|---|---|
| pen | 6 | 0 | 147 | 164 | 202 |
| brush | 6 | 23 | 161 | 193 | 204 |
| fill | 6 | 294 | 189 | 508 | 543 |
| erase_vector | 3 | 1 | 158 | 177 | 182 |
