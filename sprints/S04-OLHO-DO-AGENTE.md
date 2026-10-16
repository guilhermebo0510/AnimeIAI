# S04 — O "olho" do agente: referência, mesa de luz, métricas, diff

## Objetivo
Antes das ferramentas de desenho, dar ao agente os instrumentos para medir o que desenha: importar e medir referência, sobrepor grade de coordenadas, comparar com métricas do protocolo, gerar relatório lado a lado. Também preparar o catálogo de cortes do episódio de referência.

## Tarefas
- [ ] `aiai-metrics` (Rust): FLIP (crate `nv-flip` ou implementação própria do paper), SSIM (gaussiano 11 px, luma), ΔE2000 (Lab D65) com máscara, mapa de bordas (Canny/XDoG ajustado para linha de anime) e F1 com tolerância, distribuição de espessura de linha (transformada de distância sobre o esqueleto), histograma e paleta (k-means em Lab), erro de posição de câmera (correlação de fase).
- [ ] Ação COMPARE na bridge e `aiai_compare` no MCP: entrada = render do app (quadro/camadas) ou PNG, e PNG de referência; saída = números + mapa de diferença FLIP + overlay de bordas (verde = só ref, vermelho = só app, amarelo = ambos) + recortes ampliados das piores regiões.
- [ ] Camada de referência: importar imagem/quadro, opacidade, ajuste de posição/escala, modo "mesa de luz" (tinta azul/vermelha), nunca entra no render final (teste garante).
- [ ] Grade de coordenadas: overlay opcional no render enviado ao agente (linhas a cada 50/100 px com rótulos), para ler posições com precisão.
- [ ] Conta-gotas que devolve cor em sRGB/Lab e média de região; ferramenta de medida (distância, ângulo, proporção em "cabeças").
- [ ] `tools/ref/`: extrair quadros e áudio do .mkv para `calibracao/ref/` (FFmpeg), detecção de cortes, mapa de exposição por corte, folha de contato por corte, catálogo `cortes.json`.
- [ ] `tools/metrics/report.py`: relatório HTML/PNG lado a lado (referência | app | diferença | números), reaproveitando `C:\ANIGO\scripts\look_calibration`.
- [ ] Validar as métricas contra casos conhecidos (imagem idêntica = perfeito; deslocamento de 1 px; mudança de cor de ΔE conhecido; linha mais grossa em 10%).

## Testes
- Unitários das métricas com valores de referência (FLIP contra a implementação oficial em imagens de exemplo; ΔE2000 contra a tabela de Sharma et al.).
- Integração: COMPARE pelo MCP com dois PNGs do catálogo.

## Critério de pronto
- [ ] Catálogo de cortes do episódio gerado (quantidade de cortes, durações) e folha de contato em `calibracao/` (fora do git) com resumo em `registro/evidencias/S04/catalogo-resumo.md`.
- [ ] Relatório de comparação de exemplo (um quadro da referência contra ele mesmo deslocado 2 px) mostrando que as métricas acusam o esperado.
- [ ] Ficha preliminar dos 5 cortes candidatos do protocolo.
