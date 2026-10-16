# Registro de decisões

Formato: data · decisão · por quê · descartado · adiado · quem decidiu.

## 2026-10-15 — planejamento (conversa com o usuário)

1. **Sem IA generativa de imagem/vídeo.** Todo desenho é feito pelo Claude com as ferramentas do app via MCP (modelo "Blender com MCP"). Usuário.
2. **Áudio local e gratuito**, qualidade mediana aceitável; vozes originais; sem APIs pagas por enquanto. Usuário.
3. **Stack: Rust nativo com egui + wgpu** (como o ANIGO) em vez de Tauri + WebView2. Motivo: ANIGO já migrou, problemas de airspace/entrada/captura no Smart Canvas, testabilidade por agente, uma linguagem só num PC com 8 GB. Descartado: Tauri para este produto. Adiado: CEF em textura, Xilem/Slint. Proposto pelo Claude, confirmado pelo usuário.
4. **Reaproveitar código do ANIGO e do Smart Canvas** livremente; código de terceiros só lido e reimplementado, salvo licença que permita. Usuário.
5. **AnimeIAI separado do ANIGO por enquanto**, com possibilidade de união futura (o formato não deve fechar essa porta). Usuário.
6. **Guias 3D** (manequim, blocos de cenário, câmera) com traço final por cima, e **intercalação vetorial automática** (estilo CACANi) são permitidos. Usuário.
7. **Vetor e raster** como no Clip Studio; pintura com o Photoshop como referência; **grades de referência espacial e angular**; **texturas e pincéis especiais** para cenário. Usuário.
8. **Micro anime com vozes em japonês e legenda em português.** Usuário.
9. **Windows apenas; interface em PT e EN; modo claro e escuro; ícones e logomarca originais e profissionais.** Usuário.
10. **Git local sem remoto** por enquanto. Usuário.
11. **Régua dos 5%** definida pelo Claude (FLIP, SSIM, ΔE2000, F1 de bordas, espessura, timing; áudio por timing/loudness, sem comparar timbre). Ver `07-PROTOCOLO-DE-VALIDACAO.md`. Usuário delegou.
12. **Níveis de calibração N1 (mesa de luz sem conversão automática), N2 (sem referência), N3 (original).** Claude.
13. **Vozes: VOICEVOX (JA) e Kokoro ONNX (PT/EN), sem PyTorch.** Claude.
14. **Retomada automática** por tarefa agendada local + batimento em `sprints/.heartbeat`; keep-awake durante a rodada. Autorizado pelo usuário.

## 2026-10-15 — execução

15. **Render CPU como fonte única da verdade** (S03). O compositor e a rasterização rodam na CPU (Rust + rayon), determinísticos; a GPU exibe a imagem e acelera efeitos pesados nas sprints de satsuei, sempre com equivalente CPU para comparação. Motivo: elimina divergência GPU×CPU nas métricas do agente, simplifica golden tests e o "olho" do agente; desempenho de playback vem do cache de quadros. Adiado (não descartado): compositor GPU completo, se o playback com cache não bastar. Claude.
16. **Mistura em sRGB codificado, pré-multiplicado** (como AE/Photoshop no modo padrão), não em linear. Motivo: os quadros de referência de anime são compostos assim; multiplicar/tela/sobrepor batem com a referência. Linear fica como opção por efeito (glow, desfoque) na S13. Claude.
17. **Traço vetorial rasterizado por campo de distância** (união de cápsulas desiguais entre amostras da curva), não por offset de curva + tesselação nem Vello. Motivo: junções e pontas corretas por construção, largura variável exata, determinismo, código pequeno e testável; Vello/tesselação ficam adiados para o caso de precisar de GPU para milhares de traços. Claude.
18. **Tiles raster em blob RGBA16 + LZ4; máscaras de preenchimento em blob 8 bits + LZ4.** Claude.
19. **Ordem das camadas: filhos de baixo para cima** (o índice 0 é a camada mais de baixo); a UI mostra invertido. Claude.
