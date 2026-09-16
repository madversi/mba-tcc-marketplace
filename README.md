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
| Prometheus | http://localhost:9090 | — |
| Grafana | http://localhost:3000 | `admin` / `admin` |
| cAdvisor | http://localhost:8085 | — |

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
migrations ao subir. Para sobrescrever portas e credenciais, copie
`docker/.env.example` para `docker/.env`.

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
| `LOG_LEVEL` / `LOG_FORMAT` | todos | `info` / `json` | Logs estruturados |

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

O teste `estoque_insuficiente_publica_stock_rejected` pode falhar
esporadicamente na suíte completa: os testes compartilham o exchange do broker e
consumidores de testes paralelos respondem ao mesmo evento. Rodado isoladamente
(`cargo test -p inventory --test consumer estoque_insuficiente`), passa.

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
| `baseline.js` | Comportamento saudável sob carga constante | 2 min | `RATE` (10), `DURATION` |
| `stress.js` | Ponto de degradação, em patamares até o pico | ~13 min | `PEAK_RATE` (200), `LEVELS`, `RAMP`, `HOLD` |
| `spike.js` | Reação a um pico súbito e recuperação | ~4 min | `BASE_RATE` (10), `SPIKE_RATE` (150), `SPIKE_HOLD`, `RECOVERY` |
| `falhas.js` | Resiliência com falhas injetadas | 5 min | `FALHA`, `RATE`, `WARMUP`, `FAILURE_WINDOW`, `RECOVERY` |

Todos os cenários usam carga em modelo aberto (taxa de chegada fixa) e registram
duas métricas próprias: `saga_duration` (do `POST /orders` até o pedido chegar a
`CONFIRMED` ou `CANCELLED`) e `saga_confirmed` (taxa de pedidos confirmados).
Stress, spike e falhas acompanham só uma amostra das sagas (`SAGA_SAMPLE`).

**Leitura dos resultados:** stress, spike e falhas marcam cada requisição com a
fase em que ela ocorreu e avaliam os SLOs por fase (POST com p95 < 500 ms e menos
de 1% de erro; saga confirmada em mais de 99%). É esperado que stress e spike
terminem com thresholds violados: o resultado é *em qual fase* isso acontece.

### Cenários de falha

Cada experimento tem 1 min normal, 1 min de falha e 3 min de recuperação.

| `FALHA` | Injeção |
|---|---|
| `gateway-indisponivel` | Gateway recusa todas as cobranças |
| `gateway-instavel` | `FAILURE_RATE` (padrão 0.5) |
| `gateway-lento` | `LATENCY_MS` (padrão 2000) |
| `servico-parado` | Container de `catalog`, `inventory` ou `payments` parado |

Falhas no gateway são aplicadas pelo próprio k6, via endpoint admin:

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
| CPU por container | `sum by (name) (rate(container_cpu_usage_seconds_total{name=~"marketplace-.+"}[1m]))` |
| Memória por container | `container_memory_usage_bytes{name=~"marketplace-.+"}` |

Série temporal de um experimento pela API (horários em UTC):

```bash
curl -G localhost:9090/api/v1/query_range \
  --data-urlencode 'query=max by (breaker) (circuit_breaker_state)' \
  --data-urlencode 'start=2026-09-15T10:00:00Z' \
  --data-urlencode 'end=2026-09-15T10:05:00Z' \
  --data-urlencode 'step=5s'
```

### Procedimento sugerido por experimento

1. Anotar o horário de início.
2. Rodar o cenário com `--summary-export` para `load-tests/results/`.
3. Anotar o horário de término e exportar os painéis do Grafana nesse intervalo.
4. Para repetir do zero, exportar o que for necessário e só então
   `docker compose -f docker/docker-compose.yml down -v`, que apaga também as
   séries do Prometheus.

O container `marketplace-k6` aparece no cAdvisor: o consumo do gerador de carga
fica separado do consumo dos serviços, mas divide a mesma máquina.

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
- Cada fila é consumida uma mensagem por vez. Com `LATENCY_MS` alto, o
  processamento de pagamentos vira gargalo e as filas acumulam.
