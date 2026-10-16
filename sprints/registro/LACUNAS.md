# Lacunas descobertas durante a execução

Cada lacuna: nº · data · onde apareceu (sprint/corte/tentativa) · tipo (ferramenta faltando / ferramenta fraca / bug / ergonomia para o agente / desempenho) · descrição com evidência · sprint de origem · estado (aberta / resolvida em <commit> / adiada: motivo).

| Nº | Data | Origem | Tipo | Descrição | Sprint | Estado |
|---|---|---|---|---|---|---|
| 1 | 2026-10-16 | S11 cenário externo | ferramenta fraca | Grama em perspectiva com moiré ao longe (supersampling 2×2 não basta) — precisa de filtragem por distância (LOD/mip da textura procedural) | S11 | resolvida (amostragem adaptativa pela área do pixel na textura, 2..8²) |
| 2 | 2026-10-16 | S11 cenário externo | ergonomia agente | Mapear textura em perspectiva exige o agente calcular quadriláteros; oferecer modo "plano 3D + câmera" (usa a guia 3D direto, com recorte no plano próximo) | S11/S09 | aberta |
| 3 | 2026-10-16 | S11 cenário externo | ferramenta fraca | Pincel "Flores/pétalas" parece confete; rosas pedem pincel de flor agrupada (pétalas em volta de um centro) | S11 | resolvida (ponta "Rosa" em espiral + preset Rosas) |
| 4 | 2026-10-16 | S06 estudo de céu | execução | Nuvens desenhadas como arcos finos; massas pedem preenchimento + pincel de nuvem (agora existe gerador de céu e ponta de nuvem) | S06/S11 | resolvida (gerador de céu + ponta de nuvem) |
| 5 | 2026-10-16 | S09 guia 3D | bug | Remoção de linhas ocultas não funcionava (inicialização do máximo com +∞) e RDP apagava polilinhas de ida e volta | S09 | resolvida |
| 6 | 2026-10-16 | S10 model sheet | ferramenta fraca | Manequim simétrico demais: 3/4 frente e 3/4 costas quase iguais; marcador de nariz ajudou; falta peito/omoplata e mãos com direção | S10 | aberta (parcial) |
