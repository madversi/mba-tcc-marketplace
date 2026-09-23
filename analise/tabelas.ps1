param(
    [string]$Base = $PSScriptRoot
)

$ErrorActionPreference = 'Stop'
$Base = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Base)
$inv = [Globalization.CultureInfo]::InvariantCulture
[Threading.Thread]::CurrentThread.CurrentCulture = $inv
$utf8 = New-Object Text.UTF8Encoding($false)
if (-not ('Analise' -as [type])) {
    Add-Type -TypeDefinition ([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'lib\Analise.cs'))) -Language CSharp
}

function F($x) {
    if ($null -eq $x) { return '' }
    $d = [double]$x
    if ([double]::IsNaN($d) -or [double]::IsInfinity($d)) { return '' }
    return $d.ToString('0.######', $inv)
}

function Q($valores, [double]$p) {
    $l = New-Object 'Collections.Generic.List[double]'
    foreach ($v in $valores) { if ("$v" -ne '') { $l.Add([double]::Parse("$v", $inv)) } }
    return [Analise]::Percentil($l, $p)
}

function Read-Csv([string]$caminho) {
    if (-not (Test-Path $caminho) -or (Get-Item $caminho).Length -eq 0) { return @() }
    return @(Import-Csv $caminho -Encoding UTF8)
}

function Write-Csv([string]$caminho, $linhas) {
    New-Item -ItemType Directory -Force -Path (Split-Path $caminho) | Out-Null
    if (@($linhas).Count -eq 0) { [IO.File]::WriteAllText($caminho, '', $utf8); return }
    $texto = @($linhas) | ConvertTo-Csv -NoTypeInformation
    [IO.File]::WriteAllLines($caminho, [string[]]$texto, $utf8)
}

function Validas($linhas) { return @($linhas | Where-Object valida -eq 'true') }

function Add-Resumo($linha, [string]$nome, $valores, [switch]$ComIqr) {
    $linha["${nome}_mediana"] = F (Q $valores 0.5)
    if ($ComIqr) {
        $linha["${nome}_q1"] = F (Q $valores 0.25)
        $linha["${nome}_q3"] = F (Q $valores 0.75)
    }
}

$pendencias = New-Object Collections.Generic.List[string]
$geradas = New-Object Collections.Generic.List[string]

$c1A = Validas (Read-Csv (Join-Path $Base 'consolidado\1A.csv'))
if ($c1A.Count -gt 0) {
    $t4 = @()
    $f2 = @()
    foreach ($g in ($c1A | Where-Object fase -eq 'medicao' | Group-Object { [int][double]$_.taxa_ofertada_rps } | Sort-Object { [int]$_.Name })) {
        $r = $g.Group
        $l = [ordered]@{ taxa_ofertada_rps = $g.Name; execucoes_validas = $r.Count }
        Add-Resumo $l 'vazao_efetiva_rps' ($r.vazao_efetiva_rps)
        Add-Resumo $l 'lat_p50_ms' ($r.lat_p50_ms)
        Add-Resumo $l 'lat_p95_ms' ($r.lat_p95_ms)
        Add-Resumo $l 'saga_p50_ms' ($r.saga_p50_ms)
        Add-Resumo $l 'saga_p95_ms' ($r.saga_p95_ms)
        Add-Resumo $l 'sagas_concluidas_pct' ($r.sagas_concluidas_pct)
        Add-Resumo $l 'cpu_media_orders' ($r.cpu_media_orders)
        Add-Resumo $l 'cpu_media_payments' ($r.cpu_media_payments)
        Add-Resumo $l 'mem_max_mb_orders' ($r.mem_max_mb_orders)
        Add-Resumo $l 'mem_max_mb_payments' ($r.mem_max_mb_payments)
        $t4 += [pscustomobject]$l

        foreach ($v in @(@('lat_p95_ms', 'post'), @('saga_p95_ms', 'saga'))) {
            $f = [ordered]@{ taxa_ofertada_rps = $g.Name; variavel = $v[0]; servico = $v[1]; execucoes_validas = $r.Count }
            Add-Resumo $f 'valor' ($r | ForEach-Object { $_.($v[0]) }) -ComIqr
            $f2 += [pscustomobject]$f
        }
        foreach ($svc in @('postgres', 'rabbitmq', 'toxiproxy', 'catalog', 'orders', 'inventory', 'payments')) {
            foreach ($v in @('cpu_media', 'cpu_max', 'mem_media_mb', 'mem_max_mb')) {
                $f = [ordered]@{ taxa_ofertada_rps = $g.Name; variavel = $v; servico = $svc; execucoes_validas = $r.Count }
                Add-Resumo $f 'valor' ($r | ForEach-Object { $_."${v}_$svc" }) -ComIqr
                $f2 += [pscustomobject]$f
            }
        }
    }
    Write-Csv (Join-Path $Base 'tabelas\tabela4.csv') $t4
    Write-Csv (Join-Path $Base 'figuras\figura2.csv') $f2
    $geradas.Add('tabelas\tabela4.csv'); $geradas.Add('figuras\figura2.csv')
}

$c1B = Validas (Read-Csv (Join-Path $Base 'consolidado\1B.csv'))
if ($c1B.Count -gt 0) {
    $med = @($c1B | Where-Object fase -eq 'medicao')
    $a = @($med | Where-Object condicao -eq 'C0-ROFF')
    $b = @($med | Where-Object condicao -eq 'C4-RON')
    $metricas = @('lat_p50_ms', 'lat_p95_ms', 'lat_p99_ms', 'vazao_efetiva_rps', 'erro_pct', 'saga_p50_ms', 'saga_p95_ms')
    foreach ($svc in @('postgres', 'rabbitmq', 'toxiproxy', 'catalog', 'orders', 'inventory', 'payments')) { $metricas += "cpu_media_$svc"; $metricas += "mem_max_mb_$svc" }
    $t5 = @()
    foreach ($m in $metricas) {
        $ma = Q ($a | ForEach-Object { $_.$m }) 0.5
        $mb = Q ($b | ForEach-Object { $_.$m }) 0.5
        $t5 += [pscustomobject][ordered]@{
            metrica                 = $m
            n_C0_ROFF               = $a.Count
            n_C4_RON                = $b.Count
            mediana_C0_ROFF         = F $ma
            mediana_C4_RON          = F $mb
            diferenca_C4_menos_C0   = F ($mb - $ma)
            ic95_inf                = ''
            ic95_sup                = ''
        }
    }
    Write-Csv (Join-Path $Base 'tabelas\tabela5.csv') $t5
    $geradas.Add('tabelas\tabela5.csv')
    
}

$t6 = @()
foreach ($exp in @('2A', '2B')) {
    $c = Validas (Read-Csv (Join-Path $Base "consolidado\$exp.csv"))
    if ($c.Count -eq 0) { continue }
    foreach ($g in ($c | Group-Object condicao | Sort-Object Name)) {
        $falha = @($g.Group | Where-Object fase -eq 'falha')
        $exec = @($g.Group | Where-Object fase -eq 'execucao')
        $l = [ordered]@{ experimento = $exp; condicao = $g.Name; execucoes_validas = $exec.Count }
        Add-Resumo $l 'erro_pct_falha' ($falha.erro_pct) -ComIqr
        Add-Resumo $l 'lat_p95_ms_falha' ($falha.lat_p95_ms) -ComIqr
        Add-Resumo $l 'fator_amplificacao_falha' ($falha.fator_amplificacao) -ComIqr
        Add-Resumo $l 'cb_primeira_abertura_s' ($exec.cb_catalogo_primeira_abertura_s) -ComIqr
        Add-Resumo $l 'fallback_pct_201_falha' ($falha.fallback_pct_201) -ComIqr
        Add-Resumo $l 'recuperacao_s' ($exec.recuperacao_s) -ComIqr
        $l['execucoes_sem_recuperacao'] = @($exec | Where-Object recuperou -eq 'false').Count
        $t6 += [pscustomobject]$l
    }
}
if ($t6.Count -gt 0) { Write-Csv (Join-Path $Base 'tabelas\tabela6.csv') $t6; $geradas.Add('tabelas\tabela6.csv') }

$t7 = @()
foreach ($exp in @('3A', '4A')) {
    $c = Validas (Read-Csv (Join-Path $Base "consolidado\$exp.csv"))
    if ($c.Count -eq 0) { continue }
    foreach ($g in ($c | Group-Object condicao | Sort-Object Name)) {
        $exec = @($g.Group | Where-Object fase -eq 'execucao')
        $l = [ordered]@{ experimento = $exp; condicao = $g.Name; execucoes_validas = $exec.Count }
        foreach ($coorte in @('antes', 'falha', 'depois')) {
            Add-Resumo $l "confirmados_pct_coorte_$coorte" (@($g.Group | Where-Object fase -eq $coorte).confirmados_pct) -ComIqr
        }
        Add-Resumo $l 'msg_reentregues' ($exec.msg_reentregues) -ComIqr
        Add-Resumo $l 'msg_mortas' ($exec.msg_mortas) -ComIqr
        Add-Resumo $l 'reprocessamento_p50_s' ($exec.reprocessamento_p50_s) -ComIqr
        Add-Resumo $l 'reprocessamento_max_s' ($exec.reprocessamento_max_s) -ComIqr
        Add-Resumo $l 'drenagem_segundos' ($exec.drenagem_segundos) -ComIqr
        $l['invariantes_violados_soma'] = (@($exec | ForEach-Object { [int]$_.invariantes_violados }) | Measure-Object -Sum).Sum
        $l['execucoes_com_invariante_violado'] = @($exec | Where-Object { [int]$_.invariantes_violados -gt 0 }).Count
        $t7 += [pscustomobject]$l
    }
}
if ($t7.Count -gt 0) { Write-Csv (Join-Path $Base 'tabelas\tabela7.csv') $t7; $geradas.Add('tabelas\tabela7.csv') }

$j2A = Read-Csv (Join-Path $Base 'janelas\2A.csv')
$j2A = Validas $j2A
if ($j2A.Count -gt 0) {
    $f3 = @()
    foreach ($g in ($j2A | Group-Object condicao, t_rel_s | Sort-Object { $_.Group[0].condicao }, { [int]$_.Group[0].t_rel_s })) {
        $r = $g.Group
        $l = [ordered]@{ condicao = $r[0].condicao; t_rel_s = $r[0].t_rel_s; fase = $r[0].fase; execucoes = $r.Count }
        Add-Resumo $l 'lat_p95_ms' ($r.lat_p95_ms) -ComIqr
        Add-Resumo $l 'erro_pct' ($r.erro_pct) -ComIqr
        Add-Resumo $l 'cb_estado_max' ($r.cb_catalogo_estado_max)
        $l['cb_estado_max_maximo'] = F (Q ($r.cb_catalogo_estado_max) 1.0)
        Add-Resumo $l 'cb_aberto_s' ($r.cb_catalogo_aberto_s)
        $f3 += [pscustomobject]$l
    }
    Write-Csv (Join-Path $Base 'figuras\figura3.csv') $f3
    $geradas.Add('figuras\figura3.csv')
}

$p3A = Validas (Read-Csv (Join-Path $Base 'pedidos\3A.csv'))
if ($p3A.Count -gt 0) {
    $f4 = @($p3A | Select-Object condicao, repeticao, coorte, criado_rel_s, status, saga_ms)
    Write-Csv (Join-Path $Base 'figuras\figura4.csv') $f4
    $geradas.Add('figuras\figura4.csv')
}

$j4A = Validas (Read-Csv (Join-Path $Base 'janelas\4A.csv'))
if ($j4A.Count -gt 0) {
    $c4A = Validas (Read-Csv (Join-Path $Base 'consolidado\4A.csv'))
    $f5 = @()
    foreach ($g in ($j4A | Group-Object condicao, t_rel_s | Sort-Object { $_.Group[0].condicao }, { [int]$_.Group[0].t_rel_s })) {
        $r = $g.Group
        $l = [ordered]@{ condicao = $r[0].condicao; t_rel_s = $r[0].t_rel_s; fase = $r[0].fase; execucoes = $r.Count }
        Add-Resumo $l 'fila_media' ($r.fila_media) -ComIqr
        Add-Resumo $l 'sagas_concluidas' ($r.sagas_concluidas) -ComIqr
        $l['marcadores'] = (@($r.marcadores | Where-Object { $_ } | ForEach-Object { $_ -split ';' }) | Sort-Object -Unique) -join ';'
        $f5 += [pscustomobject]$l
    }
    Write-Csv (Join-Path $Base 'figuras\figura5.csv') $f5
    $geradas.Add('figuras\figura5.csv')
}

$pendencias.Add('IC95%, comparações entre níveis adjacentes com Holm e Mann-Whitney C2 x C3: calculados à parte por analise\estatistica.py (preenche ic95 da tabela5.csv)')

Write-Host 'Gerados:'
$geradas | ForEach-Object { Write-Host "  $_" }
Write-Host 'Notas:'
$pendencias | ForEach-Object { Write-Host "  $_" }
