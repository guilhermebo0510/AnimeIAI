# S19 — Produção e revisão: tracker de cortes, correção, retakes, proveniência

## Objetivo
Gerenciar a produção local como um estúdio pequeno: tabela de cortes com estado por etapa, camada vermelha de correção, retakes comparáveis e log de proveniência. Para o agente, é o painel que mostra o que falta fazer e onde a régua falhou.

## Tarefas
- [ ] Tabela de cortes (カット表): número, duração, personagens, etapas (LO, genga, douga, shiage, BG, satsuei, cor, som) com estado (não iniciado, em andamento, para revisão, aprovado, retake), responsável (agente/humano), notas; filtros e ordenação; miniaturas.
- [ ] Atualização automática de estado a partir do conteúdo (ex.: lint de shiage = 0 → shiage pronto para revisão).
- [ ] Camada de correção (vermelha, não renderiza), com anotações de texto e desenho; lista de correções por corte; marcar como resolvida.
- [ ] Retake: snapshot da versão aprovada/anterior; comparar lado a lado e com diferença (usa S04); histórico de retakes.
- [ ] Proveniência: painel por cel/camada mostrando autor e comandos de origem (a partir do log do núcleo).
- [ ] Relatório de produção: progresso por etapa, cortes bloqueados, lacunas de ferramenta apontadas.
- [ ] MCP: `aiai_cut_status`, `aiai_correction`, `aiai_retake`, `aiai_provenance`, `aiai_production_report`.

## Critério de pronto
- [ ] Prova de uso: nos cortes de teste, o agente marca etapas, deixa duas correções, resolve uma com retake e gera o relatório de produção; capturas em evidências.
