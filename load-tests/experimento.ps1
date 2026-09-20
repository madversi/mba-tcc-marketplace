param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('1A', '1B', '2A', '2B', '3A', '4A')]
    [string]$Experimento,
    [int]$Repeticoes = 1,
    [int]$RepeticaoInicial = 1,
    [int]$Semente = 20260917,
    [string[]]$Condicoes = @(),
    [int]$Taxa = 20,
    [int[]]$Taxas = @(10, 20, 40, 60, 80, 100),
    [string]$Aquecimento = '1m',
    [string]$Antes = '1m',
    [string]$Janela = '1m',
    [string]$Recuperacao = '3m',
    [string]$Medicao = '3m',
    [int]$LatenciaCatalogoMs = 2000,
    [ValidateSet('payments', 'inventory', 'catalog')]
    [string]$ServicoParado = 'payments',
    [int]$DrenagemMaxSegundos = 600,
    [double]$LimiteDescarte = 0.01,
    [string]$PostgresUser = 'marketplace',
    [switch]$SemBuild
)

$ErrorActionPreference = 'Stop'

if (-not (Get-Command 'docker-credential-desktop' -ErrorAction SilentlyContinue)) {
    $candidatos = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\DockerDesktop\resources\bin'),
        (Join-Path $env:ProgramFiles 'Docker\Docker\resources\bin')
    )
    foreach ($dir in $candidatos) {
        if (Test-Path (Join-Path $dir 'docker-credential-desktop.exe')) {
            $env:PATH = "$dir;$env:PATH"
            break
        }
    }
}

$raiz = Resolve-Path (Join-Path $PSScriptRoot '..')
$compose = Join-Path $raiz 'docker\docker-compose.yml'
$saidaBase = Join-Path $PSScriptRoot 'results\experimentos'
$prometheus = 'http://localhost:9090'
$servicosSut = @('postgres', 'rabbitmq', 'toxiproxy', 'catalog', 'orders', 'inventory', 'payments')
$containersSut = @(
    'marketplace-postgres', 'marketplace-rabbitmq', 'marketplace-toxiproxy', 'marketplace-catalog',
    'marketplace-orders', 'marketplace-inventory', 'marketplace-payments'
)
$utf8 = New-Object System.Text.UTF8Encoding($false)

$niveisCatalogo = [ordered]@{
    'C0' = @('false', 'false', 'false', 'false')
    'C1' = @('true', 'false', 'false', 'false')
    'C2' = @('true', 'true', 'false', 'false')
    'C3' = @('true', 'true', 'true', 'false')
    'C4' = @('true', 'true', 'true', 'true')
}

function Get-EnvServicos([string]$nivel, [bool]$reprocessamento, [bool]$breakerGateway) {
    $flags = $niveisCatalogo[$nivel]
    return [ordered]@{
        CATALOG_TIMEOUT_ENABLED   = $flags[0]
        CATALOG_RETRY_ENABLED     = $flags[1]
        CATALOG_BREAKER_ENABLED   = $flags[2]
        CATALOG_FALLBACK_ENABLED  = $flags[3]
        PAYMENT_REPROCESS_ENABLED = $reprocessamento.ToString().ToLower()
        GATEWAY_BREAKER_ENABLED   = $breakerGateway.ToString().ToLower()
    }
}

function New-Condicao([string]$nome, [string]$script, $servicos, $k6) {
    return [pscustomobject]@{ Nome = $nome; Script = $script; Servicos = $servicos; K6 = $k6 }
}

function Get-K6Falha([string]$falha) {
    return [ordered]@{
        FALHA              = $falha
        RATE               = "$Taxa"
        AQUECIMENTO        = $Aquecimento
        WARMUP             = $Antes
        FAILURE_WINDOW     = $Janela
        RECOVERY           = $Recuperacao
        CATALOG_LATENCY_MS = "$LatenciaCatalogoMs"
    }
}

function Get-Condicoes {
    switch ($Experimento) {
        '1A' {
            foreach ($t in $Taxas) {
                New-Condicao ('taxa-{0:D3}' -f $t) 'baseline.js' (Get-EnvServicos 'C4' $true $true) `
                ([ordered]@{ RATE = "$t"; AQUECIMENTO = $Aquecimento; DURATION = $Medicao })
            }
        }
        '1B' {
            $k6 = [ordered]@{ RATE = "$Taxa"; AQUECIMENTO = $Aquecimento; DURATION = $Medicao }
            New-Condicao 'C0-ROFF' 'baseline.js' (Get-EnvServicos 'C0' $false $false) $k6
            New-Condicao 'C4-RON' 'baseline.js' (Get-EnvServicos 'C4' $true $true) $k6
        }
        '2A' {
            foreach ($nivel in @('C0', 'C1', 'C2', 'C3', 'C4')) {
                New-Condicao $nivel 'falhas.js' (Get-EnvServicos $nivel $true $true) (Get-K6Falha 'catalogo-lento')
            }
        }
        '2B' {
            foreach ($nivel in @('C0', 'C2', 'C3', 'C4')) {
                New-Condicao $nivel 'falhas.js' (Get-EnvServicos $nivel $true $true) (Get-K6Falha 'catalogo-indisponivel')
            }
        }
        '3A' {
            New-Condicao 'R-ON' 'falhas.js' (Get-EnvServicos 'C4' $true $true) (Get-K6Falha 'gateway-indisponivel')
            New-Condicao 'R-OFF' 'falhas.js' (Get-EnvServicos 'C4' $false $true) (Get-K6Falha 'gateway-indisponivel')
        }
        '4A' {
            $k6 = Get-K6Falha 'servico-parado'
            $k6['SERVICO'] = $ServicoParado
            New-Condicao "parado-$ServicoParado" 'falhas.js' (Get-EnvServicos 'C4' $true $true) $k6
        }
    }
}

function Invoke-Docker([string[]]$Argumentos, [switch]$IgnorarErro) {
    $anterior = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $saida = & docker @Argumentos 2>&1 | ForEach-Object { "$_" }
    }
    finally {
        $ErrorActionPreference = $anterior
    }
    if ($LASTEXITCODE -ne 0 -and -not $IgnorarErro) {
        throw "docker $($Argumentos -join ' ') falhou ($LASTEXITCODE):`n$($saida -join "`n")"
    }
    return $saida
}

function Write-Texto([string]$caminho, $linhas) {
    [System.IO.File]::WriteAllText($caminho, (($linhas | ForEach-Object { "$_" }) -join "`n") + "`n", $utf8)
}

function Get-Utc([DateTime]$data) {
    return $data.ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
}

function Get-Sql([string]$banco, [string]$consulta) {
    return Invoke-Docker @('exec', 'marketplace-postgres', 'psql', '-U', $PostgresUser, '-d', $banco, '-tAc', $consulta)
}

function Export-Sql([string]$destino, [string]$banco, [string]$consulta) {
    $linhas = Invoke-Docker @(
        'exec', 'marketplace-postgres', 'psql', '-U', $PostgresUser, '-d', $banco, '-c',
        "COPY ($consulta) TO STDOUT WITH CSV HEADER"
    )
    Write-Texto $destino $linhas
}

function Reset-Ambiente($condicao) {
    foreach ($chave in $condicao.Servicos.Keys) {
        Set-Item -Path "env:$chave" -Value $condicao.Servicos[$chave]
    }
    Invoke-Docker (@('compose', '-f', $compose, 'rm', '-s', '-f') + $servicosSut) | Out-Null
    Invoke-Docker @('volume', 'rm', 'marketplace_postgres-data', 'marketplace_rabbitmq-data') -IgnorarErro | Out-Null
    Invoke-Docker (@('compose', '-f', $compose, 'up', '-d', '--wait') + $servicosSut + @('prometheus', 'cadvisor')) | Out-Null
}

function Get-Pendencias {
    $pedidos = [int](Get-Sql 'orders' "SELECT count(*) FROM orders WHERE status NOT IN ('CONFIRMED', 'CANCELLED')" | Select-Object -Last 1)
    $mensagens = 0
    $filas = Invoke-Docker @('exec', 'marketplace-rabbitmq', 'rabbitmqctl', 'list_queues', '--quiet', '--no-table-headers', 'name', 'messages')
    foreach ($linha in $filas) {
        $partes = "$linha".Trim() -split '\s+'
        if ($partes.Count -eq 2 -and $partes[1] -match '^\d+$' -and -not $partes[0].EndsWith('.dead')) {
            $mensagens += [int]$partes[1]
        }
    }
    return [pscustomobject]@{ Pedidos = $pedidos; Mensagens = $mensagens }
}

function Wait-Drenagem {
    $inicio = Get-Date
    do {
        $pendencias = Get-Pendencias
        if ($pendencias.Pedidos -eq 0 -and $pendencias.Mensagens -eq 0) {
            return [pscustomobject]@{ Completa = $true; Segundos = ((Get-Date) - $inicio).TotalSeconds; Pendencias = $pendencias }
        }
        Start-Sleep -Seconds 2
    } while (((Get-Date) - $inicio).TotalSeconds -lt $DrenagemMaxSegundos)
    return [pscustomobject]@{ Completa = $false; Segundos = ((Get-Date) - $inicio).TotalSeconds; Pendencias = $pendencias }
}

function Test-Marcador([string]$arquivo, [string]$marcador) {
    if (-not (Test-Path $arquivo)) { return $null }
    $stream = [System.IO.File]::Open($arquivo, 'Open', 'Read', 'ReadWrite')
    try {
        $texto = (New-Object System.IO.StreamReader($stream)).ReadToEnd()
    }
    finally {
        $stream.Dispose()
    }
    $achado = [regex]::Match($texto, "$marcador[^\n]*utc=([0-9T:\.\-]+Z)")
    if ($achado.Success) { return $achado.Groups[1].Value }
    return $null
}

function Wait-Marcador([string]$arquivo, [string]$marcador, $processo) {
    while (-not (Test-Marcador $arquivo $marcador)) {
        if ($processo.HasExited) {
            throw "k6 terminou antes de emitir '$marcador'. Veja $arquivo"
        }
        Start-Sleep -Milliseconds 250
    }
}

function Wait-Saudavel([string]$container) {
    $inicio = Get-Date
    while (((Get-Date) - $inicio).TotalSeconds -lt 120) {
        $estado = Invoke-Docker @('inspect', '-f', '{{.State.Health.Status}}', $container) -IgnorarErro | Select-Object -Last 1
        if ("$estado".Trim() -eq 'healthy') {
            return ((Get-Date) - $inicio).TotalSeconds
        }
        Start-Sleep -Milliseconds 500
    }
    return $null
}

function Invoke-K6($condicao, [string]$relativo, [string]$dirLogs) {
    $argumentos = @('compose', '-f', "`"$compose`"", '--profile', 'load', 'run', '--rm', '-T')
    foreach ($chave in $condicao.K6.Keys) {
        $argumentos += @('-e', "$chave=$($condicao.K6[$chave])")
    }
    $argumentos += @(
        'k6', 'run',
        '--summary-trend-stats', '"avg,min,med,p(90),p(95),p(99),max"',
        '--summary-export', "/results/$relativo/summary.json",
        '--out', "csv=/results/$relativo/k6.csv.gz",
        "/scripts/$($condicao.Script)"
    )
    $stdout = Join-Path $dirLogs 'k6.out.log'
    $stderr = Join-Path $dirLogs 'k6.err.log'
    $processo = Start-Process -FilePath 'docker' -ArgumentList ($argumentos -join ' ') -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $null = $processo.Handle

    $containerK6 = $null
    while (-not $containerK6 -and -not $processo.HasExited) {
        $containerK6 = Invoke-Docker @(
            'ps', '--no-trunc', '--filter', 'label=com.docker.compose.service=k6', '--format', '{{.ID}}'
        ) -IgnorarErro | Where-Object { "$_" -match '^[0-9a-f]{64}$' } | Select-Object -First 1
        if (-not $containerK6) { Start-Sleep -Milliseconds 500 }
    }

    $parada = $null
    if ($condicao.K6['FALHA'] -eq 'servico-parado') {
        $servico = $condicao.K6['SERVICO']
        try {
            Wait-Marcador $stderr 'FALHA_INICIO' $processo
            $paradoEm = Get-Date
            Invoke-Docker @('compose', '-f', $compose, 'stop', '-t', '0', $servico) | Out-Null
            Wait-Marcador $stderr 'FALHA_FIM' $processo
            $religadoEm = Get-Date
            Invoke-Docker @('compose', '-f', $compose, 'start', $servico) | Out-Null
            $parada = [ordered]@{
                servico            = $servico
                parado_utc         = Get-Utc $paradoEm
                religado_utc       = Get-Utc $religadoEm
                reinicio_segundos  = Wait-Saudavel "marketplace-$servico"
            }
        }
        finally {
            Invoke-Docker @('compose', '-f', $compose, 'start', $servico) -IgnorarErro | Out-Null
        }
    }

    $processo.WaitForExit()
    return [pscustomobject]@{
        Codigo      = $processo.ExitCode
        FalhaInicio = Test-Marcador $stderr 'FALHA_INICIO'
        FalhaFim    = Test-Marcador $stderr 'FALHA_FIM'
        Parada      = $parada
        Container   = $containerK6
    }
}

$consultasPrometheus = [ordered]@{
    'cpu'                     = 'sum by (id) (rate(container_cpu_usage_seconds_total{id=~"/docker/[0-9a-f]{64}"}[20s]))'
    'memoria'                 = 'max by (id) (container_memory_working_set_bytes{id=~"/docker/[0-9a-f]{64}"})'
    'fila_prontas'            = 'sum by (queue) (rabbitmq_queue_messages_ready)'
    'fila_nao_confirmadas'    = 'sum by (queue) (rabbitmq_queue_messages_unacked)'
    'mensagens_processadas'   = 'sum by (queue, status) (message_processing_duration_seconds_count)'
    'mensagens_retry'         = 'sum by (queue) (message_retry_total)'
    'mensagens_dead'          = 'sum by (queue) (message_dead_total)'
    'breaker_estado'          = 'max by (job, breaker) (circuit_breaker_state)'
    'breaker_transicoes'      = 'sum by (job, breaker, to) (circuit_breaker_transitions_total)'
    'breaker_rejeicoes'       = 'sum by (job, breaker) (circuit_breaker_rejections_total)'
    'fallback'                = 'sum by (job, source) (fallback_activations_total)'
    'tentativas_catalogo'     = 'sum by (outcome) (catalog_client_requests_total)'
    'reprocessamento_buckets' = 'sum by (le, outcome) (failure_reprocessing_duration_seconds_bucket)'
    'http_requisicoes'        = 'sum by (job, method, path, status) (http_request_duration_seconds_count)'
    'mecanismos'              = 'max by (job, mechanism) (resilience_mechanism_enabled)'
}

function Export-Prometheus([string]$dir, [DateTime]$inicio, [DateTime]$fim) {
    foreach ($nome in $consultasPrometheus.Keys) {
        $resposta = Invoke-WebRequest -UseBasicParsing -Method Get -Uri "$prometheus/api/v1/query_range" -Body @{
            query = $consultasPrometheus[$nome]
            start = Get-Utc $inicio
            end   = Get-Utc $fim
            step  = '5s'
        }
        [System.IO.File]::WriteAllText((Join-Path $dir "$nome.json"), $resposta.Content, $utf8)
    }
}

function Get-CpuMaximaK6([string]$containerK6, [DateTime]$inicio, [DateTime]$fim) {
    if (-not $containerK6) { return $null }
    $segundos = [int][Math]::Ceiling(($fim - $inicio).TotalSeconds)
    $consulta = "max(max_over_time(sum(rate(container_cpu_usage_seconds_total{id=`"/docker/$containerK6`"}[20s]))[${segundos}s:5s]))"
    $resposta = Invoke-RestMethod -Method Get -Uri "$prometheus/api/v1/query" -Body @{ query = $consulta; time = Get-Utc $fim }
    if ($resposta.data.result.Count -eq 0) { return $null }
    return [double]$resposta.data.result[0].value[1]
}

function Get-Ambiente {
    $processador = Get-CimInstance Win32_Processor | Select-Object -First 1
    $sistema = Get-CimInstance Win32_OperatingSystem
    $docker = Invoke-Docker @('info', '--format', '{{.NCPU}}|{{.MemTotal}}|{{.ServerVersion}}|{{.OperatingSystem}}|{{.KernelVersion}}') | Select-Object -Last 1
    $partes = "$docker".Split('|')
    return [ordered]@{
        processador          = $processador.Name.Trim()
        nucleos              = $processador.NumberOfCores
        threads              = $processador.NumberOfLogicalProcessors
        memoria_host_bytes   = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
        sistema              = "$($sistema.Caption) $($sistema.Version)"
        docker_cpus          = [int]$partes[0]
        docker_memoria_bytes = [long]$partes[1]
        docker_versao        = $partes[2]
        docker_so            = $partes[3]
        docker_kernel        = $partes[4]
        compose_versao       = (Invoke-Docker @('compose', 'version', '--short') | Select-Object -Last 1)
    }
}

function Get-Imagens {
    $imagens = [ordered]@{}
    foreach ($container in $containersSut) {
        $linha = Invoke-Docker @('inspect', '-f', '{{.Config.Image}}|{{.Image}}', $container) -IgnorarErro | Select-Object -Last 1
        $imagens[$container] = "$linha"
    }
    return $imagens
}

function Get-Containers {
    $ids = [ordered]@{}
    foreach ($container in $containersSut + @('marketplace-prometheus', 'marketplace-cadvisor')) {
        $id = Invoke-Docker @('inspect', '-f', '{{.Id}}', $container) -IgnorarErro | Select-Object -Last 1
        $ids[$container] = "$id".Trim()
    }
    return $ids
}

function Get-IdsComSerie([string]$arquivo) {
    if (-not (Test-Path $arquivo)) { return @() }
    $dados = Get-Content $arquivo -Raw | ConvertFrom-Json
    return @($dados.data.result | ForEach-Object { "$($_.metric.id)" -replace '^/docker/', '' })
}

function Get-Reinicios {
    $estado = [ordered]@{}
    foreach ($container in $containersSut) {
        $linha = Invoke-Docker @('inspect', '-f', '{{.RestartCount}}|{{.State.OOMKilled}}', $container) -IgnorarErro | Select-Object -Last 1
        $partes = "$linha".Split('|')
        $estado[$container] = [ordered]@{ reinicios = [int]$partes[0]; oom = ($partes[1] -eq 'true') }
    }
    return $estado
}

function Test-Execucao($dir, $k6, $drenagem, $reinicios, $cpuK6, $containers) {
    $motivos = @()
    if ($k6.Codigo -ne 0 -and $k6.Codigo -ne 99) {
        $motivos += "k6 terminou com código $($k6.Codigo)"
    }
    $resumoArquivo = Join-Path $dir 'summary.json'
    if (-not (Test-Path $resumoArquivo)) {
        $motivos += 'summary.json ausente'
    }
    else {
        $resumo = Get-Content $resumoArquivo -Raw | ConvertFrom-Json
        $iteracoes = 0
        $descartadas = 0
        if ($resumo.metrics.iterations) { $iteracoes = [double]$resumo.metrics.iterations.count }
        if ($resumo.metrics.dropped_iterations) { $descartadas = [double]$resumo.metrics.dropped_iterations.count }
        if (($iteracoes + $descartadas) -gt 0 -and ($descartadas / ($iteracoes + $descartadas)) -gt $LimiteDescarte) {
            $motivos += "iterações descartadas pelo k6: $descartadas de $($iteracoes + $descartadas)"
        }
        $controle = $resumo.metrics.'checks{tipo:controle}'
        if ($controle -and [int]$controle.fails -gt 0) {
            $motivos += "comandos de controle falharam: $($controle.fails)"
        }
    }
    if (-not $drenagem.Completa) {
        $motivos += "drenagem incompleta: $($drenagem.Pendencias.Pedidos) pedidos e $($drenagem.Pendencias.Mensagens) mensagens pendentes"
    }
    foreach ($container in $reinicios.Keys) {
        if ($reinicios[$container].reinicios -gt 0 -or $reinicios[$container].oom) {
            $motivos += "$container reiniciou ou sofreu OOM"
        }
    }
    foreach ($serie in @('cpu', 'memoria')) {
        $comSerie = Get-IdsComSerie (Join-Path $dir "prometheus\$serie.json")
        $semSerie = @($containersSut | Where-Object { $comSerie -notcontains $containers[$_] })
        if ($semSerie.Count -gt 0) {
            $motivos += "série de $serie ausente para: $($semSerie -join ', ')"
        }
    }
    $limiteK6 = 2.0
    if ($env:K6_CPUS) { $limiteK6 = [double]$env:K6_CPUS }
    if ($null -eq $cpuK6) {
        $motivos += 'CPU do k6 não foi medida'
    }
    elseif ($cpuK6 -gt 0.8 * $limiteK6) {
        $motivos += "CPU do k6 chegou a $([Math]::Round($cpuK6, 2)) núcleos (limite $limiteK6)"
    }
    return $motivos
}

function Invoke-Execucao($condicao, [int]$repeticao, [int]$posicao, $ambiente, $git) {
    $relativo = "experimentos/$Experimento/$($condicao.Nome)/rep-{0:D2}" -f $repeticao
    $dir = Join-Path $PSScriptRoot "results\$($relativo.Replace('/', '\'))"
    if (Test-Path (Join-Path $dir 'metadata.json')) {
        throw "$dir já tem uma execução concluída; apague a pasta ou use -RepeticaoInicial para continuar de outra repetição"
    }
    if (Test-Path $dir) {
        $abortada = "$dir.abortada-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
        Move-Item -Path $dir -Destination $abortada
        Write-Warning "execução anterior incompleta movida para $abortada"
    }
    $dirLogs = Join-Path $dir 'logs'
    $dirProm = Join-Path $dir 'prometheus'
    $dirSql = Join-Path $dir 'sql'
    New-Item -ItemType Directory -Force -Path $dirLogs, $dirProm, $dirSql | Out-Null

    $indice = Join-Path $saidaBase "$Experimento\execucoes.csv"
    if (-not (Test-Path $indice)) {
        Write-Texto $indice 'experimento,condicao,repeticao,posicao,inicio_utc,fim_utc,valida,motivos'
    }

    try {
        Write-Host "$(Get-Date -Format HH:mm:ss) [$Experimento] $($condicao.Nome) rep ${repeticao}: preparando ambiente"
        Reset-Ambiente $condicao
        $imagens = Get-Imagens
        $containers = Get-Containers

        $inicio = (Get-Date).ToUniversalTime()
        Write-Host "$(Get-Date -Format HH:mm:ss) [$Experimento] $($condicao.Nome) rep ${repeticao}: carga iniciada"
        $k6 = Invoke-K6 $condicao $relativo $dirLogs
        $fimCarga = (Get-Date).ToUniversalTime()
    }
    catch {
        $erro = "$($_.Exception.Message)"
        Write-Texto (Join-Path $dir 'erro.txt') $erro
        $primeiraLinha = (($erro -split "`n")[0] -replace '"', "'").Trim()
        $linha = '{0},{1},{2},{3},{4},{5},{6},"{7}"' -f $Experimento, $condicao.Nome, $repeticao, $posicao,
            (Get-Utc (Get-Date)), (Get-Utc (Get-Date)), 'False', "abortada: $primeiraLinha"
        [System.IO.File]::AppendAllText($indice, "$linha`n", $utf8)
        throw
    }

    Write-Host "$(Get-Date -Format HH:mm:ss) [$Experimento] $($condicao.Nome) rep ${repeticao}: drenando"
    $drenagem = Wait-Drenagem
    Start-Sleep -Seconds 12
    $fim = (Get-Date).ToUniversalTime()

    Export-Prometheus $dirProm $inicio.AddSeconds(-30) $fim
    $cpuK6 = Get-CpuMaximaK6 $k6.Container $inicio $fimCarga
    $containers["k6"] = $k6.Container
    Export-Sql (Join-Path $dirSql 'pedidos.csv') 'orders' 'SELECT id, status, total_cents, created_at, updated_at FROM orders'
    Export-Sql (Join-Path $dirSql 'pagamentos.csv') 'payments' 'SELECT id, order_id, status, failure_reason, created_at, updated_at FROM payments'
    Export-Sql (Join-Path $dirSql 'estoque.csv') 'inventory' 'SELECT product_id, available, reserved, updated_at FROM stock'
    Export-Sql (Join-Path $dirSql 'reservas.csv') 'inventory' 'SELECT order_id, product_id, quantity FROM stock_reservations'
    Export-Sql (Join-Path $dirSql 'resultados_reserva.csv') 'inventory' 'SELECT order_id, outcome, reason, updated_at FROM reservation_outcomes'
    Write-Texto (Join-Path $dirLogs 'servicos.log') (Invoke-Docker @('compose', '-f', $compose, 'logs', '--no-color', '--timestamps', 'catalog', 'orders', 'inventory', 'payments', 'toxiproxy') -IgnorarErro)

    $reinicios = Get-Reinicios
    $motivos = @(Test-Execucao $dir $k6 $drenagem $reinicios $cpuK6 $containers)

    $metadados = [ordered]@{
        experimento          = $Experimento
        condicao             = $condicao.Nome
        repeticao            = $repeticao
        posicao_no_bloco     = $posicao
        semente              = $Semente
        inicio_utc           = Get-Utc $inicio
        fim_carga_utc        = Get-Utc $fimCarga
        fim_utc              = Get-Utc $fim
        falha_inicio_utc     = $k6.FalhaInicio
        falha_fim_utc        = $k6.FalhaFim
        parada_servico       = $k6.Parada
        k6_script            = $condicao.Script
        k6_codigo_saida      = $k6.Codigo
        k6_parametros        = $condicao.K6
        k6_cpu_maxima        = $cpuK6
        env_servicos         = $condicao.Servicos
        drenagem_completa    = $drenagem.Completa
        drenagem_segundos    = [Math]::Round($drenagem.Segundos, 1)
        reinicios            = $reinicios
        valida               = ($motivos.Count -eq 0)
        motivos_invalidacao  = $motivos
        git                  = $git
        imagens              = $imagens
        containers           = $containers
        ambiente             = $ambiente
    }
    Write-Texto (Join-Path $dir 'metadata.json') ($metadados | ConvertTo-Json -Depth 6)

    $linha = '{0},{1},{2},{3},{4},{5},{6},"{7}"' -f $Experimento, $condicao.Nome, $repeticao, $posicao,
        (Get-Utc $inicio), (Get-Utc $fim), $metadados.valida, (($motivos -join '; ') -replace '"', "'")
    [System.IO.File]::AppendAllText($indice, "$linha`n", $utf8)

    $status = 'válida'
    if (-not $metadados.valida) { $status = "INVÁLIDA ($($motivos -join '; '))" }
    Write-Host "$(Get-Date -Format HH:mm:ss) [$Experimento] $($condicao.Nome) rep ${repeticao}: $status"
}

$todas = @(Get-Condicoes)
if ($Condicoes.Count -gt 0) {
    $todas = @($todas | Where-Object { $Condicoes -contains $_.Nome })
    if ($todas.Count -eq 0) {
        throw "nenhuma condição de $Experimento corresponde a: $($Condicoes -join ', ')"
    }
}

New-Item -ItemType Directory -Force -Path (Join-Path $saidaBase $Experimento) | Out-Null

$git = [ordered]@{
    commit = (git -C $raiz rev-parse HEAD)
    alteracoes_nao_commitadas = @(git -C $raiz status --porcelain).Count
}
if ($git.alteracoes_nao_commitadas -gt 0) {
    Write-Warning "há $($git.alteracoes_nao_commitadas) arquivos alterados sem commit; o hash registrado não descreve exatamente o código medido"
}

if (-not $SemBuild) {
    Write-Host "$(Get-Date -Format HH:mm:ss) construindo imagens"
    Invoke-Docker @('compose', '-f', $compose, 'build', 'catalog', 'orders', 'inventory', 'payments') | Out-Null
}

$ambiente = Get-Ambiente
$sorteio = New-Object System.Random($Semente)
$variaveis = @('CATALOG_TIMEOUT_ENABLED', 'CATALOG_RETRY_ENABLED', 'CATALOG_BREAKER_ENABLED', 'CATALOG_FALLBACK_ENABLED', 'PAYMENT_REPROCESS_ENABLED', 'GATEWAY_BREAKER_ENABLED')

try {
    for ($repeticao = 1; $repeticao -lt $RepeticaoInicial + $Repeticoes; $repeticao++) {
        $ordem = @($todas)
        for ($i = $ordem.Count - 1; $i -gt 0; $i--) {
            $j = $sorteio.Next($i + 1)
            $troca = $ordem[$i]; $ordem[$i] = $ordem[$j]; $ordem[$j] = $troca
        }
        if ($repeticao -lt $RepeticaoInicial) { continue }

        $posicao = 0
        foreach ($condicao in $ordem) {
            $posicao++
            Invoke-Execucao $condicao $repeticao $posicao $ambiente $git
        }
    }
}
finally {
    foreach ($variavel in $variaveis) {
        Remove-Item -Path "env:$variavel" -ErrorAction SilentlyContinue
    }
}
