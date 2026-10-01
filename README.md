# Marketplace em Rust — Microsserviços com Resiliência e Mensageria

Projeto de TCC (MBA em Engenharia de Software) — implementação de um back-end de
marketplace fictício em Rust, dividido em microsserviços que se comunicam via
mensageria assíncrona (RabbitMQ), com padrões de resiliência (retry, timeout,
circuit breaker e fallback).

**Aluno:** Helder Marcelo Adversi Junior
**Orientadora:** Elaine Barbosa de Figueiredo

O código medido corresponde à tag [`experimento-v1`](../../tree/experimento-v1).
Os resumos por execução e a reprodução da análise estão descritos em
[Dados do experimento e reprodução da análise](#dados-do-experimento-e-reprodução-da-análise).

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

As credenciais e portas padrão são de desenvolvimento, e as portas são publicadas
em todas as interfaces do host: use o ambiente apenas em máquina local.

Todos os containers têm limite de CPU e memória, e PostgreSQL, RabbitMQ e
Toxiproxy estão fixados por digest. O Grafana fica fora da subida padrão; para
inspecionar os painéis:
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

## Configuração

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
| `SERVICE_CPUS` / `SERVICE_MEMORY` | compose | `1.0` / `256m` | Limites dos 4 serviços Rust (há variáveis equivalentes para Postgres, RabbitMQ e Toxiproxy) |
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
expõe a métrica `resilience_mechanism_enabled{mechanism}` (0 ou 1).

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

## Observabilidade

### Grafana

Pasta **Marketplace**, com três dashboards provisionados:

- **Aplicacao - Latencia e Throughput** — latência p95 por rota, requisições
  por segundo, erros 5xx, latência e throughput de processamento por fila.
- **Recursos por Container** — CPU e memória de cada container (cAdvisor).
- **Resiliencia** — estado e transições dos circuit breakers, tempo de
  reprocessamento, ativações do fallback, retries e mensagens na fila dead.

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
| CPU por container | `sum by (id) (rate(container_cpu_usage_seconds_total{id=~"/docker/[0-9a-f]{64}"}[1m]))` |
| Memória por container | `container_memory_working_set_bytes{id=~"/docker/[0-9a-f]{64}"}` |

O cAdvisor roda sem acesso ao Docker. Com o image store do containerd (padrão
do Docker Desktop atual) ele não reconhece os containers pelo nome e descarta
as séries; lendo só os cgroups, cada container aparece como
`id="/docker/<id completo>"`. Para saber quem é quem:
`docker ps --no-trunc --format "{{.ID}} {{.Names}}"`.

## Dados do experimento e reprodução da análise

A pasta `analise/` reúne os scripts de consolidação e de análise e os resumos
por execução da coleta de 23 e 24 de setembro de 2026 (77 execuções, 76
válidas, todas com o código da tag `experimento-v1`):

| Caminho | Conteúdo |
|---|---|
| `analise/execucoes.csv` | Uma linha por execução: experimento, condição, repetição e posição no bloco, início e fim (UTC), validade e motivo de descarte, CPU máxima do k6, tempo de drenagem, commit medido e indicação de alterações não commitadas |
| `analise/consolidado/<experimento>.csv` | Resumos por execução e fase (requisições, erros, latências, vazão, sagas, CPU e memória por container): a unidade estatística do trabalho |
| `analise/janelas/<experimento>.csv` | Séries por janela de 5 s de cada execução (requisições, erros, latências, fila, sagas e estado dos circuit breakers) |
| `analise/tabelas/` | Tabelas 4 a 7 do texto (`tabela*.csv`), medianas, intervalos e testes (`estatistica_*.csv`) e a análise complementar (`complementar_*.csv`) |

Não estão publicados os dados brutos de cada execução (`load-tests/results/`,
mais de 200 MB com a saída do k6, as tabelas do banco, as métricas e os logs),
as tabelas de pedidos extraídas deles (`analise/pedidos/`), o piloto
(`analise/piloto/`) e a pasta `analise/figuras/` (CSVs, PNGs e PDFs das
figuras; os CSVs das figuras 2, 3 e 5 são refeitos pelo comando abaixo, e o da
figura 4 depende de `analise/pedidos/3A.csv`).

Para refazer as tabelas e os testes estatísticos a partir dos resumos (Windows
PowerShell 5.1 e Python 3.12 com NumPy, pandas e SciPy; semente fixa 20260917
e 10 000 reamostragens), na raiz do repositório:

```powershell
pip install numpy pandas scipy
powershell -ExecutionPolicy Bypass -File analise\tabelas.ps1   # analise\tabelas\tabela4.csv a tabela7.csv e analise\figuras\figura2, 3 e 5.csv
python analise\estatistica.py    # analise\tabelas\estatistica_*.csv e os intervalos da tabela5
python analise\complementar.py   # analise\tabelas\complementar_sensibilidade.csv e complementar_pontos_por_execucao.csv
```

`analise/extrair.ps1` produz `consolidado/`, `janelas/`, `pedidos/` e
`execucoes.csv` a partir dos dados brutos, e `complementar.py` só refaz
`complementar_1A_por_execucao.csv` (fila, pedidos pendentes e conclusão de
sagas por execução do 1A) quando encontra os `pedidos.csv` brutos do 1A em
`load-tests/results/experimentos` ou no caminho passado em `--resultados`.
`decisao_taxa.ps1` aplica a regra de escolha da taxa nominal ao consolidado do
piloto.

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
  serviço ser reiniciado.
- O gateway de pagamento é simulado dentro do próprio `payments`; o breaker do
  gateway não passa por rede.
- A cobrança no gateway não tem timeout; o timeout só existe na chamada ao
  catálogo e no limite de cada requisição HTTP.
