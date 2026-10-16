# ADR-002 — Modelo de dados e Command Bus

Status: aceito como base; campos detalhados na S01.

## 1. Hierarquia

```
Projeto (.aiai/)
├── Configuração: resolução (1920x1080), fps (24000/1001), espaço de cor (sRGB/Rec.709, trabalho em linear float16), idioma
├── Bíblia: logline, sinopse, temas, mundo
├── Assets compartilhados
│   ├── Personagens: model sheet, turnaround, expressões, settei de cor (ColorSet), perfil de voz, partes reutilizáveis
│   ├── Props, cenários reutilizáveis, texturas, presets de pincel, paletas, LUTs, efeitos salvos
│   └── Áudio: SFX, BGM, vozes geradas (com texto e parâmetros de origem)
├── Roteiro: cenas, ações, falas (personagem, texto, emoção, direção)
├── Storyboard / e-conte: painéis ligados a cortes (desenho, ação, fala/SE, duração)
└── Episódio
    └── Cena
        └── Corte (Cut)  <- unidade de trabalho
            ├── Metadados: número, duração em quadros, estado por etapa, retakes, notas
            ├── Vínculos: falas do roteiro, painel do storyboard, áudio
            ├── Timesheet: trilhas (colunas) x quadros; cada célula aponta para um Cel ou "mantém"
            ├── Árvore de camadas
            │   ├── Pasta/grupo
            │   ├── Vetorial (strokes com perfil de largura, cor por índice de paleta)
            │   ├── Raster (tiles)
            │   ├── Preenchimento (regiões por índice de paleta, "paint behind")
            │   ├── Texto/balão (storyboard, créditos, legendas)
            │   ├── Guia (régua, grade, perspectiva, 3D de guia; não sai no render final)
            │   ├── Referência (imagem importada, mesa de luz; nunca sai no render final)
            │   ├── Ajuste (níveis, curvas, HSL, LUT) e efeito (nó de satsuei)
            │   └── Partículas, câmera
            │   (cada camada: opacidade, mistura, máscara, clipping, trava, visibilidade, papel LO/genga/douga/shiage/BG/FX)
            ├── Cels: desenho de uma camada animável num quadro (key/breakdown/inbetween)
            ├── Câmera: transformações animadas, multiplano (profundidade por camada), shake
            └── Grafo de composição: ordem, efeitos, passes
    └── Edição: sequência de cortes renderizados + áudio + transições + legendas
```

## 2. Identidade e conteúdo

- Todo objeto tem `id` (UUID v7) e `revision`.
- Conteúdo pesado (tiles raster, áudio, imagens importadas) em `blobs/<blake3>.bin`, referenciado por hash. Strokes vetoriais no JSON (pequenos e diffáveis) até um limite; acima dele, blob.
- `project.json` canônico: chaves ordenadas, números com precisão fixa, sem campos voláteis.
- WAL de recuperação: cada comando aplicado vai para `wal.ndjson`; ao salvar, compacta.

## 3. Command Bus

- Todo efeito no documento é um `Command` serializável (`{"kind": "..."}`), validado e aplicado por `CoreSession::apply`, que devolve `Outcome` (ids criados, diff resumido, revisão nova).
- `batch` = um passo atômico de undo.
- Traço tem forma completa (MCP e replay) e incremental (UI), que fecha num único comando no histórico.
- Undo/redo por inverso gerado no apply; o log da sessão não perde entradas com undo.
- Relógio e RNG injetáveis (partículas, jitter de pincel, ruído determinísticos).
- Consultas separadas de comandos: ler documento, camadas, timesheet, cores, métricas, render.

## 4. Timesheet

- Trilhas: A, B, C… (personagens/partes), BG, BOOK (sobreposições de cenário), câmera, efeitos, diálogo, boca (lip-sync).
- Célula = referência a um Cel por número ("A3") ou vazio/mantém. Exposição 1s/2s/3s é repetição de referência.
- Quadro **0-based internamente**, exibição 1-based como no papel (armadilha do XDTS).
- Exportação XDTS desejável (S18); OTIO/EDL para a edição.

## 5. Proveniência

Cada cel/camada registra autor (`agent:claude`, `human`, `tool:<nome>` para operações automáticas como intercalação) e o comando de origem.

## 6. Em aberto (decidir na S01 com protótipo)

- Granularidade do undo de pintura raster (por traço, guardando os tiles afetados).
- Limite do JSON para vetores grandes.
- Cache de frames renderizados (RAM + disco) e invalidação por revisão.
- Preparação para união com o ANIGO: cenas 3D referenciadas como asset externo (`.anigo`) renderizado em camada.
