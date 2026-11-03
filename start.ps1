[CmdletBinding()]
param(
    [switch]$InitOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
$server = $null
$savedEnvironment = @{}

function Invoke-Gateveriq {
    param([string[]]$Arguments)
    $output = & $script:binary @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Gateveriq failed (exit code $LASTEXITCODE)."
    }
    return $output
}

function Assert-PortAvailable {
    param([int]$Port)
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
    try {
        $listener.Start()
    } catch {
        throw "Port $Port is already in use. Stop the existing service and retry."
    } finally {
        $listener.Stop()
    }
}

try {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw 'Cargo was not found. Install stable Rust and the C/C++ build tools first.'
    }
    if (-not $InitOnly) {
        $python = Get-Command python -ErrorAction SilentlyContinue
        if (-not $python) { $python = Get-Command python3 -ErrorAction SilentlyContinue }
        if (-not $python) { throw 'Python 3 was not found.' }
        & $python.Source -c 'import sys; sys.exit(0 if sys.version_info.major == 3 else 1)'
        if ($LASTEXITCODE -ne 0) { throw 'A working Python 3 installation is required.' }
        foreach ($port in @(9080, 9081, 8090)) { Assert-PortAvailable $port }
    }

    Write-Host 'Building Gateveriq...'
    & cargo build --locked
    if ($LASTEXITCODE -ne 0) { throw 'Cargo build failed. See the compiler output above.' }
    # Cargo metadata respects CARGO_TARGET_DIR and .cargo/config.toml.
    $metadataJson = & cargo metadata --no-deps --format-version 1 --locked
    if ($LASTEXITCODE -ne 0) { throw 'Could not locate the Cargo build directory.' }
    $metadata = $metadataJson | ConvertFrom-Json
    $script:binary = Join-Path $metadata.target_directory 'debug/gateveriq.exe'

    $credentialsPath = Join-Path $PSScriptRoot '.demo-credentials.json'
    if (-not (Test-Path -LiteralPath 'config.toml')) {
        if (Test-Path -LiteralPath $credentialsPath) {
            throw 'config.toml is missing but demo credentials exist. Restore the configuration before continuing.'
        }
        Invoke-Gateveriq -Arguments @('init') | Out-Host
    }
    if (-not (Test-Path -LiteralPath $credentialsPath)) {
        $credentialsJson = Invoke-Gateveriq -Arguments @(
            'integration-create', '--name', 'visual-demo', '--actions', 'demo',
            '--origins', 'http://127.0.0.1:8090'
        )
        $credentials = $credentialsJson | ConvertFrom-Json
        $credentials | ConvertTo-Json | Set-Content -LiteralPath $credentialsPath -Encoding UTF8
        Write-Host 'Demo credentials saved to .demo-credentials.json. Keep this file private.'
    } else {
        $credentials = Get-Content -LiteralPath $credentialsPath -Raw | ConvertFrom-Json
    }
    if (-not $credentials.integration_id -or -not $credentials.integration_secret) {
        throw 'The demo credentials file must contain integration_id and integration_secret.'
    }
    if ($InitOnly) {
        Write-Host 'Initialization complete. Run .\start.ps1 to start the demo.'
        return
    }

    foreach ($name in @('GATEVERIQ_ID', 'GATEVERIQ_SECRET', 'GATEVERIQ_WEBSITE')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
    $env:GATEVERIQ_ID = $credentials.integration_id
    $env:GATEVERIQ_SECRET = $credentials.integration_secret
    $env:GATEVERIQ_WEBSITE = '1'
    $server = Start-Process -FilePath $script:binary -ArgumentList 'serve' -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    foreach ($port in @(9080, 9081)) {
        while ($true) {
            if ($server.HasExited) { throw "API server exited with code $($server.ExitCode). Check config.toml." }
            $client = [System.Net.Sockets.TcpClient]::new()
            try {
                $client.Connect('127.0.0.1', $port)
                break
            } catch {
                if ([DateTime]::UtcNow -ge $deadline) {
                    throw "API did not start on 127.0.0.1:$port. Check the listen addresses in config.toml."
                }
                Start-Sleep -Milliseconds 200
            } finally {
                $client.Dispose()
            }
        }
    }
    Write-Host 'Demo: http://127.0.0.1:8090 (Ctrl+C to stop)'
    & $python.Source (Join-Path $PSScriptRoot 'examples/demo.py')
    if ($LASTEXITCODE -ne 0) { throw "Demo exited with code $LASTEXITCODE." }
} finally {
    if ($null -ne $server -and -not $server.HasExited) {
        Stop-Process -Id $server.Id -ErrorAction SilentlyContinue
    }
    foreach ($name in $savedEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process')
    }
    Pop-Location
}
