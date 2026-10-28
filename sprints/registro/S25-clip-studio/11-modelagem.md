# Modelagem: ANIGO + Blender (seção 4 do plano, 28/10/2026)

Ponto de partida: o porte de 20/10 (`sprints/registro/S24-pesquisa/modelagem-anigo.md`, `VENDOR.md` do módulo)
trouxe do ANIGO o viewport nativo (Workbench + prévia cel), câmera, gizmos, seleção, modais G/R/S com proporcional,
35 operadores com "Ajustar última operação" e 9 modificadores. Ficaram de fora: multivista, escultura, UV, normais
e cabelo.

## O que o Blender tem e faltava (por prioridade para quem modela personagem/cenário de anime)

| Item | Blender | AnimeIAI | Situação |
|---|---|---|---|
| Multivista | Ctrl+Alt+Q (quatro vistas), Ctrl+Espaço (maximizar), vistas alinhadas com grade no plano | multiview do ANIGO + grade da vista ortográfica alinhada | feito (28/10) |
| Escultura | pincéis (puxar, alisar, inflar, beliscar, achatar, máscara), simetria, dyntopo, remesh | modo Escultura com os 20 pincéis do ANIGO, simetria XYZ, topologia dinâmica, máscara visível, alisar tudo, remalhar (voxel), simetrizar; ponte `UI_ACTION sculpt` | feito (28/10); o espelho incremental na GPU do ANIGO não veio: em malhas grandes o traço sobe o objeto inteiro a cada quadro |
| UV | costuras, desdobrar, empacotar, editor 2D | `anigo-modeling/uv` + `uv_ui*` | depois |
| Normais personalizadas (rosto anime) | normais do modificador "Data Transfer", editar normais | `normals_ui.rs` do ANIGO | depois |
