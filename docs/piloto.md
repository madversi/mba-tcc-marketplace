# Piloto e congelamento — 2026-09-22/23

Commit de partida: `4d58f42` (chore: remove timestamps), árvore limpa.

## Fase -1 — estado zerado

| Horário | Ação |
|---|---|
| 23:04:19 | `docker compose -f docker\docker-compose.yml --profile grafana --profile load down -v --remove-orphans` |
| 23:04:28 | Removidos os contêineres grafana, cadvisor, prometheus, inventory, orders, payments, toxiproxy, rabbitmq, catalog e postgres, a rede `marketplace_default` e os volumes `marketplace_prometheus-data`, `marketplace_postgres-data`, `marketplace_grafana-data` e `marketplace_rabbitmq-data`. `docker volume ls --filter name=marketplace` e `docker ps -a --filter name=marketplace` vieram vazios; nenhuma remoção manual foi necessária. |
| 23:04:33 | `load-tests\results` (58 arquivos, 15.711.891 bytes: resumos avulsos `baseline-01.json`, `gateway-indisponivel-01.json`, `spike-01.json`, `stress-01.json`, `stress-02.json` e `experimentos\3A\R-ON\` com `rep-01`, `rep-01.abortada-20260917-014602` e `rep-02`) movida para `load-tests\results_antigos_20260922`. `load-tests\results` recriada vazia. Nada foi apagado. |
| 23:04:50 | Acrescentadas a `.git\info\exclude` (arquivo local, não versionado) as linhas `load-tests/results_antigos_*/`, `docs/piloto.md` e `docs/coleta.md`. Motivo: o script grava em `metadata.json` o campo `git.alteracoes_nao_commitadas` a partir de `git status --porcelain`; sem essa exclusão, arquivos de registro não relacionados ao código fariam o campo ser diferente de zero. |
| 23:04:58 | Primeira tentativa de build falhou: `docker-credential-desktop` não estava no PATH. O binário fica em `%LOCALAPPDATA%\Programs\DockerDesktop\resources\bin`, que foi acrescentado ao PATH da sessão. |
| 23:05:45 | `docker compose -f docker\docker-compose.yml build catalog orders inventory payments` (sem `--no-cache`) concluído. Imagens: catalog `4fd36161d478`, orders `219bd86fb368`, inventory `c287db5927bb`, payments `d21be861c56f`. |
| 23:05 | Espaço livre em E:: 674.753.052.672 bytes (~628 GiB). |

| ~23:30 | Usuário informou ter desativado a suspensão do Windows e pausado as atualizações automáticas. |

## Fase 0 — sanidade

- 23:06: pilha subiu com `up -d --wait`. `/health` respondeu 200 nas portas 8081 (catalog), 8082 (orders), 8083 (inventory) e 8084 (payments), com `database: up` em todos.
- Serviço `postgreSQL` nativo do Windows: **Running** (inicialização automática). Pará-lo exige administrador; foi repassado ao usuário. O usuário o parou; às 23:21:34 ele constava como **Stopped**. A parada ocorreu durante a execução piloto 1A taxa-060 rep 1 (carga das 23:17:17 às 23:21:19). As execuções piloto anteriores (1A taxa-020 rep 1 e a Fase 0) rodaram com ele ativo. Inicialização continua Automatic: volta se o Windows reiniciar.
- Execução curta: `.\load-tests\experimento.ps1 -Experimento 1A -Taxas 10 -Repeticoes 1 -Aquecimento 30s -Medicao 30s` (23:06:47–23:10:00; o script reconstruiu as imagens porque foi rodado sem `-SemBuild`). O script a classificou como **válida**.
- Saída movida para `load-tests\results\fase0\experimentos\` para não colidir com o piloto e a coleta oficial. Na primeira tentativa a pasta ficou presa pelo diretório de trabalho do shell; os 26 arquivos foram movidos e a árvore vazia que sobrou foi removida (0 arquivos, conferido antes de remover).

Verificação item a item (arquivos em `load-tests\results\fase0\experimentos\1A\taxa-010\rep-01\`):

| Item | Resultado | Evidência |
|---|---|---|
| a) bancos vazios | OK | O contêiner postgres iniciou às 03:08:08Z sobre volume novo. Após a execução: 1 vendedor e 20 produtos (o primeiro às 03:08:41,9Z, criado pelo `setup()` do k6), 20 linhas de estoque, 601 pedidos e 601 pagamentos, com o primeiro pedido às 03:08:42,1Z, depois de `inicio_utc` = 03:08:40,8Z. As 601 iterações do k6 (`summary.json`) batem com os 601 pedidos. |
| b) metadata.json | OK | Commit `4d58f42f748cc5002a2c5cfef7846fba09ae877d`, `alteracoes_nao_commitadas` = 0, imagem e digest dos 7 contêineres do sistema sob teste, IDs dos contêineres (incluindo prometheus, cadvisor e k6) e ambiente (Ryzen 5 3600, 6 núcleos/12 threads, 16 GB; Docker 29.7.2 com 12 CPUs e 7,7 GiB; kernel 6.18.33.2-microsoft-standard-WSL2; compose 5.5.1). |
| c) CPU e memória | OK | `cpu.json`: 13 séries, 10 delas mapeadas para os 7 contêineres do SUT, prometheus, cadvisor e k6; as 3 restantes não estão no mapa de contêineres e têm 1 ponto cada. `memoria.json`: 10 séries, todas mapeadas. |
| d) k6_cpu_maxima | OK, não nulo | 0,02216 núcleo. |
| e) drenagem | OK | `drenagem_completa` = true, 2,6 s. |
| f) invariantes | OK, todos 0 | I1: 0 linhas em `stock_reservations` e `reserved` = 0 nos 20 produtos. I2: Σ(1.000.000 − available − reserved) = 601 = pedidos CONFIRMED (cada item tem quantidade 1), diferença 0. I3: 0 pagamentos PENDING, todos APPROVED. I4: 0 pedidos CONFIRMED sem pagamento APPROVED. |
| g) reinícios/OOM | OK | `RestartCount` = 0 e `OOMKilled` = false nos 7 contêineres. |

Observações de estrutura dos dados, que valem para a análise:
- O `k6.csv.gz` tem carimbo de tempo com resolução de 1 s. A fase vem na coluna `extra_tags` (`phase=...`).
- `pedidos.csv` não traz `product_id`. Por isso o invariante I2 só pode ser verificado no agregado (soma sobre os produtos), e não produto a produto.

## Fase 1 — piloto

Execuções: `load-tests\results\piloto\experimentos\` (movidas de `results\experimentos` ao fim do piloto, 00:21). Todas com `-SemBuild` e commit `4d58f42`.
- 23:11:34–23:53:09: `experimento.ps1 -Experimento 1A -Taxas 10,20,40,60 -Repeticoes 2 -SemBuild`. Aquecimento de 1m e medição de 3m (padrões do script).
- 23:53:09–00:21:17: 2A em C0 e C4, 3A em R-ON e 4A (`parado-payments`), uma execução cada, com `-Taxa 20 -SemBuild` e os demais parâmetros no padrão (1m/1m/1m/3m, latência de 2000 ms, drenagem máxima de 600 s).
- **12 de 12 execuções válidas** pelo critério do script. `k6_cpu_maxima` ficou entre 0,022 e 0,131 núcleo em todas.
- A partir do 2A piloto, `git.alteracoes_nao_commitadas` = 1. A causa é a pasta nova e não versionada `analise/` (scripts de análise), que não contém código dos serviços. Para a coleta oficial, `analise/` foi acrescentada a `.git\info\exclude`; a exclusão será removida ao fim da coleta.

Extração: `analise\extrair.ps1 -Resultados load-tests\results\piloto\experimentos -Saida analise\piloto`, com saídas em `analise\piloto\`. Decisão de taxa: `analise\decisao_taxa.ps1 -Consolidado analise\piloto\consolidado\1A.csv -Saida analise\piloto\decisao_taxa.csv`.

### 1A — fase de medição (valores de `analise\piloto\consolidado\1A.csv`)

| Taxa | Rep | Vazão efetiva | p95 POST (ms) | Erro % | Iter. descartadas | Sagas concluídas % | Fila: média das 3 primeiras | Fila: média das 3 últimas |
|---|---|---|---|---|---|---|---|---|
| 10 | 1 | 10,00 | 8,72 | 0 | 0 | 100 | 3,00 | 2,00 |
| 10 | 2 | 9,99 | 8,25 | 0 | 0 | 100 | 3,00 | 2,67 |
| 20 | 1 | 19,98 | 73,95 | 0 | 3 | 100 | 2,33 | 2,00 |
| 20 | 2 | 20,00 | 16,75 | 0 | 0 | 100 | 0,33 | 3,00 |
| 40 | 1 | 39,84 | 122,69 | 0 | 28 | 100 | 3,00 | 2,00 |
| 40 | 2 | 40,00 | 14,37 | 0 | 0 | 100 | 2,67 | 3,33 |
| 60 | 1 | 60,00 | 14,67 | 0 | 0 | 100 | 599,00 | 1284,33 |
| 60 | 2 | 59,99 | 19,17 | 0 | 0 | 100 | 948,67 | 1434,33 |

"Sagas concluídas" = pedidos criados na fase de medição que estavam em CONFIRMED ou CANCELLED na exportação feita após a drenagem. Fila = soma de prontas e não confirmadas em todas as filas, exceto `*.dead`, em amostras de 5 s do Prometheus.

## Valores escolhidos

| Parâmetro | Valor | Regra | Justificativa |
|---|---|---|---|
| Taxa máxima sustentável | **10 req/s** | Regra de decisão 1 | Os critérios de p95, erro e sagas passam em todos os níveis e repetições. O de fila passa nas duas repetições só em 10 req/s: falha em 20 (rep 2: 0,33 → 3,00), em 40 (rep 2: 2,67 → 3,33) e em 60 (as duas repetições). Agregação adotada: um nível atende quando **todas** as repetições válidas atendem. Essa regra foi codificada em `decisao_taxa.ps1` antes de os dados do piloto existirem. |
| Taxa nominal (demais experimentos) | **10 req/s** | Regra de decisão 2 | 50% de 10 = 5, abaixo da grade 10/20/40/60; usado o menor nível da grade, 10 req/s. Registrado conforme a regra. |
| Durações | Aquecimento 1m, Antes 1m, Janela 1m, Recuperação 3m, Medição 3m, DrenagemMaxSegundos 600 | Regra 3 | Valores fixados pela regra. |
| Latência injetada no catálogo | **2000 ms** | Regra 4 | A sanidade do 2A em C0 teve 0 respostas 504 em todas as fases (`erro_504` = 0), então não houve redução. |
| Tempo de recuperação | K = 3 janelas de 5 s com erro ≤ 1% e p95 ≤ 1,2 × p95 da fase `antes` | Regra 5 | Implementado em `extrair.ps1` (parâmetros `-RecuperacaoK 3 -RecuperacaoErroMax 0.01 -RecuperacaoFatorP95 1.2`). Janelas alinhadas ao início da falha; o tempo é contado do fim da falha até o início da primeira janela da sequência. |
| Margem de relevância prática | 10 ms no p95; 0,05 núcleo na CPU média | Regra 6 | Fixada pela regra; usada só na leitura posterior. |
| AMQP_CONSUMER_CONCURRENCY | 1 (controle) | Regra 7 | Padrão do compose, não alterado. |
| Limite de iterações descartadas | 1% | Regra 8 | Padrão do script (`-LimiteDescarte 0.01`). |

**Sensibilidade da decisão de taxa, registrada para transparência.** O resultado depende de como as repetições são agregadas, e o critério que decide é o de fila, em diferenças de 1 a 3 mensagens:
- Com a mediana entre repetições em vez de "todas as repetições", 10 e 40 atenderiam e 20 não. A máxima seria 40 e a nominal 20 req/s.
- Com "ao menos uma repetição", a máxima seria 40 e a nominal 20.
- Adotou-se a regra definida antes dos dados.

## O que foi ajustado durante o piloto

Nenhuma alteração em código dos serviços, compose, scripts de carga ou `experimento.ps1`. Os ajustes foram só nos scripts de análise (`analise\`), antes do congelamento:
1. `.ps1` regravados em UTF-8 com BOM. Sem o BOM, o Windows PowerShell 5.1 lia o texto como ANSI e não encontrava as mensagens de log acentuadas (`transição do circuit breaker`), de modo que as transições saíam zeradas.
2. Nome do breaker do gateway corrigido de `gateway` para `payment_gateway`, o nome real em log e em métrica.
3. Caminhos de saída resolvidos como absolutos (o .NET usava outro diretório corrente).

## Evidências das verificações (arquivos brutos em `load-tests\results\piloto\experimentos\`)

- **Circuit breaker, 2A C4** (`2A\C4\rep-01\logs\servicos.log`, contêiner `marketplace-orders`, breaker `catalog`). A falha foi de 04:02:54,590Z a 04:03:54,587Z (`metadata.json`). Transições: open 04:02:58,095; half_open 04:03:11,390; open 04:03:14,696; half_open 04:03:27,989; open 04:03:31,292; half_open 04:03:44,588; open 04:03:47,894; half_open 04:04:01,186; closed 04:04:01,189. Primeira abertura 3,50 s após o início da injeção; 9 transições (4 aberturas); 53,2 s em estado aberto (46,6 s na fase `falha` e 6,6 s na `depois`). **O breaker abriu dentro da janela de falha; o fechamento definitivo ocorreu às 04:04:01,189, 6,6 s depois do fim da janela.** O Prometheus registra 1060 rejeições do breaker `catalog` na execução.
- **Fallback, 2A C4.** Houve 1332 ativações (`fallback_activations_total`) e 1332 linhas `usando produto do cache de fallback` no log, 1165 delas na fase `falha`. Todas as 1200 requisições da fase `falha` responderam 201 (0 respostas 503).
  - **503 sem cache: não há evidência no piloto.** Em C4 todos os 20 produtos entram no cache durante o aquecimento, e nenhuma resposta 503 ocorreu em nenhuma execução do piloto. As condições que exercitariam 503 sem fallback (C3 no 2A, 2B) não estavam no piloto.
- **Reprocessamento, 3A R-ON.** 1202 pagamentos enviados a reprocessamento e 1202 reprocessados (log `pagamento reprocessado`), 0 esgotados. Tempo de reprocessamento (`updated_at − created_at` do pagamento): mediana 35,7 s, p95 62,5 s, **máximo 71,2 s, dentro do prazo de 300 s**. 0 pagamentos PENDING residuais. Breaker `payment_gateway`: abriu 0,12 s após a injeção, 13 transições, 60,0 s aberto, 9014 rejeições. Reentregas: 6504 na fase `falha` e 1318 na `depois`; 0 mensagens mortas.
- **Coerência da falha medida no cliente:**
  - 2A C0 (latência de 2000 ms no catálogo): p50 do POST /orders na fase `falha` = 2007,5 ms, contra 7,0 ms na fase `antes`, com 0% de erro.
  - 2A C4: p50 de 6,8 ms, p95 de 3316 ms e 0% de erro na `falha`; fator de amplificação 0,71 na `falha`.
  - 3A: o POST /orders não passa pelo gateway (p95 de 8,0 ms na `falha`); o efeito aparece nos pagamentos, como descrito acima.
  - 4A: payments parado por 60,1 s. A fila subiu de 1 para 1181 mensagens em janelas de 5 s, com 0 sagas concluídas entre t = 5 s e t = 55 s. Após o retorno, o contêiner ficou saudável em 5,4 s e a fila foi drenada. Invariantes zerados.
- **Invariantes nas execuções sem falha (1A, 8 execuções): I1 = I2 = I3 = I4 = 0 em todas.** Também deram zero nas quatro execuções com falha.

## Fase 2 — congelamento (2026-09-23)

| Horário | Verificação | Resultado |
|---|---|---|
| 00:24 | Python 3.12.10 instalado (winget, escopo do usuário, autorizado pelo usuário) com pandas 3.0.6, numpy 2.5.3, scipy 1.18.1 e matplotlib 3.11.2 | Só no host; nada muda no repositório |
| 00:28:12 | `cargo fmt --all -- --check` | exit 0 |
| 00:28:19–00:28:43 | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| 00:28:50–00:29:50 | `cargo test --workspace` (PostgreSQL e RabbitMQ do compose no ar) | exit 0; 167 testes aprovados, 0 falhas |
| 00:29:57 | `git tag -a experimento-v1` (anotada, sem commit e sem push) | Aponta para **`4d58f42f748cc5002a2c5cfef7846fba09ae877d`**; `git status --porcelain` vazio |

> Ao fim da coleta (09:05), `analise/`, `docs/piloto.md` e `docs/coleta.md` foram retirados de `.git\info\exclude` e aparecem como não versionados no `git status`. `load-tests/results_antigos_*/` continua excluído.
