# 04 — Usar com o Claude (MCP)

O `aiai-mcp.exe` é um servidor MCP (stdio) com cerca de 90 ferramentas `aiai_*`: desenhar curvas, pintar, preencher, animar, gerar falas, mixar, exportar etc. Ele trabalha de dois jeitos:

- **Sem o app aberto (headless):** abre o projeto direto da pasta e trabalha nele.
- **Com o app aberto:** fala com o app pela *bridge* local (127.0.0.1, com token) — você vê cada ação na tela e o histórico de desfazer é o mesmo do mouse.

## Configurar no Claude Code

Na raiz do repositório já existe um `.mcp.json`:

```json
{
  "mcpServers": {
    "animeiai": {
      "command": "C:\\AnimeIAI\\app\\target\\release\\aiai-mcp.exe",
      "env": { "AIAI_MCP_ALLOWED_DIRS": "C:\\AnimeIAI\\producoes;C:\\AnimeIAI\\calibracao;C:\\AnimeIAI\\sprints\\registro\\evidencias" }
    }
  }
}
```

Para outro cliente MCP (Claude Desktop etc.), use o mesmo comando e as mesmas variáveis.

## Ligar a bridge (app aberto)

Abra o app com `--bridge` (ou com `AIAI_BRIDGE=1`). Ele grava o endereço e o token em `%LOCALAPPDATA%\AnimeIAI\bridge\bridge.json`, onde o servidor MCP os encontra sozinho. `aiai-mcp info` mostra se o app está respondendo. Variáveis: `AIAI_BRIDGE_ADDR` (porta fixa), `AIAI_BRIDGE_TOKEN`, `AIAI_BRIDGE_DISCOVERY` (outro caminho para o arquivo de descoberta).

## Segurança de arquivos (sandbox)

| Variável | Efeito |
|---|---|
| `AIAI_MCP_ALLOWED_DIRS` | pastas onde o MCP pode gravar (separadas por `;`) |
| `AIAI_MCP_READ_DIRS` | pastas de onde pode ler (importar imagem/áudio) |
| `AIAI_MCP_ALLOW_FS_WRITE=0` | desliga qualquer gravação em disco |

## Como a IA deve trabalhar

1. `aiai_status` — app aberto ou headless, projeto atual.
2. `aiai_query` — ler o documento (cortes, camadas, paletas…).
3. Mudar com as ferramentas curadas (`aiai_draw_curves`, `aiai_fill`, `aiai_paint`, `aiai_transform`…) ou com `aiai_apply` (comando bruto; `aiai_schema {"which":"commands"}` lista todos).
4. Conferir com `aiai_render` (olhar a imagem) ou `aiai_lint` (buracos e cores fora da paleta).

## Linha de comando

Fora de um cliente MCP, o mesmo executável chama as ações da bridge pelo nome em maiúsculas (não pelo nome `aiai_*` da ferramenta):

```
aiai-mcp info
aiai-mcp call PING
aiai-mcp call QUERY @consulta.json --project producoes\exemplo.aiai
aiai-mcp call RENDER @render.json --png saida.png --project producoes\exemplo.aiai
```

`aiai-mcp call` sem ação lista todas. As ações de documento (QUERY, APPLY_COMMAND, RENDER, DRAW_CURVES, FILL, EXPORT_VIDEO…) rodam headless com `--project` (o projeto é salvo após cada mudança); GET_STATUS, SCREENSHOT, INPUT, GET_UI_STATE, UI_ACTION e GET_METRICS precisam do app aberto com `--bridge`. O argumento é um JSON em linha ou `@arquivo.json`. O resultado sai em JSON numa linha no stdout; erros vão para o stderr (saída 1; 2 = uso incorreto). `--png` grava a imagem devolvida no caminho dado (dentro da sandbox).
