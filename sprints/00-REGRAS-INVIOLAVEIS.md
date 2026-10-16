# Regras invioláveis

1. **Sem IA generativa de imagem ou vídeo.** Nada de Stable Diffusion, Flux, modelos de vídeo, img2img, inpainting generativo. Todo pixel final sai das ferramentas do programa (traço vetorial, pincel raster, balde, textura procedural ou desenhada, efeito, partícula, render 3D de guia) operadas pelo Claude via MCP ou por mouse, ou por um humano no retoque.
   - Permitido: algoritmos clássicos (interpolação vetorial entre desenhos-chave, fechamento de gap, vetorização de um rascunho **feito no próprio app**, ruído procedural, render 3D de blocos/manequim como guia).
   - Proibido nas cenas de calibração: vetorizar ou recortar automaticamente o frame de referência para virar arte final. O frame de referência é só para olhar e medir (ver `07-PROTOCOLO-DE-VALIDACAO.md`).
2. **Áudio só local e gratuito.** Vozes sintéticas originais (VOICEVOX/Kokoro ou equivalente). Nunca clonar voz de dublador ou pessoa real. Créditos de voz exigidos pela licença do motor vão nos créditos do vídeo.
3. **O episódio de referência não vira produto.** Frames e áudio extraídos ficam em `calibracao/`, fora do git, e nunca entram em `producoes/` nem em assets do app.
4. **Sem cópia de código de terceiros** fora do que a licença permite. Código de `C:\ANIGO` e `C:\smart-canvas-arquitetura` pode ser copiado. GPL (Krita, Blender, Natron, Synfig, OpenToonz é BSD mas respeitar) só para leitura e entendimento. Programas externos GPL podem ser chamados como processo separado, opcional, nunca linkados.
5. **Validação visual real.** Nenhuma ferramenta é dada como pronta sem ser usada de verdade no app (MCP e, se tiver UI interativa, mouse) e sem o executor olhar a imagem resultante. "Teste passa" não é "aparece na tela".
6. **Nunca mascarar erro.** Falha, regressão, limitação e meta não atingida aparecem no `00-ESTADO-ATUAL.md` e nos relatórios com a saída real.
7. **Não decidir pelo mais fácil.** O critério de arquitetura é "não virar limitador depois"; custo de implementação é consequência registrada, não fundamento. Separar opção descartada de opção adiada.
8. **Interface profissional.** Sem emoji na UI; ícones vetoriais originais; modo claro e escuro; português e inglês; nada de mockup (viewport é render real na GPU).
9. **Disco.** Nenhum build abaixo de 8 GB livres; nenhuma escrita em fonte abaixo de 3 GB. Ver lições.
10. **Sem mexer no sistema.** Plano de energia, segurança, registro do Windows e arquivos fora de `C:\AnimeIAI` (exceto caches de ferramentas e `%LOCALAPPDATA%\AnimeIAI`) ficam intocados.
