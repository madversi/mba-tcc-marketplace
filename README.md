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

### Infraestrutura (Docker)

```bash
docker compose -f docker/docker-compose.yml up -d
```

Sobe o PostgreSQL com um database por serviço (`catalog`, `inventory`,
`payments`, `orders`), criados pelo script `docker/postgres/init-databases.sql`
na primeira inicialização. Credenciais padrão `marketplace`/`marketplace`;
para alterar, copie `docker/.env.example` para `docker/.env`.

Para conferir os databases criados:

```bash
docker exec marketplace-postgres psql -U marketplace -l
```

### Serviços (local)

Cada serviço é um binário do workspace e lê sua configuração de variáveis de
ambiente (veja `.env.example`). A porta padrão é 8080; para rodar mais de um
serviço ao mesmo tempo, defina `HTTP_PORT`:

```bash
HTTP_PORT=8081 cargo run -p catalog
```

```bash
curl localhost:8081/health
```

### Testes

```bash
cargo test
```

## Status

Projeto em desenvolvimento inicial. Os serviços, a mensageria e os
cenários de teste serão adicionados nas próximas fases.
