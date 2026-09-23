param(
    [Parameter(Mandatory = $true)][string]$Consolidado,
    [string]$Saida,
    [double]$P95MaxMs = 500,
    [double]$ErroMaxPct = 1,
    [double]$SagasMinPct = 99,
    [double]$FracaoNominal = 0.5,
    [int[]]$Grade = @(10, 20, 40, 60)
)

$ErrorActionPreference = 'Stop'
if ($Saida) { $Saida = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Saida) }
$inv = [Globalization.CultureInfo]::InvariantCulture
[Threading.Thread]::CurrentThread.CurrentCulture = $inv

$linhas = @(Import-Csv $Consolidado -Encoding UTF8)
$medicao = @($linhas | Where-Object fase -eq 'medicao')
$avaliacao = @()
foreach ($m in $medicao) {
    $p95 = [double]$m.lat_p95_ms
    $erro = [double]$m.erro_pct
    $sagas = [double]$m.sagas_concluidas_pct
    $filaIni = [double]$m.fila_media_3_primeiras
    $filaFim = [double]$m.fila_media_3_ultimas
    $c = [ordered]@{
        taxa                   = [int][double]$m.taxa_ofertada_rps
        repeticao              = $m.repeticao
        valida                 = $m.valida
        lat_p95_ms             = $m.lat_p95_ms
        erro_pct               = $m.erro_pct
        sagas_concluidas_pct   = $m.sagas_concluidas_pct
        fila_media_3_primeiras = $m.fila_media_3_primeiras
        fila_media_3_ultimas   = $m.fila_media_3_ultimas
        ok_p95                 = ($p95 -lt $P95MaxMs)
        ok_erro                = ($erro -lt $ErroMaxPct)
        ok_sagas               = ($sagas -ge $SagasMinPct)
        ok_fila                = ($filaFim -le $filaIni)
    }
    $c['atende'] = ($m.valida -eq 'true') -and $c.ok_p95 -and $c.ok_erro -and $c.ok_sagas -and $c.ok_fila
    $avaliacao += [pscustomobject]$c
}

$niveis = @($avaliacao | Group-Object taxa | ForEach-Object {
        $validas = @($_.Group | Where-Object valida -eq 'true')
        [pscustomobject]@{
            taxa              = [int]$_.Name
            repeticoes        = $_.Count
            validas           = $validas.Count
            atende_todas      = ($validas.Count -gt 0 -and @($_.Group | Where-Object { -not $_.atende }).Count -eq 0)
        }
    } | Sort-Object taxa)

$aprovados = @($niveis | Where-Object atende_todas)
if ($aprovados.Count -gt 0) {
    $maxima = ($aprovados | Measure-Object taxa -Maximum).Maximum
    $alvo = $FracaoNominal * $maxima
    $candidatas = @($Grade | Where-Object { $_ -le $alvo })
    if ($candidatas.Count -gt 0) { $nominal = ($candidatas | Measure-Object -Maximum).Maximum; $nota = '' }
    else { $nominal = ($Grade | Measure-Object -Minimum).Minimum; $nota = "50% de $maxima = $alvo fica abaixo da grade; usado o menor nível da grade" }
}
else {
    $maxima = $null
    $nominal = 10
    $nota = 'nenhum nível atendeu a todos os critérios; usado 10 req/s pela regra 2'
}

$avaliacao | Format-Table -AutoSize | Out-String -Width 250 | Write-Host
$niveis | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "taxa_maxima_sustentavel=$maxima taxa_nominal=$nominal $nota"
if ($Saida) {
    New-Item -ItemType Directory -Force -Path (Split-Path $Saida) | Out-Null
    $texto = $avaliacao | ConvertTo-Csv -NoTypeInformation
    [IO.File]::WriteAllLines($Saida, [string[]]$texto, (New-Object Text.UTF8Encoding($false)))
}
