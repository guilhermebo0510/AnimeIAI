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
20. **Régua de FLIP relativa ao piso de ruído da referência** (2026-10-16, S04). Medido: FLIP médio entre dois quadros *idênticos* do mesmo desenho (compressão HEVC) = 0,0146–0,0170 no corte A (#138) e 0,0047 no corte D (#218); um deslocamento de 1 px dá 0,059. Exigir FLIP ≤ 0,05 absoluto deixaria ~0,03 de margem real e reprovaria diferenças invisíveis. Nova regra: **FLIP médio ≤ piso do corte + 0,05** e **máximo por quadro ≤ piso + 0,10**, com o piso medido em cada corte (par de quadros dentro de uma mesma exposição) e registrado no relatório. SSIM ≥ 0,95, F1 de bordas ≥ 0,95, ΔE plano ≤ 5, paleta ≤ 3 e timing exato **não mudam**. Não é afrouxar o alvo visual: é descontar o ruído que nem a própria referência passa.
21. **Espessura de linha**: a mediana por transformada de distância é discreta em linhas de ~2 px (valores 1 / 1,83 / 3), então ±5% é inatingível por resolução da medida. Regra: média dentro de ±5% **ou** diferença absoluta ≤ 0,3 px, o que for mais folgado, até existir um estimador subpixel confiável (pendência registrada para S21).
22. **Medição assistida no N1** (2026-10-16). Fato medido na S04: 1 px de deslocamento já reprova FLIP/SSIM, e a leitura visual de coordenadas pelo agente tem erro de vários px num quadro 1920×1080. Um decalcador humano na mesa de luz vê exatamente onde a linha está; o equivalente para o agente é uma **ferramenta de medição**: dado um traço *que o agente estimou e vai desenhar*, ela informa onde fica o centro e a largura da linha da referência perto de cada ponto (busca ao longo da normal, ±N px) e a cor mediana numa região. O agente continua decidindo existência, topologia, ordem, ponta, largura e cor de cada traço e emite o comando; a ferramenta não cria traços nem extrai contornos sozinha. **Continua proibido**: vetorização automática, detecção de contornos para gerar traços, cópia/recorte de pixels da referência. Vale só no N1; N2 e N3 não têm referência.
