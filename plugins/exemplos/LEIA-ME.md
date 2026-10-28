# Plugins de exemplo (`.aiaiplugin`)

Filtros em WGSL que o AnimeIAI roda na GPU (Filtro > Plugins). Para usar, copie as pastas `.aiaiplugin` para
`%APPDATA%\AnimeIAI\plugins` (ou use Filtro > Plugins > Instalar plugin com um `.aiaiplugin` em zip).

- `posterizar` — cor chapada de cel (poucos níveis por canal)
- `contorno` — linha escura onde a cor muda (Sobel)
- `reticula` — pontos de retícula de mangá

O formato (manifesto e as ligações do shader) está em `app/crates/aiai-tools/src/plugin.rs`.
O shader só recebe a imagem e os parâmetros: não lê arquivos, não acessa rede nem o resto do programa.
