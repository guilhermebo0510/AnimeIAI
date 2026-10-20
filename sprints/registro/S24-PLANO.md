# S24 — Plano mestre (pedido do usuário de 2026-10-19)

Base: quatro pesquisas em `sprints/registro/S24-pesquisa/`:
- `workspaces.md`: o que cada workspace deve ser, os painéis novos e o layout de fábrica × personalizado;
- `formatos.md`: o formato de projeto, o salvamento/recuperação, o formato de pincel e a biblioteca do usuário;
- `nucleo.md`: o núcleo comparado a Photoshop, Clip Studio, Procreate e Krita;
- `3d.md`: o 3D como referência, a Modelagem e a Biblioteca, com o reaproveitamento do ANIGO.

O plano junta os backlogs delas em ondas. Itens grandes dependem de spikes medidos antes, como pedem os princípios do usuário.

## Já feito nesta rodada (commits)

- Seleção:
  - caixa de seleção de traços no Mover;
  - Ctrl+A e Ctrl+D;
  - Ctrl+T com 8 alças: escala livre, Shift trava a proporção, Alt a partir do centro, espelhar (`3a6c235`).
- Painel Camadas:
  - escolha múltipla;
  - arrastar para reordenar ou para dentro de uma pasta;
  - Agrupar (Ctrl+G);
  - Delete;
  - recorte explicado, com aviso;
  - papel do cel sem cobrir o nome (`401c9ea`).

## Onda 1 (em andamento)

| Item | Fonte | Quem | Arquivos principais |
|---|---|---|---|
| Endurecimento do salvamento: fsync dos blobs, abrir tolerante a blob ou WAL danificado, `brushes.json` nunca sobrescrito sem cópia, trava de projeto aberto, `.bak` rotativo, miniatura | formatos P0 | agente Opus | aiai-core persist/blob, studio doc.rs, brushes.rs |
| Fundação dos workspaces: perfil por workspace (ferramentas e padrão), catálogo de painéis, layouts salvos (`layouts.json`), layouts com nome, voltar ao de fábrica por workspace, Exibir > Painéis completo, Cor → Biblioteca, Modelagem (esqueleto), ordem e grupos da barra, fábricas novas por workspace com os painéis atuais | workspaces Fase 0 + 1.1 | agente Opus | layout.rs, workspace.rs, app.rs (menus) |
| Ferramenta Texto + painel Texto + camada de texto editável (fonte, tamanho, entrelinha, espaçamento, alinhamento, vertical, contorno, sombra) | nucleo backlog | agente Sonnet | aiai-paint text.rs, aiai-core LayerType::Text, studio (módulo novo) |

## Onda 2

- **Pincel no nosso formato:**
  - `.aiaibrush` e `.aiaibrushes` (zip com manifesto: id, autor, licença SPDX ou texto, fonte, versão, datas, tags, hashes);
  - exportar e importar;
  - campos de autor e licença no criador;
  - biblioteca em `Documentos\AnimeIAI\Pincéis` com índice reconstruível (formatos B1–B7). Começa depois de o endurecimento do salvamento liberar `brushes.rs`.
- **Animação:**
  - Mesa de luz / papel cebola;
  - marcas K/B/I e seletor Genga/Douga;
  - linha do tempo compacta;
  - painéis Cena (roteiro e storyboard) e Referências do personagem (workspaces 1.2–1.7).
- **Transformação:** opção "Manter proporção" (o Photoshop atual é proporcional por padrão; o pedido do usuário, livre por padrão, fica como padrão aqui) e modos inclinar/distorcer/perspectiva, que o motor já tem (nucleo).
- **Seleção que limita pincel, borracha, balde e gradiente** (nucleo, alto impacto).

## Onda 3

- **Formato de projeto em arquivo único:**
  - `.aiaiproj` em SQLite: spike com o corpus ultima-rosa primeiro;
  - `ProjectStore` (pasta legada + SQLite);
  - conversão sem apagar a pasta;
  - recuperação de queda e snapshots (formatos P1).
- **3D:**
  - spike (renderer cel do ANIGO offscreen: ms por quadro, VRAM em 4 GB, alfa, tamanho do target);
  - em seguida a camada 3D: importar glb/gltf/obj/vrm, gizmo, opacidade e câmera (3d.md fases 0–5).
- **Layout:** réguas de perspectiva editáveis (1/2/3 pontos e outras) e Guia 3D (workspaces 2.1–2.4).
- **Personagens:** pasta do personagem com folha, expressões, boca, olhos, figurinos e modelo 3D (2.5–2.6).

## Onda 4

- **Biblioteca** (materiais, modelos 3D, paletas, texturas) e **Modelagem v1** (3d.md fases 7 e 12; workspaces 2.7, 2.13).
- **Composição estilo After Effects:** viewer, linha do tempo da composição, Efeitos e predefinições, Controles de efeito (3.1–3.7).
- **Edição estilo Premiere:** EditDoc v2 com trilhas V/A/S separadas, transições, filtros, legendas, monitor de origem (3.13–3.18).
- **Áudio nível Audacity:**
  - forma de onda, espectrograma, efeitos, multipista, medidores;
  - painel Voz sintética com detecção de GPU e instalação opcional do modelo de alta qualidade, deixado como plano (3.8–3.12, 4.13–4.14).
- **Produção:** visões, gráficos, gargalos, IA opcional. **Entrega:** fila, destinos, entregáveis, verificação (4.1–4.12).

## Decisões em aberto (perguntar ao usuário quando chegar a hora)

1. Storyboard com canvas próprio (recomendado).
2. Biblioteca e Modelagem na ordem pedida ou num grupo "Recursos" no fim.
3. IndexTTS: licença sem uso comercial declarado; Style-Bert-VITS2 é AGPL (sidecar HTTP).
4. Aceitar `.vrm` de terceiros (cada modelo tem a sua licença; o app mostra e respeita o `meta`).
5. Prévia 3D por software primeiro ou já em wgpu (a pesquisa 3D recomenda o renderer do ANIGO offscreen depois do spike).
