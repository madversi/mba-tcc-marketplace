param(
    [string]$Resultados = (Join-Path $PSScriptRoot '..\load-tests\results\experimentos'),
    [string]$Saida = $PSScriptRoot,
    [string[]]$Experimentos = @('1A', '1B', '2A', '2B', '3A', '4A'),
    [long]$EstoqueInicial = 1000000,
    [int]$JanelaSegundos = 5,
    [int]$RecuperacaoK = 3,
    [double]$RecuperacaoErroMax = 0.01,
    [double]$RecuperacaoFatorP95 = 1.2
)

$ErrorActionPreference = 'Stop'
$Resultados = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Resultados)
$Saida = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Saida)
$inv = [Globalization.CultureInfo]::InvariantCulture
[Threading.Thread]::CurrentThread.CurrentCulture = $inv
$utf8 = New-Object Text.UTF8Encoding($false)
if (-not ('Analise' -as [type])) {
    Add-Type -TypeDefinition ([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'lib\Analise.cs'))) -Language CSharp
}

$servicos = @('postgres', 'rabbitmq', 'toxiproxy', 'catalog', 'orders', 'inventory', 'payments', 'k6')
$terminais = @('CONFIRMED', 'CANCELLED')

function F($x) {
    if ($null -eq $x) { return '' }
    if ($x -is [double] -or $x -is [single] -or $x -is [decimal]) {
        $d = [double]$x
        if ([double]::IsNaN($d) -or [double]::IsInfinity($d)) { return '' }
        return $d.ToString('0.######', $inv)
    }
    if ($x -is [bool]) { return $x.ToString().ToLower() }
    return "$x"
}

function Pct($parte, $total) {
    if ($total -le 0) { return [double]::NaN }
    return 100.0 * $parte / $total
}

function Get-Segundos([string]$d) {
    $total = 0.0
    foreach ($m in [regex]::Matches($d, '(\d+)(ms|s|m|h)')) {
        $n = [double]$m.Groups[1].Value
        switch ($m.Groups[2].Value) {
            'ms' { $total += $n / 1000 }
            's' { $total += $n }
            'm' { $total += 60 * $n }
            'h' { $total += 3600 * $n }
        }
    }
    return $total
}

function U($iso) {
    if (-not $iso) { return [double]::NaN }
    return [Analise]::ParseTs("$iso")
}

function Write-Csv([string]$caminho, $linhas) {
    New-Item -ItemType Directory -Force -Path (Split-Path $caminho) | Out-Null
    if (@($linhas).Count -eq 0) {
        [IO.File]::WriteAllText($caminho, '', $utf8)
        return
    }
    $texto = @($linhas) | ConvertTo-Csv -NoTypeInformation
    [IO.File]::WriteAllLines($caminho, [string[]]$texto, $utf8)
}

function Read-Prom([string]$arquivo) {
    $saida = New-Object Collections.ArrayList
    if (-not (Test-Path $arquivo)) { return , $saida }
    $j = [IO.File]::ReadAllText($arquivo) | ConvertFrom-Json
    foreach ($r in $j.data.result) {
        $n = @($r.values).Count
        $t = New-Object 'double[]' $n
        $v = New-Object 'double[]' $n
        for ($i = 0; $i -lt $n; $i++) {
            $t[$i] = [double]$r.values[$i][0]
            $v[$i] = [Analise]::Num("$($r.values[$i][1])")
        }
        [void]$saida.Add([pscustomobject]@{ Rotulos = $r.metric; T = $t; V = $v })
    }
    return , $saida
}

function Get-Aumento($series, [scriptblock]$filtro, [double]$ini, [double]$fim) {
    $total = 0.0
    foreach ($s in $series) {
        if (& $filtro $s.Rotulos) { $total += [Analise]::Aumento($s.T, $s.V, $ini, $fim) }
    }
    return $total
}

function Get-SerieSomada($listas, [scriptblock]$filtro) {
    $soma = @{}
    foreach ($series in $listas) {
        foreach ($s in $series) {
            if (-not (& $filtro $s.Rotulos)) { continue }
            for ($i = 0; $i -lt $s.T.Length; $i++) {
                if ([double]::IsNaN($s.V[$i])) { continue }
                $k = $s.T[$i]
                if ($soma.ContainsKey($k)) { $soma[$k] += $s.V[$i] } else { $soma[$k] = $s.V[$i] }
            }
        }
    }
    $chaves = [double[]]@($soma.Keys | Sort-Object)
    $valores = New-Object 'double[]' $chaves.Length
    for ($i = 0; $i -lt $chaves.Length; $i++) { $valores[$i] = $soma[$chaves[$i]] }
    return [pscustomobject]@{ T = $chaves; V = $valores }
}

function Get-Logs([string]$arquivo) {
    $eventos = New-Object Collections.ArrayList
    if (-not (Test-Path $arquivo)) { return , $eventos }
    $padroes = @(
        'transição do circuit breaker', 'usando produto do cache de fallback', 'pagamento reprocessado',
        'reprocessamento esgotado', 'enviado para reprocessamento', 'reprocessamento desligado'
    )
    foreach ($m in (Select-String -Path $arquivo -SimpleMatch -Pattern $padroes -Encoding UTF8)) {
        $r = [regex]::Match($m.Line, '^(\S+)\s+\|\s+(\S+)\s+(\{.*\})\s*$')
        if (-not $r.Success) { continue }
        $j = $r.Groups[3].Value | ConvertFrom-Json
        $t = U $j.timestamp
        if ([double]::IsNaN($t)) { $t = U $r.Groups[2].Value }
        [void]$eventos.Add([pscustomobject]@{
                Container = $r.Groups[1].Value
                T         = $t
                Mensagem  = "$($j.fields.message)"
                Campos    = $j.fields
            })
    }
    return , $eventos
}

function Get-Transicoes($eventos, [string]$container, [string]$breaker) {
    return @($eventos | Where-Object {
            $_.Container -eq $container -and $_.Mensagem -eq 'transição do circuit breaker' -and "$($_.Campos.breaker)" -eq $breaker
        } | Sort-Object T | ForEach-Object { [pscustomobject]@{ T = $_.T; Para = "$($_.Campos.to)" } })
}

function Get-EstadoIntervalo($transicoes, [double]$ini, [double]$fim) {
    $codigo = @{ closed = 0; half_open = 1; open = 2 }
    $estado = 0
    foreach ($tr in $transicoes) { if ($tr.T -le $ini) { $estado = $codigo[$tr.Para] } }
    $maximo = $estado
    $aberto = 0.0
    $desde = $ini
    foreach ($tr in $transicoes) {
        if ($tr.T -le $ini -or $tr.T -ge $fim) { continue }
        if ($estado -eq 2) { $aberto += $tr.T - $desde }
        $estado = $codigo[$tr.Para]
        $desde = $tr.T
        if ($estado -gt $maximo) { $maximo = $estado }
    }
    if ($estado -eq 2) { $aberto += $fim - $desde }
    return [pscustomobject]@{ Max = $maximo; Aberto = $aberto }
}

function Get-Fases($meta, $k6, $pedidos) {
    $p = $meta.k6_parametros
    $aq = Get-Segundos $p.AQUECIMENTO
    $inicioFalha = U $meta.falha_inicio_utc
    $fimFalha = U $meta.falha_fim_utc
    if ($meta.k6_script -eq 'baseline.js') {
        $plano = @(, @('aquecimento', $aq)) + @(, @('medicao', (Get-Segundos $p.DURATION)))
        $criados = @($pedidos | ForEach-Object { $_.Criado })
        if ($criados.Count -gt 0) { $t0 = ($criados | Measure-Object -Minimum).Minimum; $origem = 'primeiro pedido criado (SQL)' }
        else { $t0 = [double][Analise]::MinTs($k6); $origem = 'primeira requisição do k6' }
    }
    else {
        $antes = Get-Segundos $p.WARMUP
        $plano = @(, @('aquecimento', $aq)) + @(, @('antes', $antes)) + @(, @('falha', (Get-Segundos $p.FAILURE_WINDOW))) + @(, @('depois', (Get-Segundos $p.RECOVERY)))
        if (-not [double]::IsNaN($inicioFalha)) { $t0 = $inicioFalha - $aq - $antes; $origem = 'marcador FALHA_INICIO' }
        else { $t0 = [double][Analise]::MinTs($k6); $origem = 'primeira requisição do k6' }
    }
    $fases = @()
    $ini = $t0
    foreach ($e in $plano) {
        $fases += [pscustomobject]@{ Nome = $e[0]; Ini = $ini; Fim = $ini + $e[1] }
        $ini += $e[1]
    }
    if (-not [double]::IsNaN($inicioFalha) -and -not [double]::IsNaN($fimFalha)) {
        foreach ($f in $fases) {
            if ($f.Nome -eq 'antes') { $f.Fim = $inicioFalha }
            if ($f.Nome -eq 'falha') { $f.Ini = $inicioFalha; $f.Fim = $fimFalha }
            if ($f.Nome -eq 'depois') { $f.Ini = $fimFalha }
        }
    }
    return [pscustomobject]@{ Fases = $fases; T0 = $t0; Origem = $origem }
}

function Get-FaseDe([double]$t, $fases) {
    if ($t -lt $fases[0].Ini) { return $fases[0].Nome }
    foreach ($f in $fases) { if ($t -lt $f.Fim) { return $f.Nome } }
    return $fases[-1].Nome
}

function Get-Invariantes($dir, $pedidos) {
    $estoque = @(Import-Csv (Join-Path $dir 'sql\estoque.csv'))
    $reservas = @(Import-Csv (Join-Path $dir 'sql\reservas.csv'))
    $pagamentos = @(Import-Csv (Join-Path $dir 'sql\pagamentos.csv'))
    $reservado = 0L
    $vendido = 0L
    foreach ($e in $estoque) {
        $reservado += [long]$e.reserved
        $vendido += $EstoqueInicial - [long]$e.available - [long]$e.reserved
    }
    $aprovados = @{}
    foreach ($p in $pagamentos) { if ($p.status -eq 'APPROVED') { $aprovados[$p.order_id] = $true } }
    $confirmados = @($pedidos | Where-Object Status -eq 'CONFIRMED')
    $semPagamento = @($confirmados | Where-Object { -not $aprovados.ContainsKey($_.Id) }).Count
    $pendentes = @($pagamentos | Where-Object status -eq 'PENDING').Count
    $i2 = $vendido - $confirmados.Count
    $violados = 0
    if ($reservas.Count -gt 0 -or $reservado -ne 0) { $violados++ }
    if ($i2 -ne 0) { $violados++ }
    if ($pendentes -ne 0) { $violados++ }
    if ($semPagamento -ne 0) { $violados++ }
    return [pscustomobject]@{
        Produtos        = $estoque.Count
        ReservasLinhas  = $reservas.Count
        ReservadoSoma   = $reservado
        Vendido         = $vendido
        Confirmados     = $confirmados.Count
        I2Diferenca     = $i2
        PendentesResid  = $pendentes
        ConfSemPagto    = $semPagamento
        Violados        = $violados
        Pagamentos      = $pagamentos
    }
}

function Invoke-Execucao([string]$exp, $dir) {
    $meta = Get-Content (Join-Path $dir 'metadata.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $k6 = [Analise]::LerK6((Join-Path $dir 'k6.csv.gz'))
    $pedidos = @(Import-Csv (Join-Path $dir 'sql\pedidos.csv') | ForEach-Object {
            [pscustomobject]@{ Id = $_.id; Status = $_.status; Criado = [Analise]::ParseTs($_.created_at); Atualizado = [Analise]::ParseTs($_.updated_at); Fase = '' }
        })
    $fasesInfo = Get-Fases $meta $k6 $pedidos
    $fases = $fasesInfo.Fases
    foreach ($p in $pedidos) { $p.Fase = Get-FaseDe $p.Criado $fases }
    $inv4 = Get-Invariantes $dir $pedidos
    $pagPorId = @{}
    foreach ($p in $inv4.Pagamentos) { $pagPorId[$p.id] = $p }

    $nomePorId = @{}
    foreach ($prop in $meta.containers.PSObject.Properties) {
        if ($prop.Value) { $nomePorId["$($prop.Value)"] = ($prop.Name -replace '^marketplace-', '') }
    }
    $prom = @{}
    foreach ($nome in @('cpu', 'memoria', 'fila_prontas', 'fila_nao_confirmadas', 'mensagens_processadas', 'mensagens_retry',
            'mensagens_dead', 'breaker_rejeicoes', 'fallback', 'tentativas_catalogo')) {
        $prom[$nome] = Read-Prom (Join-Path $dir "prometheus\$nome.json")
    }
    $fila = Get-SerieSomada @($prom['fila_prontas'], $prom['fila_nao_confirmadas']) { param($r) -not "$($r.queue)".EndsWith('.dead') }

    $eventos = Get-Logs (Join-Path $dir 'logs\servicos.log')
    $cbCatalogo = Get-Transicoes $eventos 'marketplace-orders' 'catalog'
    $cbGateway = Get-Transicoes $eventos 'marketplace-payments' 'payment_gateway'

    $p = $meta.k6_parametros
    $taxa = [double]$p.RATE
    $inicioFalha = U $meta.falha_inicio_utc
    $fimFalha = U $meta.falha_fim_utc
    $fimExec = U $meta.fim_utc
    $base = [ordered]@{
        experimento = $exp
        condicao    = $meta.condicao
        repeticao   = $meta.repeticao
        posicao     = $meta.posicao_no_bloco
        valida      = F ([bool]$meta.valida)
    }

    $linhas = @()
    $resumosFase = @{}
    foreach ($f in $fases) {
        $r = [Analise]::ResumirFase($k6, $f.Nome)
        $resumosFase[$f.Nome] = $r
        $seg = $f.Fim - $f.Ini
        $erros = $r.N - $r.N201
        $doFase = @($pedidos | Where-Object Fase -eq $f.Nome)
        $saga = New-Object 'Collections.Generic.List[double]'
        foreach ($q in $doFase) { if ($terminais -contains $q.Status) { $saga.Add(1000.0 * ($q.Atualizado - $q.Criado)) } }
        $conf = @($doFase | Where-Object Status -eq 'CONFIRMED').Count
        $canc = @($doFase | Where-Object Status -eq 'CANCELLED').Count
        $tentativas = Get-Aumento $prom['tentativas_catalogo'] { $true } $f.Ini $f.Fim
        $fallback = Get-Aumento $prom['fallback'] { $true } $f.Ini $f.Fim

        $l = [ordered]@{}
        foreach ($k in $base.Keys) { $l[$k] = $base[$k] }
        $l['fase'] = $f.Nome
        $l['fase_ini_utc'] = [DateTimeOffset]::FromUnixTimeMilliseconds([long]($f.Ini * 1000)).ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
        $l['fase_segundos'] = F $seg
        $l['taxa_ofertada_rps'] = F $taxa
        $l['post_n'] = $r.N
        $l['post_201'] = $r.N201
        $l['erro_pct'] = F (Pct $erros $r.N)
        $l['erro_503'] = $r.N503
        $l['erro_504'] = $r.N504
        $l['erro_conexao'] = $r.NConexao
        $l['erro_outros'] = $r.NOutros
        $l['lat_p50_ms'] = F $r.P50
        $l['lat_p95_ms'] = F $r.P95
        $l['lat_p99_ms'] = F $r.P99
        $l['lat_max_ms'] = F $r.Max
        $l['vazao_efetiva_rps'] = F ($r.N201 / $seg)
        $l['iteracoes_descartadas'] = [Analise]::DescartadasEntre($k6, $f.Ini, $f.Fim)
        $l['pedidos_criados'] = $doFase.Count
        $l['pedidos_confirmados'] = $conf
        $l['pedidos_cancelados'] = $canc
        $l['pedidos_nao_terminais'] = $doFase.Count - $conf - $canc
        $l['confirmados_pct'] = F (Pct $conf $doFase.Count)
        $l['sagas_concluidas_pct'] = F (Pct ($conf + $canc) $doFase.Count)
        $l['saga_p50_ms'] = F ([Analise]::Percentil($saga, 0.50))
        $l['saga_p95_ms'] = F ([Analise]::Percentil($saga, 0.95))
        $l['saga_max_ms'] = F ([Analise]::Percentil($saga, 1.0))
        foreach ($svc in $servicos) {
            $cpu = $null; $mem = $null
            foreach ($s in $prom['cpu']) { if ($nomePorId["$("$($s.Rotulos.id)" -replace '^/docker/', '')"] -eq $svc) { $cpu = [Analise]::NoIntervalo($s.T, $s.V, $f.Ini, $f.Fim) } }
            foreach ($s in $prom['memoria']) { if ($nomePorId["$("$($s.Rotulos.id)" -replace '^/docker/', '')"] -eq $svc) { $mem = [Analise]::NoIntervalo($s.T, $s.V, $f.Ini, $f.Fim) } }
            $l["cpu_media_$svc"] = if ($cpu) { F $cpu.Media } else { '' }
            $l["cpu_max_$svc"] = if ($cpu) { F $cpu.Max } else { '' }
            $l["mem_media_mb_$svc"] = if ($mem) { F ($mem.Media / 1MB) } else { '' }
            $l["mem_max_mb_$svc"] = if ($mem) { F ($mem.Max / 1MB) } else { '' }
        }
        $filaFase = [Analise]::NoIntervalo($fila.T, $fila.V, $f.Ini, $f.Fim)
        $l['fila_media'] = F $filaFase.Media
        $l['fila_max'] = F $filaFase.Max
        $l['fila_media_3_primeiras'] = F ([Analise]::MediaPrimeirasUltimas($fila.T, $fila.V, $f.Ini, $f.Fim, 3, $false))
        $l['fila_media_3_ultimas'] = F ([Analise]::MediaPrimeirasUltimas($fila.T, $fila.V, $f.Ini, $f.Fim, 3, $true))
        $l['msg_processadas_ok'] = F (Get-Aumento $prom['mensagens_processadas'] { param($r) $r.status -eq 'ok' } $f.Ini $f.Fim)
        $l['msg_processadas_erro'] = F (Get-Aumento $prom['mensagens_processadas'] { param($r) $r.status -eq 'error' } $f.Ini $f.Fim)
        $l['msg_reentregues'] = F (Get-Aumento $prom['mensagens_retry'] { $true } $f.Ini $f.Fim)
        $l['msg_mortas'] = F (Get-Aumento $prom['mensagens_dead'] { $true } $f.Ini $f.Fim)
        $l['fallback_ativacoes'] = F $fallback
        $l['fallback_pct_201'] = F (Pct $fallback $r.N201)
        $l['cb_catalogo_rejeicoes'] = F (Get-Aumento $prom['breaker_rejeicoes'] { param($r) $r.breaker -eq 'catalog' } $f.Ini $f.Fim)
        $l['cb_gateway_rejeicoes'] = F (Get-Aumento $prom['breaker_rejeicoes'] { param($r) $r.breaker -eq 'payment_gateway' } $f.Ini $f.Fim)
        $l['catalogo_tentativas'] = F $tentativas
        foreach ($o in @('ok', 'not_found', 'timeout', 'connect_error', 'server_error', 'rejected', 'other_error')) {
            $l["catalogo_tentativas_$o"] = F (Get-Aumento $prom['tentativas_catalogo'] ([scriptblock]::Create("param(`$r) `$r.outcome -eq '$o'")) $f.Ini $f.Fim)
        }
        $l['fator_amplificacao'] = if ($r.N -gt 0) { F ($tentativas / $r.N) } else { '' }
        $cbFase = Get-EstadoIntervalo $cbCatalogo $f.Ini $f.Fim
        $l['cb_catalogo_aberto_s'] = F $cbFase.Aberto
        $linhas += [pscustomobject]$l
    }

    $janelas = @()
    $tRef = if (-not [double]::IsNaN($inicioFalha)) { $inicioFalha } else { ($fases | Where-Object Nome -eq 'medicao').Ini }
    $kIni = [int][Math]::Floor(($fasesInfo.T0 - $tRef) / $JanelaSegundos)
    $kFim = [int][Math]::Ceiling(($fimExec - $tRef) / $JanelaSegundos)
    $paradoEm = [double]::NaN; $religadoEm = [double]::NaN
    if ($meta.parada_servico) { $paradoEm = U $meta.parada_servico.parado_utc; $religadoEm = U $meta.parada_servico.religado_utc }
    $concluidasPorJanela = @{}
    $confirmadasPorJanela = @{}
    foreach ($q in $pedidos) {
        if ($terminais -notcontains $q.Status) { continue }
        $kq = [int][Math]::Floor(($q.Atualizado - $tRef) / $JanelaSegundos)
        $concluidasPorJanela[$kq] = 1 + [int]$concluidasPorJanela[$kq]
        if ($q.Status -eq 'CONFIRMED') { $confirmadasPorJanela[$kq] = 1 + [int]$confirmadasPorJanela[$kq] }
    }
    for ($k = $kIni; $k -lt $kFim; $k++) {
        $ini = $tRef + $k * $JanelaSegundos
        $fim = $ini + $JanelaSegundos
        $r = [Analise]::ResumirIntervalo($k6, $ini, $fim)
        $cb = Get-EstadoIntervalo $cbCatalogo $ini $fim
        $gw = Get-EstadoIntervalo $cbGateway $ini $fim
        $filaJ = [Analise]::NoIntervalo($fila.T, $fila.V, $ini, $fim)
        $concluidas = [int]$concluidasPorJanela[$k]
        $confirmadas = [int]$confirmadasPorJanela[$k]
        $marcas = @()
        if ($paradoEm -ge $ini -and $paradoEm -lt $fim) { $marcas += 'parada' }
        if ($religadoEm -ge $ini -and $religadoEm -lt $fim) { $marcas += 'retorno' }
        if ($inicioFalha -ge $ini -and $inicioFalha -lt $fim) { $marcas += 'inicio_falha' }
        if ($fimFalha -ge $ini -and $fimFalha -lt $fim) { $marcas += 'fim_falha' }
        $j = [ordered]@{}
        foreach ($key in $base.Keys) { $j[$key] = $base[$key] }
        $j['t_rel_s'] = $k * $JanelaSegundos
        $j['fase'] = Get-FaseDe $ini $fases
        $j['post_n'] = $r.N
        $j['erro_pct'] = F (Pct ($r.N - $r.N201) $r.N)
        $j['erro_503'] = $r.N503
        $j['erro_504'] = $r.N504
        $j['erro_conexao'] = $r.NConexao
        $j['lat_p50_ms'] = F $r.P50
        $j['lat_p95_ms'] = F $r.P95
        $j['fila_media'] = F $filaJ.Media
        $j['sagas_concluidas'] = $concluidas
        $j['sagas_confirmadas'] = $confirmadas
        $j['cb_catalogo_estado_max'] = $cb.Max
        $j['cb_catalogo_aberto_s'] = F $cb.Aberto
        $j['cb_gateway_estado_max'] = $gw.Max
        $j['marcadores'] = $marcas -join ';'
        $janelas += [pscustomobject]$j
    }

    $recuperacao = [double]::NaN
    $recuperou = ''
    if (-not [double]::IsNaN($fimFalha) -and $resumosFase.ContainsKey('antes')) {
        $p95Ref = $resumosFase['antes'].P95
        $kInicio = [int][Math]::Ceiling(($fimFalha - $tRef) / $JanelaSegundos)
        $posteriores = @($janelas | Where-Object { $_.t_rel_s -ge $kInicio * $JanelaSegundos })
        $recuperou = 'false'
        for ($i = 0; $i -le $posteriores.Count - $RecuperacaoK; $i++) {
            $ok = $true
            for ($m = $i; $m -lt $i + $RecuperacaoK; $m++) {
                $w = $posteriores[$m]
                if ($w.post_n -eq 0 -or [double]$w.erro_pct -gt 100 * $RecuperacaoErroMax -or [double]$w.lat_p95_ms -gt $RecuperacaoFatorP95 * $p95Ref) { $ok = $false; break }
            }
            if ($ok) {
                $recuperacao = [Math]::Max(0.0, ($tRef + $posteriores[$i].t_rel_s) - $fimFalha)
                $recuperou = 'true'
                break
            }
        }
    }

    $reproc = New-Object 'Collections.Generic.List[double]'
    $idsReproc = @($eventos | Where-Object Mensagem -eq 'pagamento reprocessado' | ForEach-Object { "$($_.Campos.payment_id)" } | Sort-Object -Unique)
    foreach ($id in $idsReproc) {
        $pg = $pagPorId[$id]
        if ($pg) { $reproc.Add([Analise]::ParseTs($pg.updated_at) - [Analise]::ParseTs($pg.created_at)) }
    }
    $pendentesEnviados = @($eventos | Where-Object Mensagem -eq 'pagamento pendente; enviado para reprocessamento' | ForEach-Object { "$($_.Campos.payment_id)" } | Sort-Object -Unique).Count
    $esgotados = @($eventos | Where-Object Mensagem -eq 'reprocessamento esgotado; pagamento falhou').Count
    $falhouSemReproc = @($eventos | Where-Object Mensagem -eq 'reprocessamento desligado; pagamento falhou').Count
    $fallbackLogs = @($eventos | Where-Object Mensagem -eq 'usando produto do cache de fallback').Count

    $cbTotal = Get-EstadoIntervalo $cbCatalogo $fasesInfo.T0 $fimExec
    $gwTotal = Get-EstadoIntervalo $cbGateway $fasesInfo.T0 $fimExec
    $primeiraAbertura = [double]::NaN
    if (-not [double]::IsNaN($inicioFalha)) {
        $a = @($cbCatalogo | Where-Object { $_.Para -eq 'open' -and $_.T -ge $inicioFalha } | Select-Object -First 1)
        if ($a.Count -gt 0) { $primeiraAbertura = $a[0].T - $inicioFalha }
    }
    $gwPrimeira = [double]::NaN
    if (-not [double]::IsNaN($inicioFalha)) {
        $a = @($cbGateway | Where-Object { $_.Para -eq 'open' -and $_.T -ge $inicioFalha } | Select-Object -First 1)
        if ($a.Count -gt 0) { $gwPrimeira = $a[0].T - $inicioFalha }
    }

    $e = [ordered]@{}
    foreach ($key in $base.Keys) { $e[$key] = $base[$key] }
    $e['fase'] = 'execucao'
    $e['t0_origem'] = $fasesInfo.Origem
    $e['k6_cpu_maxima'] = F $meta.k6_cpu_maxima
    $e['iteracoes_descartadas'] = $k6.Descartadas.Count
    $e['drenagem_completa'] = F ([bool]$meta.drenagem_completa)
    $e['drenagem_segundos'] = F $meta.drenagem_segundos
    $e['pedidos_criados'] = $pedidos.Count
    $e['pedidos_confirmados'] = @($pedidos | Where-Object Status -eq 'CONFIRMED').Count
    $e['pedidos_cancelados'] = @($pedidos | Where-Object Status -eq 'CANCELLED').Count
    $e['pedidos_nao_terminais'] = @($pedidos | Where-Object { $terminais -notcontains $_.Status }).Count
    $e['inv_i1_reservas_residuais'] = $inv4.ReservasLinhas
    $e['inv_i1_reservado_soma'] = $inv4.ReservadoSoma
    $e['inv_i2_vendido'] = $inv4.Vendido
    $e['inv_i2_confirmados'] = $inv4.Confirmados
    $e['inv_i2_diferenca'] = $inv4.I2Diferenca
    $e['inv_i3_pagamentos_pendentes'] = $inv4.PendentesResid
    $e['inv_i4_confirmados_sem_pagamento'] = $inv4.ConfSemPagto
    $e['invariantes_violados'] = $inv4.Violados
    $e['cb_catalogo_primeira_abertura_s'] = F $primeiraAbertura
    $e['cb_catalogo_transicoes'] = $cbCatalogo.Count
    $e['cb_catalogo_aberturas'] = @($cbCatalogo | Where-Object Para -eq 'open').Count
    $e['cb_catalogo_aberto_s'] = F $cbTotal.Aberto
    $e['cb_gateway_primeira_abertura_s'] = F $gwPrimeira
    $e['cb_gateway_transicoes'] = $cbGateway.Count
    $e['cb_gateway_aberto_s'] = F $gwTotal.Aberto
    $e['fallback_logs'] = $fallbackLogs
    $e['msg_processadas_ok'] = F (Get-Aumento $prom['mensagens_processadas'] { param($r) $r.status -eq 'ok' } $fasesInfo.T0 $fimExec)
    $e['msg_processadas_erro'] = F (Get-Aumento $prom['mensagens_processadas'] { param($r) $r.status -eq 'error' } $fasesInfo.T0 $fimExec)
    $e['msg_reentregues'] = F (Get-Aumento $prom['mensagens_retry'] { $true } $fasesInfo.T0 $fimExec)
    $e['msg_mortas'] = F (Get-Aumento $prom['mensagens_dead'] { $true } $fasesInfo.T0 $fimExec)
    $e['fallback_ativacoes'] = F (Get-Aumento $prom['fallback'] { $true } $fasesInfo.T0 $fimExec)
    $e['catalogo_tentativas'] = F (Get-Aumento $prom['tentativas_catalogo'] { $true } $fasesInfo.T0 $fimExec)
    $e['cb_catalogo_rejeicoes'] = F (Get-Aumento $prom['breaker_rejeicoes'] { param($r) $r.breaker -eq 'catalog' } $fasesInfo.T0 $fimExec)
    $e['cb_gateway_rejeicoes'] = F (Get-Aumento $prom['breaker_rejeicoes'] { param($r) $r.breaker -eq 'payment_gateway' } $fasesInfo.T0 $fimExec)
    $e['post_n'] = $k6.Posts.Count
    $e['fator_amplificacao'] = if ($k6.Posts.Count -gt 0) { F ((Get-Aumento $prom['tentativas_catalogo'] { $true } $fasesInfo.T0 $fimExec) / $k6.Posts.Count) } else { '' }
    $e['fila_max'] = F ([Analise]::NoIntervalo($fila.T, $fila.V, $fasesInfo.T0, $fimExec)).Max
    $e['recuperacao_s'] = F $recuperacao
    $e['recuperou'] = $recuperou
    $e['pagamentos_enviados_reprocessamento'] = $pendentesEnviados
    $e['pagamentos_reprocessados'] = $reproc.Count
    $e['pagamentos_reprocessamento_esgotado'] = $esgotados
    $e['pagamentos_falha_sem_reprocessamento'] = $falhouSemReproc
    $e['reprocessamento_p50_s'] = F ([Analise]::Percentil($reproc, 0.50))
    $e['reprocessamento_p95_s'] = F ([Analise]::Percentil($reproc, 0.95))
    $e['reprocessamento_max_s'] = F ([Analise]::Percentil($reproc, 1.0))
    if ($meta.parada_servico) {
        $e['parada_servico'] = $meta.parada_servico.servico
        $e['parada_segundos'] = F ($religadoEm - $paradoEm)
        $e['reinicio_saudavel_s'] = F $meta.parada_servico.reinicio_segundos
    }
    $linhaExec = [pscustomobject]$e

    $porPedido = @()
    if (@('3A', '4A') -contains $exp) {
        foreach ($q in $pedidos) {
            $o = [ordered]@{}
            foreach ($key in $base.Keys) { $o[$key] = $base[$key] }
            $o['coorte'] = $q.Fase
            $o['criado_rel_s'] = F ($q.Criado - $tRef)
            $o['status'] = $q.Status
            $o['saga_ms'] = if ($terminais -contains $q.Status) { F (1000.0 * ($q.Atualizado - $q.Criado)) } else { '' }
            $porPedido += [pscustomobject]$o
        }
    }

    return [pscustomobject]@{ Fases = $linhas; Execucao = $linhaExec; Janelas = $janelas; Pedidos = $porPedido; Meta = $meta }
}

$todasExecucoes = @()
foreach ($exp in $Experimentos) {
    $dirExp = Join-Path $Resultados $exp
    if (-not (Test-Path $dirExp)) { Write-Host "[$exp] sem resultados em $dirExp"; continue }
    $indice = Join-Path $dirExp 'execucoes.csv'
    $linhasIndice = @()
    if (Test-Path $indice) { $linhasIndice = @(Import-Csv $indice -Encoding UTF8) }

    $fases = @(); $execs = @(); $janelas = @(); $pedidos = @()
    $dirs = @(Get-ChildItem -Path $dirExp -Recurse -Filter 'metadata.json' | ForEach-Object { $_.Directory } | Sort-Object FullName)
    foreach ($d in $dirs) {
        Write-Host "$(Get-Date -Format HH:mm:ss) [$exp] $($d.Parent.Name)/$($d.Name)"
        $r = Invoke-Execucao $exp $d.FullName
        $fases += $r.Fases
        $execs += $r.Execucao
        $janelas += $r.Janelas
        $pedidos += $r.Pedidos
    }
    $consolidado = @()
    foreach ($linha in $fases + $execs) { $consolidado += $linha }
    $colunas = New-Object Collections.Generic.List[string]
    foreach ($linha in $consolidado) { foreach ($n in $linha.PSObject.Properties.Name) { if (-not $colunas.Contains($n)) { $colunas.Add($n) } } }
    $ordemFase = @{ aquecimento = 0; medicao = 1; antes = 1; falha = 2; depois = 3; execucao = 9 }
    $consolidado = @($consolidado | Sort-Object condicao, @{ e = { [int]$_.repeticao } }, @{ e = { $ordemFase[$_.fase] } } | Select-Object -Property ([string[]]$colunas))
    Write-Csv (Join-Path $Saida "consolidado\$exp.csv") $consolidado
    Write-Csv (Join-Path $Saida "janelas\$exp.csv") $janelas
    if ($pedidos.Count -gt 0) { Write-Csv (Join-Path $Saida "pedidos\$exp.csv") $pedidos }

    foreach ($li in $linhasIndice) {
        $dirRep = Join-Path $dirExp ("{0}\rep-{1:D2}" -f $li.condicao, [int]$li.repeticao)
        $meta = $null
        if ((Test-Path (Join-Path $dirRep 'metadata.json')) -and $li.motivos -notlike 'abortada:*') {
            $meta = Get-Content (Join-Path $dirRep 'metadata.json') -Raw -Encoding UTF8 | ConvertFrom-Json
        }
        $todasExecucoes += [pscustomobject][ordered]@{
            experimento       = $exp
            condicao          = $li.condicao
            repeticao         = $li.repeticao
            posicao           = $li.posicao
            inicio_utc        = $li.inicio_utc
            fim_utc           = $li.fim_utc
            valida            = "$($li.valida)".ToLower()
            motivo_descarte   = $li.motivos
            k6_cpu_maxima     = if ($meta) { F $meta.k6_cpu_maxima } else { '' }
            drenagem_segundos = if ($meta) { F $meta.drenagem_segundos } else { '' }
            commit            = if ($meta) { $meta.git.commit } else { '' }
            alteracoes_nao_commitadas = if ($meta) { $meta.git.alteracoes_nao_commitadas } else { '' }
        }
    }
}
Write-Csv (Join-Path $Saida 'execucoes.csv') $todasExecucoes
Write-Host "$(Get-Date -Format HH:mm:ss) extração concluída em $Saida"
