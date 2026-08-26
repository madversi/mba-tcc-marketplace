# Marketplace em Rust — Microsserviços com Resiliência e Mensageria

Projeto de TCC (MBA em Engenharia de Software) — implementação de um back-end de
marketplace fictício em Rust, dividido em microsserviços que se comunicam via
mensageria assíncrona (RabbitMQ), com padrões de resiliência (retry, timeout,
circuit breaker e fallback), voltado à coleta de métricas de desempenho.

**Aluno:** Helder Marcelo Adversi Junior
**Orientadora:** Elaine Barbosa de Figueiredo

## Arquitetura

| Serviço | Responsabilidade |
|---|---|
| `catalog` | Produtos e vendedores (CRUD REST) |
| `orders` | Pedidos do comprador; orquestra o fluxo de compra via eventos |
| `inventory` | Estoque: reserva e liberação |
| `payments` | Pagamento, com gateway externo simulado (falhas injetáveis) |

Infraestrutura: RabbitMQ, PostgreSQL, Prometheus, Grafana e cAdvisor, tudo
orquestrado via Docker Compose.

## Executando

### Stack completa (Docker)

```bash
docker compose -f docker/docker-compose.yml up -d --build
```

Sobe o PostgreSQL e os 4 serviços, cada um construído a partir do mesmo
`docker/Dockerfile` (multi-stage com `cargo-chef`; a camada de dependências é
compartilhada entre eles). Os serviços só iniciam depois do Postgres passar no
healthcheck e aplicam as próprias migrations ao subir.

| Serviço | Porta no host | Health |
|---|---|---|
| `catalog` | 8081 | `curl localhost:8081/health` |
| `orders` | 8082 | `curl localhost:8082/health` |
| `inventory` | 8083 | `curl localhost:8083/health` |
| `payments` | 8084 | `curl localhost:8084/health` |

Dentro da rede do compose os serviços se falam pelo nome (`orders` chama
`http://catalog:8080`); as portas acima são só o mapeamento para o host.

O PostgreSQL tem um database por serviço (`catalog`, `inventory`, `payments`,
`orders`), criados pelo script `docker/postgres/init-databases.sql` na primeira
inicialização. Credenciais padrão `marketplace`/`marketplace`; para alterar,
copie `docker/.env.example` para `docker/.env`.

> Se já existir um PostgreSQL instalado na máquina ocupando a porta 5432, defina
> `POSTGRES_PORT=5433` em `docker/.env` e ajuste a porta na `DATABASE_URL` do
> `.env` da raiz.

Para conferir os databases criados:

```bash
docker exec marketplace-postgres psql -U marketplace -l
```

### Serviços (local)

Cada serviço é um binário do workspace e lê sua configuração de variáveis de
ambiente (`.env` na raiz é carregado automaticamente). Para rodar um serviço
fora do Docker, defina a porta e o database dele:

```bash
HTTP_PORT=8081 DATABASE_URL=postgres://marketplace:marketplace@localhost:5433/catalog cargo run -p catalog
```

No PowerShell: `$env:HTTP_PORT=8081; $env:DATABASE_URL="..."; cargo run -p catalog`.
O `orders` também precisa de `CATALOG_URL` (ex.: `http://localhost:8081`).

```bash
curl localhost:8081/health
```

### Testes

```bash
cargo test
```

## Status

Os 4 serviços rodam de forma independente (REST + PostgreSQL). Ainda não há
comunicação assíncrona entre eles: a saga de compra via RabbitMQ, o log
estruturado, as métricas e os padrões de resiliência entram nas próximas fases.
