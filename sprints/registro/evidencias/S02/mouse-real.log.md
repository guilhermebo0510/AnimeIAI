# S02 - roteiro de mouse/teclado reais (SendInput)

Gerado por `mouse_real.py`; vigia: `mouse-real-watch.json`.

- `input.ps1 -S "foco"` -> exit 0: [ok] 1 operacoes 
Estado inicial: workspace=pintura, janela=[1920.0, 991.02], origem cliente=[0.0, 29.0], titulo='AnimeIAI - S02 ao vivo'
- `input.ps1 -S "foco; click 800 88; wait 350"` -> exit 0: [ok] 3 operacoes 
- OK  clique real na aba 'cenario' em (800,88) -> workspace 'cenario'
- `input.ps1 -S "cap C:\AnimeIAI\sprints\registro\evidencias\S02\mouse-real-2-cenario.png"` -> exit 0: [ok] 1 operacoes 
- `input.ps1 -S "foco; click 572 88; wait 350"` -> exit 0: [ok] 3 operacoes 
- OK  clique real na aba 'animacao' em (572,88) -> workspace 'animacao'
- `input.ps1 -S "foco; click 60 88; wait 350"` -> exit 0: [ok] 3 operacoes 
- OK  clique real na aba 'roteiro' em (60,88) -> workspace 'roteiro'
- `input.ps1 -S "foco; click 690 88; wait 350"` -> exit 0: [ok] 3 operacoes 
- OK  clique real na aba 'pintura' em (690,88) -> workspace 'pintura'
- `input.ps1 -S "foco; click 77 46; wait 400"` -> exit 0: [ok] 3 operacoes 
- OK  menu Arquivo aberto por clique real; itens: ['menu_item:file.new', 'menu_item:file.open', 'menu_item:file.quit', 'menu_item:file.save', 'menu_item:file.save_as']
- `input.ps1 -S "cap C:\AnimeIAI\sprints\registro\evidencias\S02\mouse-real-3-menu-arquivo.png"` -> exit 0: [ok] 1 operacoes 
- `input.ps1 -S "key esc; wait 300"` -> exit 0: [ok] 2 operacoes 
- OK  Esc fecha o menu Arquivo
- OK  ha o que desfazer
- `input.ps1 -S "foco; click 142 46; wait 400"` -> exit 0: [ok] 3 operacoes 
- `input.ps1 -S "click 207 76; wait 600"` -> exit 0: [ok] 2 operacoes 
- OK  Editar > Desfazer por clique real: undo_steps 17 -> 16, can_redo=True
- `input.ps1 -S "cap C:\AnimeIAI\sprints\registro\evidencias\S02\mouse-real-4-apos-desfazer.png"` -> exit 0: [ok] 1 operacoes 
- `input.ps1 -S "foco; key ctrl+shift+z; wait 400"` -> exit 0: [ok] 3 operacoes 
- OK  Ctrl+Shift+Z real refez (undo_steps=17)
- `input.ps1 -S "key ctrl+z; wait 400"` -> exit 0: [ok] 2 operacoes 
- OK  Ctrl+Z real desfez
- `input.ps1 -S "key ctrl+y; wait 400"` -> exit 0: [ok] 2 operacoes 
- OK  Ctrl+Y real refez
- `input.ps1 -S "cap C:\AnimeIAI\sprints\registro\evidencias\S02\mouse-real-5-final.png"` -> exit 0: [ok] 1 operacoes 
- `input.ps1 -S "foco; click 35 243; wait 300"` -> exit 0: [ok] 3 operacoes 
- OK  clique real na ferramenta Caneta vetorial
