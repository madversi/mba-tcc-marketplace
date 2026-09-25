# Coleta oficial — 2026-09-23

Tag de congelamento: `experimento-v1` → `929cfc4149b04844691ec4fee79dede84c1996a2`. Nenhuma alteração em código, compose ou parâmetros após a tag.

Parâmetros comuns (docs\piloto.md): taxa nominal **10 req/s**, latência injetada no catálogo **2000 ms**, `-Aquecimento 1m -Antes 1m -Janela 1m -Recuperacao 3m -Medicao 3m -DrenagemMaxSegundos 600 -SemBuild`, `LimiteDescarte` 0,01, semente 20260917. O 1A usa `-Taxas 10,20,40,60`.

Execução: um script fora do repositório chama `load-tests\experimento.ps1` uma repetição por vez (`-RepeticaoInicial r -Repeticoes 1`). A ordem das condições dentro de cada repetição é a mesma do script, porque o embaralhamento com a semente é reproduzido para as repetições anteriores. Antes de cada repetição, o script espera o Docker responder (até 10 min). Depois de cada repetição, ele lê `execucoes.csv` e interrompe o experimento se as inválidas passarem de 30% das execuções previstas. A ordem dos experimentos é 2A (6 rep.), 3A (6), 2B (3), 1B (4), 1A (3 por nível) e 4A (3).

Esta é a segunda coleta com o mesmo protocolo. A primeira (2026-09-23, commit `4d58f42`) foi feita sobre outro histórico do repositório e está arquivada em `load-tests\results_20260923_4d58f42\` e `analise_20260923_4d58f42\`. O código é equivalente; muda o identificador de commit gravado nos metadados. O piloto (docs\piloto.md) não foi refeito.

Horários em horário local (UTC−4), salvo quando marcados com Z.

Condições verificadas antes do início (15:19–15:29): PostgreSQL nativo (`postgreSQL`, 9.6) parado, com inicialização Automatic; Grafana fora do ar (não há contêiner `marketplace-grafana`); suspensão e atualizações automáticas desativadas e nenhum outro programa pesado em execução, conforme confirmado pelo usuário. `analise_*/` e `load-tests/results_*/` foram acrescentadas a `.git\info\exclude` para que as pastas de arquivo não façam `alteracoes_nao_commitadas` ser diferente de zero.

## Preparação — estado zerado e sanidade

| Horário | Ação |
|---|---|
| 15:19:55 | `git status --porcelain` vazio; `HEAD` e `experimento-v1^{commit}` = `929cfc4149b04844691ec4fee79dede84c1996a2`. O commit e a tag já tinham sido feitos pelo autor. |
| 15:21:37 | Acrescentadas a `.git\info\exclude` as linhas `analise_*/` e `load-tests/results_*/`. `git status --porcelain` continuou vazio. |
| 15:21:44–15:21:54 | `docker compose -f docker\docker-compose.yml --profile grafana --profile load down -v --remove-orphans`. Removidos os 9 contêineres, a rede e os volumes `marketplace_postgres-data`, `marketplace_rabbitmq-data` e `marketplace_prometheus-data`. `docker ps -a` e `docker volume ls` com filtro `marketplace` vieram vazios. |
| 15:22:07–15:22:08 | Movidos (não apagados): `load-tests\results\experimentos` (2008 arquivos) para `load-tests\results_20260923_4d58f42\experimentos` (2008 arquivos); `analise\{consolidado,janelas,pedidos,tabelas,figuras}` e `analise\execucoes.csv` (34 arquivos) para `analise_20260923_4d58f42\` (34 arquivos). Mantidos no lugar: `analise\piloto` (30 arquivos), `load-tests\results\piloto` (316) e `load-tests\results\fase0` (27). `load-tests\results\experimentos` recriada vazia (0 arquivos). |
| 15:22:16–15:23:19 | `docker compose -f docker\docker-compose.yml build catalog orders inventory payments` (sem `--no-cache`), exit 0. Imagens: catalog `c1601c90bb0f`, orders `aed7e0fb21f3`, inventory `1ea00445d1a4`, payments `edd6f7aa2ec6`. |
| 15:23:21 | Espaço livre em E:: 620,7 GB. |
| 15:25:58 | `docker compose --profile load pull k6`: a imagem `grafana/k6:1.4.0` não estava no host (ver sanidade, tentativa 1). Digest `sha256:6a3ee54ac0e9ff5527923f6295257453dd88012f32f40dadf0eb1b638cbb21c7`. |

Sanidade: a pilha subiu com `up -d --wait` (15:23:37–15:24:10); `/health` respondeu 200 com `database: up` nas portas 8081 (catalog), 8082 (orders), 8083 (inventory) e 8084 (payments). Execução curta: `.\load-tests\experimento.ps1 -Experimento 1A -Taxas 10 -Repeticoes 1 -Aquecimento 30s -Medicao 30s -SemBuild`.

- **Tentativa 1** (15:24:16–15:25:17), inválida: `k6 terminou com código 1; summary.json ausente; CPU do k6 não foi medida`. `logs\k6.err.log`: `Image grafana/k6:1.4.0 Pulling` seguido de `error getting credentials - err: exec: "docker-credential-desktop": executable file not found in %PATH%`. A imagem foi baixada (tabela acima) e a tentativa foi mantida em `load-tests\results\fase0_v2\tentativa-1\experimentos\`.
- **Tentativa 2** (15:26:08–15:28:11), em `load-tests\results\fase0_v2\experimentos\1A\taxa-010\rep-01\`:

| Item | Resultado | Evidência |
|---|---|---|
| a) bancos vazios | OK | `inicio_utc` = 19:26:52,149Z; primeiro pedido às 19:26:54,035Z, primeiro pagamento às 19:26:54,062Z, primeiro resultado de reserva às 19:26:54,084Z. As 593 iterações do k6 (`summary.json`) batem com os 593 pedidos. |
| b) metadata.json | OK | Commit `929cfc4149b04844691ec4fee79dede84c1996a2`, `alteracoes_nao_commitadas` = 0, imagem e digest dos 7 contêineres do SUT, IDs dos contêineres (incluindo prometheus, cadvisor e k6) e ambiente (Ryzen 5 3600, 6 núcleos/12 threads, 16 GB; Windows 11 Home 10.0.26200; Docker 29.7.2 com 12 CPUs e 7,7 GiB; kernel 6.18.33.2-microsoft-standard-WSL2; compose 5.5.1). |
| c) CPU e memória | OK | `cpu.json`: 17 séries, 10 mapeadas para os 7 contêineres do SUT, prometheus, cadvisor e k6; as outras 7 não estão no mapa de contêineres. `memoria.json`: 10 séries, todas mapeadas. |
| d) k6_cpu_maxima | OK, não nulo | 0,02685 núcleo. |
| e) drenagem | OK | `drenagem_completa` = true, 1,6 s. |
| f) invariantes | OK, todos 0 | I1: 0 linhas em `stock_reservations`, Σ`reserved` = 0. I2: Σ(1.000.000 − available − reserved) = 593 = pedidos CONFIRMED. I3: 0 pagamentos PENDING (593 APPROVED). I4: 0 pedidos CONFIRMED sem pagamento APPROVED. |
| g) reinícios/OOM | OK | `RestartCount` = 0 e `OOMKilled` = false nos 7 contêineres. |

O script classificou a tentativa 2 como **inválida**: `iterações descartadas pelo k6: 8 de 601` (1,33%, limite 1%). No `k6.csv.gz`, as 8 linhas `dropped_iterations` têm carimbo 19:27:13Z–19:27:14Z, cerca de 21 s após `inicio_utc`, dentro do aquecimento de 30 s. Esse critério não é um dos itens a–g; a coleta seguiu.

## 2A — catálogo lento (latência de 2000 ms)

- Início 15:29:42, fim 18:59:46 (horário local); duração de 210,1 min. Primeira execução em 2026-09-23T19:30:25Z e última terminando em 22:59:43Z (UTC). 64,1 MB em disco.
- **Previstas 30 (5 condições × 6 repetições); executadas 30; válidas 29; descartadas 1 (3,3%).**
- Por condição: C0 6/6, C1 6/6, C2 **5/6**, C3 6/6, C4 6/6 válidas.
- Descartada: **C2 rep 5** (posição 1 do bloco), motivo registrado pelo script: `comandos de controle falharam: 1`. Evidência em `2A\C2\rep-05\logs\k6.err.log` e no `k6.csv.gz`. No fim da janela (FALHA_FIM às 21:53:25,086Z), o `POST http://toxiproxy:8474/reset` do cenário `fim_da_falha` falhou com `read: connection reset by peer` (k6 error_code 1220, status 0; check `toxiproxy restaurado (204)` com valor 0). O toxic de latência permaneceu ativo até o `teardown` (reset com 204 às 21:56:28Z), de modo que a falha se estendeu por toda a fase `depois`. A execução foi mantida em disco e não foi refeita. Drenagem completa (2,4 s), `k6_cpu_maxima` 0,033.
- Todas as execuções: commit `929cfc4`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,022 e 0,033 núcleo; drenagem completa em todas (1,6 a 2,9 s); 0 reinícios e 0 OOM.

## 3A — gateway de pagamento indisponível

- Início 18:59:46, fim 20:24:01 (local); duração de 84,3 min. Primeira execução em 23:00:30Z e última terminando em 2026-09-24T00:23:58Z. 35,3 MB.
- **Previstas 12 (2 condições × 6 repetições); executadas 12; válidas 12; descartadas 0.**
- Por condição: R-ON 6/6, R-OFF 6/6 válidas.
- Todas as execuções: commit `929cfc4`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,029 e 0,034 núcleo; drenagem completa em todas (1,7 a 3,1 s); 0 reinícios e 0 OOM.

## 2B — catálogo indisponível

- Início 20:24:01, fim 21:48:01 (local); duração de 84,0 min. Primeira execução em 00:24:45Z e última terminando em 01:47:58Z. 19,8 MB.
- **Previstas 12 (4 condições × 3 repetições); executadas 12; válidas 12; descartadas 0.**
- Por condição: C0 3/3, C2 3/3, C3 3/3, C4 3/3 válidas.
- Todas as execuções: commit `929cfc4`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,026 e 0,032 núcleo; drenagem completa em todas (1,6 a 2,5 s); 0 reinícios e 0 OOM.

## 1B — custo dos mecanismos sem falha

- Início 21:48:01, fim 22:28:12 (local); duração de 40,2 min. Primeira execução em 01:48:44Z e última terminando em 02:28:09Z. 8,5 MB.
- **Previstas 8 (2 condições × 4 repetições); executadas 8; válidas 8; descartadas 0.**
- Por condição: C0-ROFF 4/4, C4-RON 4/4 válidas.
- Todas as execuções: commit `929cfc4`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,026 e 0,032 núcleo; drenagem completa em todas (1,9 a 3,0 s); 0 reinícios e 0 OOM.
- Invariantes, conferidos logo ao fim do experimento por ser uma execução sem falha: I1 = I2 = I3 = I4 = 0 nas 8 execuções; 0 pedidos em estado não terminal. Pedidos por execução: 2401 em sete execuções e 2389 em C4-RON rep 1.

## 1A — capacidade por nível de carga

- Início 22:28:12, fim 23:29:15 (local); duração de 61,1 min. Primeira execução em 02:28:56Z e última terminando em 03:29:12Z. 36,0 MB.
- **Previstas 12 (4 níveis × 3 repetições); executadas 12; válidas 12; descartadas 0.**
- Por nível: 10, 20, 40 e 60 req/s com 3/3 válidas cada.
- Todas as execuções: commit `929cfc4`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,027 e 0,128 núcleo; drenagem completa em todas (1,6 a 22,8 s); 0 reinícios e 0 OOM.
- Invariantes, conferidos ao fim do experimento: I1 = I2 = I3 = I4 = 0 nas 12 execuções; 0 pedidos em estado não terminal.

## 4A — payments parado

- Início 23:29:15, fim 23:50:23 (local); duração de 21,1 min. Primeira execução em 03:29:59Z e última terminando em 03:50:20Z. 4,9 MB.
- **Previstas 3; executadas 3; válidas 3; descartadas 0.**
- Todas as execuções: commit `929cfc4`, `alteracoes_nao_commitadas` = 0; `k6_cpu_maxima` entre 0,028 e 0,031 núcleo; drenagem completa (1,6 a 2,4 s); serviço parado: payments, saudável de novo em 5,40 a 5,43 s após o religamento. `RestartCount` = 0 e OOM = false nos 7 contêineres (o `stop`/`start` do compose não conta como reinício).

## Resumo da coleta

| Experimento | Previstas | Válidas | Descartadas | Duração |
|---|---|---|---|---|
| 2A | 30 | 29 | 1 (C2 rep 5: reset do toxiproxy falhou) | 210,1 min |
| 3A | 12 | 12 | 0 | 84,3 min |
| 2B | 12 | 12 | 0 | 84,0 min |
| 1B | 8 | 8 | 0 | 40,2 min |
| 1A | 12 | 12 | 0 | 61,1 min |
| 4A | 3 | 3 | 0 | 21,1 min |
| **Total** | **77** | **76** | **1** | 15:29:42 → 23:50:23 (8 h 20 min) |

Nenhum experimento passou de 30% de execuções inválidas. Nenhuma condição de escalação ocorreu. Os 77 `metadata.json` trazem o commit `929cfc4149b04844691ec4fee79dede84c1996a2`, `alteracoes_nao_commitadas` = 0 e semente 20260917; nenhum tem `k6_cpu_maxima` nulo. Nenhuma pasta `.abortada` nem `erro.txt`. Volume em disco: 168,6 MB (`load-tests\results\experimentos\`).

Durante o 2A (17:25), o usuário pediu uma pausa ao fim do experimento; o script de orquestração recebeu uma espera condicionada a um arquivo de sinal, sem mudança de parâmetros. O pedido foi cancelado às 17:40, antes do fim do 2A, e a coleta não parou.

## Consolidação (23:51–23:54)

Comandos, a partir da raiz do repositório e nesta ordem (`estatistica.py` preenche o IC95% da `tabela5.csv` gerada por `tabelas.ps1`):

```
.\analise\extrair.ps1
.\analise\tabelas.ps1
python analise\estatistica.py
python analise\graficos.py
```

Ferramentas: Windows PowerShell 5.1 com um trecho em C# (`analise\lib\Analise.cs`) para ler o `k6.csv.gz`; Python 3.12.10 com pandas 3.0.6, numpy 2.5.3, scipy 1.18.1 e matplotlib 3.11.2.

Nenhum script de análise foi alterado. Na primeira chamada, `python` resolveu para o atalho da Microsoft Store (exit 9009), porque o PATH da sessão não trazia a instalação do usuário; `%LOCALAPPDATA%\Programs\Python\Python312` foi acrescentado ao PATH da sessão e os dois scripts Python foram rodados de novo, com exit 0.

Depois da consolidação, o autor decidiu fazer as figuras do TCC no Excel, a partir dos `figuraN.csv` gerados por `tabelas.ps1`. O `analise\graficos.py` foi removido do repositório. Os `.png` e `.pdf` que ele gerou nesta consolidação continuam em `analise\figuras\` só como registro e não são as figuras finais. O fluxo de consolidação passa a ser `extrair.ps1` → `tabelas.ps1` → `estatistica.py`.

### Gerado

- `analise\execucoes.csv`: 77 execuções, com validade e motivo de descarte (76 válidas).
- `analise\consolidado\{1A,1B,2A,2B,3A,4A}.csv`: uma linha por execução e fase, mais uma linha `fase = execucao` com as métricas da execução inteira (invariantes, drenagem, breaker, reprocessamento, totais de mensagens).
- `analise\janelas\<exp>.csv`: uma linha por execução e janela de 5 s, alinhada ao início da falha (ou ao início da medição no 1A/1B).
- `analise\pedidos\{3A,4A}.csv`: uma linha por pedido (coorte, status, tempo da saga).
- `analise\tabelas\tabela4.csv` a `tabela7.csv`.
- `analise\tabelas\estatistica_medianas.csv`: mediana, IQR e IC95% por reamostragem de cada métrica × condição (154 linhas).
- `analise\tabelas\estatistica_comparacoes.csv`: diferença de medianas entre níveis adjacentes (1A, 2A, 2B), C0-ROFF × C4-RON (1B) e R-OFF × R-ON (3A), com IC95%, p por reamostragem e p de Holm nas famílias de níveis adjacentes (98 linhas).
- `analise\tabelas\estatistica_mannwhitney_2A_C2xC3.csv`: U, p bicaudal e delta de Cliff (C3 em relação a C2) (6 linhas).
- `analise\figuras\figura2.csv` a `figura5.csv`: os dados de cada figura, que serão feitas no Excel. Os `.png` (300 dpi) e `.pdf` foram gerados pelo `graficos.py`, depois removido.
- `analise\piloto\`: a extração do piloto, mais `decisao_taxa.csv` (não regerada nesta coleta).

São 34 arquivos, a mesma lista da consolidação arquivada em `analise_20260923_4d58f42\`.

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
- **Gráficos:** feitos no Excel a partir de `analise\figuras\figuraN.csv`, fora dos scripts.

### Campos vazios e limitações, sem interpretação

- `fallback_pct_201` fica vazio quando a fase teve 0 respostas 201 (`post_201` = 0): a razão é indefinida. Ocorre na fase `falha` do 2A C3 (6 repetições), do 2B C0 (3) e do 2B C3 (3), e nas fases `falha` e `depois` do 2A C2 rep 5 (a execução descartada). Também fica vazio em todas as linhas `fase = execucao`.
- `cb_catalogo_primeira_abertura_s` só é preenchido na linha `fase = execucao` e fica vazio nas condições sem circuit breaker (C0, C1, C2) e nos experimentos sem falha no catálogo (1A, 1B, 3A, 4A). `cb_gateway_primeira_abertura_s` só é preenchido na linha `fase = execucao` do 3A.
- `reprocessamento_*` só é preenchido na linha `fase = execucao` do 3A R-ON; fica vazio no 3A R-OFF e no 4A (não há log `pagamento reprocessado` nessas condições) e nos demais experimentos.
- No Mann-Whitney, `fallback_pct_201` (0 valores em C3) e `cb_catalogo_primeira_abertura_s` (0 valores em C2) não têm valores em uma das condições, por isso o teste não foi calculado para essas métricas.
- A `figura4.csv` e a figura 4 incluem sagas CONFIRMED e CANCELLED (coluna `status` disponível para separar).
- O 2A C2 tem 5 repetições válidas (rep 5 descartada); o Mann-Whitney usou n = 5 em C2 e n = 6 em C3.
- A evidência de "503 sem cache", ausente no piloto, não foi verificada separadamente na coleta oficial.
- A sanidade desta coleta (tentativa 2) foi classificada inválida pelo critério de iterações descartadas, com os itens a–g atendidos; ela não entra na análise.
