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
| UV | costuras, desdobrar, empacotar, editor 2D | editor de UV do ANIGO: painel "Editor de UV" (ilhas, estiramento, densidade, xadrez, G/R/S, seleção sincronizada com o 3D) e a seção UV nas Propriedades (costuras manuais, por ângulo, caminho, automáticas de personagem; desdobrar LSCM, minimizar estiramento, empacotar, normalizar densidade; projeções planar/caixa/cilindro/esfera/smart/da vista); ponte `UI_ACTION uv` | feito (28/10) |
| Normais personalizadas (rosto anime) | normais do modificador "Data Transfer", editar normais | motor `normals_tools` do ANIGO + seção "Normais (sombra cel)" nas Propriedades (da esfera/centro da seleção, da vista, de outro objeto, média, suavizar, restaurar); ponte `UI_ACTION normals`; o objeto passa a "Automático" (que respeita as personalizadas) | feito (28/10); a vista/pintura de normais do `normals_ui.rs` ficou de fora |

## Fora desta rodada

- **Cabelo** (`anigo-modeling/hair` + `curve.rs` + `hair.rs`/`hair_ui.rs`, ~6 mil linhas: mechas como cartões, pente,
  cadeias de física): é ferramenta de personagem 3D. No AnimeIAI o personagem é desenhado em 2D e a Modelagem serve
  para adereços e cenário (o comentário do layout diz "modelagem de adereços"). Fica para quando houver personagem 3D.
- **Espelho incremental da escultura na GPU** (`sculpt_mesh.rs`): o traço sobe o objeto inteiro a cada quadro. Em
  malhas de dezenas de milhares de faces o traço fica mais lento; vale trazer se aparecer uso pesado.
- **Vista e pintura de normais** (`normals_ui.rs`): as operações estão no painel; a visualização das normais no
  viewport não.

## Conferência

`tools/csp/modeling_views_test.py`, `sculpt_test.py`, `uv_test.py`, `normals_test.py` (instância de teste, ponte e
cliques de verdade); fotos em `sprints/registro/evidencias/S25/modelagem/`.
