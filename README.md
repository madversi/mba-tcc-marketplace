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

## Status

Projeto em desenvolvimento inicial. Estrutura, instruções de execução e
cenários de teste serão adicionados nas próximas fases.
