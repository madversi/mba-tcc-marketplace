# Coleta oficial — 2026-09-23

Tag de congelamento: `experimento-v1` → `4d58f42f748cc5002a2c5cfef7846fba09ae877d`. Nenhuma alteração em código, compose ou parâmetros após a tag.

Parâmetros comuns (docs\piloto.md): taxa nominal **10 req/s**, latência injetada no catálogo **2000 ms**, `-Aquecimento 1m -Antes 1m -Janela 1m -Recuperacao 3m -Medicao 3m -DrenagemMaxSegundos 600 -SemBuild`, `LimiteDescarte` 0,01, semente 20260917. O 1A usa `-Taxas 10,20,40,60`.

Execução: um script fora do repositório chama `load-tests\experimento.ps1` uma repetição por vez (`-RepeticaoInicial r -Repeticoes 1`). A ordem das condições dentro de cada repetição é a mesma do script, porque o embaralhamento com a semente é reproduzido para as repetições anteriores. Antes de cada repetição, o script espera o Docker responder (até 10 min). Depois de cada repetição, ele lê `execucoes.csv` e interrompe o experimento se as inválidas passarem de 30% das execuções previstas. A ordem dos experimentos é 2A (6 rep.), 3A (6), 2B (3), 1B (4), 1A (3 por nível) e 4A (3).

Condições verificadas antes do início (00:30): PostgreSQL nativo parado; Grafana fora do ar (não há contêiner `marketplace-grafana`); suspensão e atualizações automáticas desativadas pelo usuário; nenhum outro programa pesado em execução (maiores consumidores: o app Claude e o Docker Desktop). `analise/` foi acrescentada a `.git\info\exclude` para que `alteracoes_nao_commitadas` = 0.

## 2A — catálogo lento (latência de 2000 ms)

- Início 00:30:20, fim 04:00:34 (horário local); duração de 210,2 min. Primeira execução em 2026-09-23T04:31:04Z e última terminando em 08:00:32Z (UTC). 64 MB em disco.
- **Previstas 30 (5 condições × 6 repetições); executadas 30; válidas 29; descartadas 1 (3,3%).**
- Por condição: C0 6/6, C1 6/6, C2 **5/6**, C3 6/6, C4 6/6 válidas.
- Descartada: **C2 rep 3** (posição 3 do bloco), motivo registrado pelo script: `comandos de controle falharam: 1`. Evidência em `2A\C2\rep-03\logs\k6.err.log` e no `k6.csv.gz`. No fim da janela (06:05:09Z), o `POST http://toxiproxy:8474/reset` do cenário `fim_da_falha` falhou com `read: connection reset by peer` (k6 error_code 1220). O toxic de latência permaneceu ativo até o `teardown` (reset com 204 às 06:08:12Z), de modo que a falha se estendeu por toda a fase `depois`. A execução foi mantida em disco e não foi refeita.
- Todas as execuções: commit `4d58f42`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,024 e 0,035 núcleo; drenagem completa em todas (máximo 2,8 s); 0 reinícios e 0 OOM.

## 3A — gateway de pagamento indisponível

- Início 04:00:34, fim 05:24:51 (local); duração de 84,3 min. Primeira execução em 08:01:18Z e última terminando em 09:24:49Z. 35 MB.
- **Previstas 12 (2 condições × 6 repetições); executadas 12; válidas 12; descartadas 0.**
- Por condição: R-ON 6/6, R-OFF 6/6 válidas.
- Todas as execuções: commit `4d58f42`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,028 e 0,036 núcleo; drenagem completa em todas (1,6 a 2,9 s); 0 reinícios e 0 OOM.

## 2B — catálogo indisponível

- Início 05:24:51, fim 06:48:49 (local); duração de 84,0 min. Primeira execução em 09:25:35Z e última terminando em 10:48:47Z. 20 MB.
- **Previstas 12 (4 condições × 3 repetições); executadas 12; válidas 12; descartadas 0.**
- Por condição: C0 3/3, C2 3/3, C3 3/3, C4 3/3 válidas.
- Todas as execuções: commit `4d58f42`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,026 e 0,033 núcleo; drenagem completa em todas (1,6 a 2,5 s); 0 reinícios e 0 OOM.

## 1B — custo dos mecanismos sem falha

- Início 06:48:49, fim 07:29:00 (local); duração de ~40 min. Primeira execução em 10:49:33Z e última terminando em 11:28:57Z. 8,5 MB.
- **Previstas 8 (2 condições × 4 repetições); executadas 8; válidas 8; descartadas 0.**
- Por condição: C0-ROFF 4/4, C4-RON 4/4 válidas.
- Todas as execuções: commit `4d58f42`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,026 e 0,032 núcleo; drenagem completa em todas (1,6 a 3,1 s); 0 reinícios e 0 OOM.
- Invariantes, conferidos logo ao fim do experimento por ser uma execução sem falha: I1 = I2 = I3 = I4 = 0 nas 8 execuções; 0 pedidos em estado não terminal.

## 1A — capacidade por nível de carga

- Início 07:29:00, fim 08:29:47 (local); duração de ~61 min. Primeira execução em 11:29:44Z e última terminando em 12:29:45Z. 36 MB.
- **Previstas 12 (4 níveis × 3 repetições); executadas 12; válidas 12; descartadas 0.**
- Por nível: 10, 20, 40 e 60 req/s com 3/3 válidas cada.
- Todas as execuções: commit `4d58f42`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,029 e 0,134 núcleo; drenagem completa em todas (1,6 a 18,8 s); 0 reinícios e 0 OOM.
- Invariantes, conferidos ao fim do experimento: I1 = I2 = I3 = I4 = 0 nas 12 execuções.

## 4A — payments parado

- Início 08:29:47, fim 08:50:52 (local); duração de 21,1 min. Primeira execução em 12:30:31Z e última terminando em 12:50:50Z. 4,9 MB.
- **Previstas 3; executadas 3; válidas 3; descartadas 0.**
- Todas as execuções: commit `4d58f42`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,030 e 0,031 núcleo; drenagem completa (1,7 a 2,2 s); serviço parado: payments, saudável de novo em 5,37 a 5,38 s após o religamento. `RestartCount` = 0 e OOM = false nos 7 contêineres (o `stop`/`start` do compose não conta como reinício).

## Resumo da coleta

| Experimento | Previstas | Válidas | Descartadas | Duração |
|---|---|---|---|---|
| 2A | 30 | 29 | 1 (C2 rep 3: reset do toxiproxy falhou) | 210,2 min |
| 3A | 12 | 12 | 0 | 84,3 min |
| 2B | 12 | 12 | 0 | 84,0 min |
| 1B | 8 | 8 | 0 | ~40 min |
| 1A | 12 | 12 | 0 | ~61 min |
| 4A | 3 | 3 | 0 | 21,1 min |
| **Total** | **77** | **76** | **1** | 00:30:20 → 08:50:52 (8 h 20 min) |

Nenhum experimento passou de 30% de execuções inválidas. Nenhuma condição de escalação ocorreu. Volume em disco: 168,6 MB (`load-tests\results\experimentos\`).

## Fase 4 — consolidação (08:51–09:05)

Comandos, a partir da raiz do repositório e nesta ordem (`estatistica.py` preenche o IC95% da `tabela5.csv` gerada por `tabelas.ps1`):

```
.\analise\extrair.ps1
.\analise\tabelas.ps1
python analise\estatistica.py
python analise\graficos.py
```

Ferramentas: Windows PowerShell 5.1 com um trecho em C# (`analise\lib\Analise.cs`) para ler o `k6.csv.gz`; Python 3.12.10 com pandas 3.0.6, numpy 2.5.3, scipy 1.18.1 e matplotlib 3.11.2.

### Gerado

- `analise\execucoes.csv`: 77 execuções, com validade e motivo de descarte (76 válidas).
- `analise\consolidado\{1A,1B,2A,2B,3A,4A}.csv`: uma linha por execução e fase, mais uma linha `fase = execucao` com as métricas da execução inteira (invariantes, drenagem, breaker, reprocessamento, totais de mensagens).
- `analise\janelas\<exp>.csv`: uma linha por execução e janela de 5 s, alinhada ao início da falha (ou ao início da medição no 1A/1B).
- `analise\pedidos\{3A,4A}.csv`: uma linha por pedido (coorte, status, tempo da saga).
- `analise\tabelas\tabela4.csv` a `tabela7.csv`.
- `analise\tabelas\estatistica_medianas.csv`: mediana, IQR e IC95% por reamostragem de cada métrica × condição.
- `analise\tabelas\estatistica_comparacoes.csv`: diferença de medianas entre níveis adjacentes (1A, 2A, 2B), C0-ROFF × C4-RON (1B) e R-OFF × R-ON (3A), com IC95%, p por reamostragem e p de Holm nas famílias de níveis adjacentes.
- `analise\tabelas\estatistica_mannwhitney_2A_C2xC3.csv`: U, p bicaudal e delta de Cliff (C3 em relação a C2).
- `analise\figuras\figura2.csv` a `figura5.csv`, cada uma com `.png` (300 dpi) e `.pdf`.
- `analise\piloto\`: a mesma extração aplicada ao piloto, mais `decisao_taxa.csv`.

### Definições adotadas nos scripts

- **Percentis:** calculados sobre as durações brutas do k6 (`http_req_duration` do POST /orders), com interpolação linear (tipo 7). Nenhuma latência é descartada, e as requisições com erro entram na distribuição.
- **Erro:** resposta diferente de 201. Separado em 503, 504, conexão (status 0) e outros.
- **Vazão efetiva:** respostas 201 divididas pela duração da fase.
- **Fases:**
  - k6: pela etiqueta `phase`.
  - SQL e Prometheus: por intervalos absolutos. No 1A e no 1B, a origem é o primeiro pedido criado (SQL). Nos experimentos com falha, a origem é o marcador `FALHA_INICIO` menos a soma de aquecimento e `antes`, e a fase `falha` usa os marcadores `FALHA_INICIO` e `FALHA_FIM`.
- **Contadores do Prometheus:** amostrados a cada 5 s. O aumento é calculado com tratamento de reinício do contador (necessário no 4A). A atribuição a uma fase tem incerteza de uma amostra, e por isso o fator de amplificação no aquecimento aparece abaixo de 1.
- **Janelas de 5 s:** as requisições entram pelo carimbo de término, que tem resolução de 1 s. Nas janelas, o estado do circuit breaker é o máximo no intervalo (0 fechado, 1 semiaberto, 2 aberto), calculado a partir das transições no log.
- **Tempo de recuperação:** medido do fim da falha até o início da primeira de 3 janelas consecutivas com erro ≤ 1% e p95 ≤ 1,2 × p95 da fase `antes`.
- **Reprocessamento:** tempo entre `created_at` e `updated_at` dos pagamentos com log `pagamento reprocessado`.
- **Invariantes:**
  - I1: linhas em `stock_reservations` e Σ`reserved`.
  - I2: Σ(1.000.000 − available − reserved) − pedidos CONFIRMED, verificado só no agregado (sem `product_id` nos pedidos).
  - I3: pagamentos PENDING.
  - I4: pedidos CONFIRMED sem pagamento APPROVED.
- **Reamostragem:** B = 10.000, semente 20260917, IC percentil. p bootstrap = 2 × min(P(d ≤ 0), P(d ≥ 0)). Holm aplicado por família (experimento × fase × métrica) nas comparações entre níveis adjacentes.
- **Gráficos:** sem grade, sem fundo (PNG transparente), sem título interno, só eixos esquerdo e inferior em preto de 1,5 pt, Arial de 9 a 11 pt em preto, painéis identificados por letra maiúscula.

### Campos vazios e limitações, sem interpretação

- `fallback_pct_201` fica vazio quando a fase teve 0 respostas 201 (2A C3; 2B C0 e C3): a razão é indefinida.
- `cb_primeira_abertura_s` fica vazio nas condições sem circuit breaker (C0, C1, C2).
- `reprocessamento_*` fica vazio no 3A R-OFF e no 4A: não há log `pagamento reprocessado` nessas condições.
- No Mann-Whitney, `fallback_pct_201` e `cb_catalogo_primeira_abertura_s` não têm valores em uma das condições (C2 não tem breaker nem fallback), por isso o teste não foi calculado para essas métricas.
- A `figura4.csv` e a figura 4 incluem sagas CONFIRMED e CANCELLED (coluna `status` disponível para separar).
- O 2A C2 tem 5 repetições válidas (uma descartada).
- A evidência de "503 sem cache", ausente no piloto, não foi verificada separadamente na coleta oficial.
