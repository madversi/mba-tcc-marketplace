param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('catalog', 'inventory', 'payments')]
    [string]$Servico,
    [string]$Warmup = '1m',
    [string]$Janela = '1m',
    [string]$Recuperacao = '3m',
    [int]$Rate = 10
)

$ErrorActionPreference = 'Stop'

$compose = Join-Path $PSScriptRoot '..\docker\docker-compose.yml' | Resolve-Path
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$stdout = Join-Path $env:TEMP "k6-falha-$Servico-$stamp.out.log"
$stderr = Join-Path $env:TEMP "k6-falha-$Servico-$stamp.err.log"

function Quote([string]$value) {
    if ($value -match '\s') { return "`"$value`"" }
    return $value
}

function Test-Marker([string]$marker) {
    if (-not (Test-Path $stderr)) { return $false }
    $stream = [System.IO.File]::Open($stderr, 'Open', 'Read', 'ReadWrite')
    try {
        return (New-Object System.IO.StreamReader($stream)).ReadToEnd().Contains($marker)
    }
    finally {
        $stream.Dispose()
    }
}

function Wait-Marker([string]$marker, $process) {
    while (-not (Test-Marker $marker)) {
        if ($process.HasExited) {
            throw "k6 terminou antes de emitir '$marker'. Veja $stderr"
        }
        Start-Sleep -Milliseconds 250
    }
}

$k6Args = @(
    'compose', '-f', (Quote $compose), '--profile', 'load', 'run', '--rm', '-T',
    '-e', 'FALHA=servico-parado',
    '-e', "SERVICO=$Servico",
    '-e', "RATE=$Rate",
    '-e', "WARMUP=$Warmup",
    '-e', "FAILURE_WINDOW=$Janela",
    '-e', "RECOVERY=$Recuperacao",
    'k6', 'run',
    '--summary-trend-stats', 'avg,min,med,p(90),p(95),p(99),max',
    '--summary-export', "/results/falha-$Servico-$stamp.json",
    '/scripts/falhas.js'
) -join ' '

Write-Host "k6 iniciado; logs em $stderr"
$k6 = Start-Process -FilePath 'docker' -ArgumentList $k6Args -NoNewWindow -PassThru `
    -RedirectStandardOutput $stdout -RedirectStandardError $stderr
$null = $k6.Handle

try {
    Wait-Marker 'FALHA_INICIO' $k6
    Write-Host "$(Get-Date -Format HH:mm:ss) parando $Servico"
    docker compose -f $compose stop -t 0 $Servico

    Wait-Marker 'FALHA_FIM' $k6
    Write-Host "$(Get-Date -Format HH:mm:ss) religando $Servico"
    docker compose -f $compose start $Servico

    $k6.WaitForExit()
}
finally {
    docker compose -f $compose start $Servico | Out-Null
}

Get-Content $stdout
exit $k6.ExitCode
