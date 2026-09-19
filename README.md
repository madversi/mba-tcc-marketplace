# Marketplace em Rust — Microsserviços com Resiliência e Mensageria

Projeto de TCC (MBA em Engenharia de Software) — implementação de um back-end de
marketplace fictício em Rust, dividido em microsserviços que se comunicam via
mensageria assíncrona (RabbitMQ), com padrões de resiliência (retry, timeout,
circuit breaker e fallback), voltado à coleta de métricas de desempenho.

**Aluno:** Helder Marcelo Adversi Junior
**Orientadora:** Elaine Barbosa de Figueiredo

Este README é o guia operacional: como subir o ambiente, rodar os cenários de
carga e coletar as métricas.

## Visão geral

| Serviço | Responsabilidade | Porta no host |
|---|---|---|
| `catalog` | Vendedores e produtos (REST) | 8081 |
| `orders` | Pedidos; valida produtos no `catalog` via HTTP | 8082 |
| `inventory` | Reserva, liberação e baixa de estoque | 8083 |
| `payments` | Cobrança em um gateway simulado com falhas injetáveis | 8084 |

| Infraestrutura | Endereço | Acesso |
|---|---|---|
| RabbitMQ (UI) | http://localhost:15672 | `helder` / `marketplace` |
| Toxiproxy (API) | http://localhost:8474 | — |
| Prometheus | http://localhost:9090 | — |
| Grafana (profile `grafana`) | http://localhost:3000 | `admin` / `admin` |
| cAdvisor | http://localhost:8085 | — |

O `orders` chama o `catalog` através do Toxiproxy (`http://toxiproxy:18080`), que
injeta latência, indisponibilidade ou resets de conexão sem alterar os serviços.

Fluxo de compra (saga coreografada): `POST /orders` grava o pedido `PENDING` e
publica `order.created` → `inventory` reserva o estoque (`stock.reserved`) →
`payments` cobra (`payment.approved`) → `orders` confirma e `inventory` dá baixa
na reserva. Falha de estoque ou de pagamento cancela o pedido e libera a reserva.
Gateway indisponível deixa o pedido `PAYMENT_PENDING` até o reprocessamento
resolver.

## Pré-requisitos

- Docker com Compose v2 (Docker Desktop no Windows).
- Rust 1.94+ apenas para rodar os testes ou os serviços fora do Docker.

Todos os comandos abaixo são executados na raiz do repositório.

## Subindo o ambiente

```bash
docker compose -f docker/docker-compose.yml up -d --build
docker compose -f docker/docker-compose.yml ps
```

O primeiro build compila o workspace inteiro e demora; os seguintes reaproveitam
a camada de dependências (`cargo-chef`). Os serviços aplicam as próprias
migrations ao subir. Para sobrescrever portas, credenciais e limites, copie
`docker/.env.example` para `docker/.env`.

Todos os containers têm limite de CPU e memória, e PostgreSQL, RabbitMQ e
Toxiproxy estão fixados por digest, para que as medições sejam comparáveis entre
execuções. O Grafana fica fora da subida padrão (não disputa recursos durante a
coleta); para inspecionar os painéis:
`docker compose -f docker/docker-compose.yml --profile grafana up -d grafana`.

Para derrubar mantendo os dados: `docker compose -f docker/docker-compose.yml down`.
Com `-v`, apaga também os volumes (bancos, filas e séries do Prometheus).

### Compra manual (verificação rápida)

```bash
curl -X POST localhost:8081/sellers -H "content-type: application/json" \
  -d '{"name":"Loja","email":"loja@teste.com"}'
curl -X POST localhost:8081/products -H "content-type: application/json" \
  -d '{"seller_id":"<SELLER_ID>","name":"Produto","price_cents":1000}'
curl -X PUT localhost:8083/stock/<PRODUCT_ID> -H "content-type: application/json" \
  -d '{"quantity":10}'
curl -X POST localhost:8082/orders -H "content-type: application/json" \
  -d '{"buyer_id":"<UUID_QUALQUER>","items":[{"product_id":"<PRODUCT_ID>","quantity":3}]}'
curl localhost:8082/orders/<ORDER_ID>          # CONFIRMED após alguns instantes
curl localhost:8084/payments/order/<ORDER_ID>  # APPROVED
```

No PowerShell use `curl.exe` e escape as aspas do JSON (`\"`).

## Configuração dos experimentos

Os serviços leem a configuração de variáveis de ambiente. As que não aparecem no
`docker/docker-compose.yml` podem ser adicionadas em `environment:` do serviço.

| Variável | Serviço | Padrão | Efeito |
|---|---|---|---|
| `CATALOG_TIMEOUT_ENABLED` | orders | `true` | Liga o timeout do cliente do catálogo |
| `CATALOG_RETRY_ENABLED` | orders | `true` | Liga o retry (desligado: uma única tentativa) |
| `CATALOG_BREAKER_ENABLED` | orders | `true` | Liga o circuit breaker do catálogo |
| `CATALOG_FALLBACK_ENABLED` | orders | `true` | Liga o cache de fallback do catálogo |
| `GATEWAY_BREAKER_ENABLED` | payments | `true` | Liga o circuit breaker do gateway |
| `PAYMENT_REPROCESS_ENABLED` | payments | `true` | Desligado: gateway fora falha o pagamento na hora, sem `payment.pending` |
| `AMQP_CONSUMER_CONCURRENCY` | todos | `1` | Mensagens processadas em paralelo por fila (o prefetch sobe junto) |
| `TOKIO_WORKER_THREADS` | todos | `2` no compose | Threads do runtime Tokio |
| `SERVICE_CPUS` / `SERVICE_MEMORY` | compose | `1.0` / `256m` | Limites dos 4 serviços Rust (há variáveis equivalentes para Postgres, RabbitMQ, Toxiproxy e k6) |
| `FAILURE_RATE` | payments | `0` | Chance (0–1) de o gateway ficar indisponível |
| `LATENCY_MS` | payments | `0` | Atraso artificial por cobrança |
| `UNAVAILABLE` | payments | `false` | Força o gateway indisponível |
| `GATEWAY_BREAKER_FAILURE_THRESHOLD` | payments | `5` | Falhas para abrir o circuito do gateway |
| `GATEWAY_BREAKER_OPEN_TIMEOUT_MS` | payments | `10000` | Tempo aberto antes da chamada de teste |
| `PAYMENT_REPROCESS_INTERVAL_MS` | payments | `5000` | Intervalo entre tentativas de reprocessamento |
| `PAYMENT_REPROCESS_TIMEOUT_SECS` | payments | `300` | Prazo; ao estourar, o pagamento falha e a compra é cancelada |
| `CATALOG_TIMEOUT_MS` | orders | `1000` | Timeout de cada chamada ao `catalog` |
| `CATALOG_RETRY_ATTEMPTS` | orders | `3` | Tentativas por chamada |
| `CATALOG_RETRY_BASE_DELAY_MS` / `_MAX_DELAY_MS` | orders | `100` / `1000` | Backoff exponencial |
| `CATALOG_BREAKER_FAILURE_THRESHOLD` | orders | `5` | Falhas para abrir o circuito do catálogo |
| `CATALOG_BREAKER_OPEN_TIMEOUT_MS` | orders | `10000` | Tempo aberto antes da chamada de teste |
| `CATALOG_CACHE_TTL_SECS` | orders | `600` | Validade dos produtos no cache de fallback |
| `AMQP_RETRY_TTL_MS` | todos | `5000` | Espera na fila `*.retry` |
| `AMQP_MAX_ATTEMPTS` | todos | `3` | Tentativas antes da fila `*.dead` |
| `HTTP_REQUEST_TIMEOUT_MS` | todos | `10000` | Tempo máximo por requisição; acima disso responde 504. Deve cobrir o orçamento das chamadas internas (tentativas × timeout do `catalog`) |
| `LOG_LEVEL` / `LOG_FORMAT` | todos | `info` / `pretty` (`json` no compose) | Logs estruturados |

Cada serviço registra no log, ao subir, a configuração efetiva de resiliência e
expõe a métrica `resilience_mechanism_enabled{mechanism}` (0 ou 1), usada para
conferir a condição de cada execução.

O gateway também pode ser alterado em tempo de execução:

```bash
curl localhost:8084/admin/gateway
curl -X PATCH localhost:8084/admin/gateway -H "content-type: application/json" \
  -d '{"unavailable":true}'
```

## Testes automatizados

Os testes de integração usam o PostgreSQL e o RabbitMQ do compose (cada teste de
banco cria um database temporário):

```bash
docker compose -f docker/docker-compose.yml up -d postgres rabbitmq
cp .env.example .env
cargo test --workspace
```

Cada teste de mensageria usa um exchange próprio (`shared::testing::IsolatedBroker`),
então a suíte pode rodar em paralelo.

## Testes de carga (k6)

O k6 roda em um container na rede do compose (profile `load`), sem instalação
local. Os scripts ficam em `load-tests/` e os resultados em `load-tests/results/`
(ignorado pelo Git).

```bash
docker compose -f docker/docker-compose.yml --profile load run --rm k6 run \
  --summary-trend-stats "avg,min,med,p(90),p(95),p(99),max" \
  --summary-export /results/baseline-01.json \
  /scripts/baseline.js
```

Parâmetros vão como variáveis de ambiente antes do nome do serviço, por exemplo
`run --rm -e RATE=25 -e DURATION=5m k6 run ...`.

| Cenário | O que mede | Duração padrão | Parâmetros |
|---|---|---|---|
| `baseline.js` | Comportamento saudável sob carga constante | 1 min de aquecimento + 2 min | `RATE` (10), `AQUECIMENTO`, `DURATION` |
| `stress.js` | Ponto de degradação, em patamares até o pico | ~13 min | `PEAK_RATE` (200), `LEVELS`, `RAMP`, `HOLD` |
| `spike.js` | Reação a um pico súbito e recuperação | ~4 min | `BASE_RATE` (10), `SPIKE_RATE` (150), `SPIKE_HOLD`, `RECOVERY` |
| `falhas.js` | Resiliência com falhas injetadas | 6 min | `FALHA`, `RATE`, `AQUECIMENTO`, `WARMUP`, `FAILURE_WINDOW`, `RECOVERY` |

Todos os cenários usam carga em modelo aberto (taxa de chegada fixa) e marcam
cada requisição com a fase em que ela ocorreu (`aquecimento`, `antes`, `falha`,
`depois`, ...). A fase de aquecimento deve ser descartada na análise.

**Tempo das sagas:** por padrão o k6 não acompanha as sagas (`SAGA_SAMPLE=0`).
Consultar `GET /orders/{id}` em loop gera carga proporcional à duração das
sagas, justamente o que se quer medir. O tempo de conclusão sai do banco
(`orders.updated_at` do estado final − `orders.created_at`), para todos os
pedidos. `SAGA_SAMPLE` > 0 volta a ligar as métricas `saga_duration` e
`saga_confirmed` no k6, só para inspeção manual.

**Leitura dos resultados:** os SLOs são avaliados por fase (POST com p95 < 500 ms
e menos de 1% de erro). É esperado que stress, spike e falhas terminem com
thresholds violados (código de saída 99): o resultado é *em qual fase* isso
acontece.

### Cenários de falha

Cada execução tem 1 min de aquecimento, 1 min normal, 1 min de falha e 3 min de
recuperação.

| `FALHA` | Injeção |
|---|---|
| `catalogo-lento` | Toxiproxy atrasa as respostas do catálogo (`CATALOG_LATENCY_MS`, padrão 2000) |
| `catalogo-indisponivel` | Toxiproxy desliga o proxy do catálogo (conexão recusada) |
| `catalogo-instavel` | Toxiproxy reseta conexões com probabilidade `TOXICITY` (padrão 0.3) |
| `gateway-indisponivel` | Gateway recusa todas as cobranças |
| `gateway-instavel` | `FAILURE_RATE` (padrão 0.5) |
| `gateway-lento` | `LATENCY_MS` (padrão 2000) |
| `servico-parado` | Container de `catalog`, `inventory` ou `payments` parado |

O Toxiproxy aplica falhas por conexão, e o cliente HTTP reaproveita conexões. Por
isso a taxa efetiva de falhas do `catalogo-instavel` deve ser lida na métrica
`catalog_client_requests_total{outcome}`, não presumida a partir de `TOXICITY`.

Falhas no gateway e no catálogo são aplicadas pelo próprio k6, via endpoint admin
e API do Toxiproxy:

```bash
docker compose -f docker/docker-compose.yml --profile load run --rm \
  -e FALHA=gateway-indisponivel k6 run --summary-export /results/gateway-indisponivel-01.json \
  /scripts/falhas.js
```

Parar containers exige o host, então usa o script PowerShell (que exporta o
resumo em `load-tests/results/` automaticamente):

```powershell
.\load-tests\falha-servico.ps1 -Servico catalog
```

O k6 imprime marcadores no início e no fim da janela de falha, e o script para
(`docker compose stop -t 0`) e religa o container ao vê-los. Se o teste for
interrompido, o gateway é restaurado e o container é religado.

## Execução dos experimentos

`load-tests/experimento.ps1` executa o protocolo completo de cada experimento do
TCC. Para cada execução ele:

1. Recria bancos, filas e serviços do zero com a configuração da condição.
2. Roda o k6 salvando cada requisição (`k6.csv.gz`) e o resumo (`summary.json`).
3. Espera a drenagem: nenhum pedido fora de `CONFIRMED`/`CANCELLED` e nenhuma
   mensagem pendente (exceto nas filas `.dead`).
4. Exporta as séries do Prometheus, as tabelas do banco e os logs.
5. Grava os metadados e valida a execução.

```powershell
.\load-tests\experimento.ps1 -Experimento 2A -Repeticoes 10 -Taxa 20
.\load-tests\experimento.ps1 -Experimento 1A -Repeticoes 5 -Taxas 10,20,40,60,80,100
.\load-tests\experimento.ps1 -Experimento 3A -Repeticoes 3 -Condicoes R-ON   # só uma condição (piloto)
```

| Experimento | Condições | Falha |
|---|---|---|
| `1A` | uma por taxa em `-Taxas` (tudo ligado) | — |
| `1B` | `C0-ROFF` (tudo desligado) × `C4-RON` (tudo ligado) | — |
| `2A` | `C0` a `C4` | `catalogo-lento` |
| `2B` | `C0`, `C2`, `C3`, `C4` | `catalogo-indisponivel` |
| `3A` | `R-ON` × `R-OFF` | `gateway-indisponivel` |
| `4A` | `parado-<servico>` (`-ServicoParado`, padrão `payments`) | `servico-parado` |

Níveis do cliente do catálogo, cumulativos:

| Nível | Timeout | Retry | Circuit breaker | Fallback |
|---|---|---|---|---|
| `C0` | — | — | — | — |
| `C1` | ✓ | — | — | — |
| `C2` | ✓ | ✓ | — | — |
| `C3` | ✓ | ✓ | ✓ | — |
| `C4` | ✓ | ✓ | ✓ | ✓ |

Cada repetição é um bloco com todas as condições em ordem sorteada (`-Semente`,
registrada). `-RepeticaoInicial` continua uma série interrompida sem repetir o
sorteio dos blocos anteriores. As imagens são reconstruídas no início (`-SemBuild`
pula). Se houver alterações sem commit, o script avisa: o hash registrado não
descreveria exatamente o código medido.

Resultados em `load-tests/results/experimentos/<exp>/<condição>/rep-NN/`:

| Arquivo | Conteúdo |
|---|---|
| `k6.csv.gz` | Uma linha por amostra de cada métrica do k6, com a tag `phase` |
| `summary.json` | Resumo do k6 (percentis por fase) |
| `sql/pedidos.csv` | Pedidos com estado final, `created_at` e `updated_at` |
| `sql/pagamentos.csv`, `sql/estoque.csv`, `sql/reservas.csv`, `sql/resultados_reserva.csv` | Estado final para os invariantes de consistência |
| `prometheus/*.json` | Séries de CPU, memória (working set), filas, mensagens, breaker, fallback, tentativas ao catálogo, reprocessamento e mecanismos ativos |
| `logs/` | Saída do k6 e logs JSON dos serviços (incluem as transições do circuit breaker) |
| `metadata.json` | Condição, horários UTC (início, falha, fim), parâmetros, commit, imagens, hardware, drenagem e validade |

`<exp>/execucoes.csv` lista todas as execuções com a indicação de validade.
Uma execução é marcada como inválida, sem ser apagada, se:

- o k6 terminou com código diferente de 0 ou 99 (99 = thresholds violados, esperado nas falhas);
- mais de 1% das iterações foram descartadas pelo k6 (`-LimiteDescarte`);
- algum comando de controle (injeção de falha) falhou;
- a drenagem não terminou em `-DrenagemMaxSegundos`;
- algum container reiniciou ou sofreu OOM;
- a CPU do k6 passou de 80% do seu limite.

Antes de rodar: pare o PostgreSQL nativo do Windows se estiver ativo, feche
programas pesados e mantenha o notebook na tomada.

## Coleta de métricas

### Grafana

Pasta **Marketplace**, com três dashboards provisionados:

- **Aplicacao - Latencia e Throughput** — latência p95 por rota, requisições
  por segundo, erros 5xx, latência e throughput de processamento por fila.
- **Recursos por Container** — CPU e memória de cada container (cAdvisor).
- **Resiliencia** — estado e transições dos circuit breakers, tempo de
  reprocessamento, ativações do fallback, retries e mensagens na fila dead.

Para exportar os dados de um painel: menu do painel → *Inspect* → *Data* →
*Download CSV*, com o intervalo de tempo do experimento selecionado.

### Prometheus

| Métrica | Consulta |
|---|---|
| Latência p95 do `POST /orders` | `histogram_quantile(0.95, sum by (le) (rate(http_request_duration_seconds_bucket{job="orders",method="POST",path="/orders"}[1m])))` |
| Throughput por serviço | `sum by (job) (rate(http_request_duration_seconds_count[1m]))` |
| Processamento p95 por fila | `histogram_quantile(0.95, sum by (le, queue) (rate(message_processing_duration_seconds_bucket[1m])))` |
| Tempo de reprocessamento p95 | `histogram_quantile(0.95, sum by (le, outcome) (rate(failure_reprocessing_duration_seconds_bucket[5m])))` |
| Estado dos circuit breakers | `max by (breaker) (circuit_breaker_state)` (0 fechado, 1 meio-aberto, 2 aberto) |
| Ativações do fallback | `sum(increase(fallback_activations_total[5m]))` |
| Retries e fila dead | `sum by (queue) (increase(message_retry_total[5m]))`, `sum by (queue) (message_dead_total)` |
| Chamadas rejeitadas pelo circuito aberto | `sum by (breaker) (increase(circuit_breaker_rejections_total[5m]))` |
| Tentativas ao catálogo por resultado | `sum by (outcome) (increase(catalog_client_requests_total[5m]))` |
| Mensagens na fila (prontas + em processamento) | `sum by (queue) (rabbitmq_queue_messages_ready) + sum by (queue) (rabbitmq_queue_messages_unacked)` |
| Mecanismos ativos | `max by (job, mechanism) (resilience_mechanism_enabled)` |
| CPU por container | `sum by (name) (rate(container_cpu_usage_seconds_total{name=~"marketplace-.+"}[1m]))` |
| Memória por container | `container_memory_working_set_bytes{name=~"marketplace-.+"}` |

Série temporal de um experimento pela API (horários em UTC):

```bash
curl -G localhost:9090/api/v1/query_range \
  --data-urlencode 'query=max by (breaker) (circuit_breaker_state)' \
  --data-urlencode 'start=2026-09-15T10:00:00Z' \
  --data-urlencode 'end=2026-09-15T10:05:00Z' \
  --data-urlencode 'step=5s'
```

Para os experimentos do TCC, use o `experimento.ps1`, que exporta essas séries
automaticamente. O Grafana serve só para inspeção visual.

O container do k6 aparece no cAdvisor: o consumo do gerador de carga fica
separado do consumo dos serviços, mas divide a mesma máquina.

## Solução de problemas

**`docker-credential-desktop: executable file not found in %PATH%`** — o
diretório de binários do Docker Desktop não está no `PATH` do terminal. Adicione
`resources\bin` da instalação (`C:\Program Files\Docker\Docker\resources\bin`, ou
`%LOCALAPPDATA%\Programs\DockerDesktop\resources\bin` em instalação por usuário)
ou remova `"credsStore": "desktop"` de `%USERPROFILE%\.docker\config.json`.

**Serviço reiniciando com `PRECONDITION_FAILED` do RabbitMQ** — as filas existem
com argumentos de uma versão anterior (sem dead-letter). Apague as filas:

```bash
docker exec marketplace-rabbitmq sh -c 'for q in $(rabbitmqctl list_queues -s name); do rabbitmqctl delete_queue "$q"; done'
```

**Porta 5432 ocupada** — defina `POSTGRES_PORT=5433` em `docker/.env` e ajuste a
`DATABASE_URL` do `.env` da raiz.

**k6 sem permissão para gravar em `/results` (Linux)** — a pasta criada pelo
Docker pertence ao root: `mkdir -p load-tests/results && chmod 777 load-tests/results`.

**Painéis sem dados** — confira os alvos em http://localhost:9090/targets; as
consultas com `rate()` precisam de tráfego e de pelo menos dois scrapes.

## Limitações conhecidas

- Se o RabbitMQ estiver fora do ar, o `orders` registra a falha ao publicar
  `order.created` mas responde 201: o pedido fica `PENDING` (não há outbox
  transacional).
- Pagamentos criados diretamente por `POST /payments` que ficarem pendentes não
  entram no reprocessamento; só o fluxo da saga é reprocessado.
- Por padrão cada fila é consumida uma mensagem por vez
  (`AMQP_CONSUMER_CONCURRENCY=1`). Com `LATENCY_MS` alto, o processamento de
  pagamentos vira gargalo e as filas acumulam.
- Não há reconexão ao RabbitMQ: se o broker cair, os consumidores param até o
  serviço ser reiniciado. Falha do broker fica fora dos experimentos.
- O gateway de pagamento é simulado dentro do próprio `payments`; o breaker do
  gateway não passa por rede.
- A cobrança no gateway não tem timeout; o timeout só existe na chamada ao
  catálogo e no limite de cada requisição HTTP.
