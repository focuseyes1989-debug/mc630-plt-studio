param(
    [Parameter(Mandatory=$true)][string]$Project,
    [string]$OutputDirectory = 'target/native-ui-benchmark',
    [int]$Runs = 3
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Push-Location $repo
$oldAppData = $env:APPDATA
$oldLocalAppData = $env:LOCALAPPDATA
try {
    $scene = (Resolve-Path -LiteralPath $Project).Path
    cargo build --release --locked --bin mc630-plt-studio
    if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
    $results = New-Item -ItemType Directory -Force $OutputDirectory
    $metadata = @{
        project_bytes = (Get-Item -LiteralPath $scene).Length
        project_sha256 = (Get-FileHash -LiteralPath $scene -Algorithm SHA256).Hash
        processor = (Get-CimInstance Win32_Processor).Name
        graphics = @((Get-CimInstance Win32_VideoController).Name)
        os = (Get-CimInstance Win32_OperatingSystem).Caption
        command = 'Release build, identical project, sequential alternating runs, continuous camera replay'
    }
    $metadata | ConvertTo-Json | Set-Content (Join-Path $results.FullName 'machine.json') -Encoding utf8
    foreach ($iteration in 1..$Runs) {
        $order = if ($iteration % 2 -eq 1) { @('egui', 'win32') } else { @('win32', 'egui') }
        foreach ($renderer in $order) {
            $profile = Join-Path $results.FullName "$renderer-$iteration-profile"
            $env:APPDATA = Join-Path $profile 'AppData'
            $env:LOCALAPPDATA = Join-Path $profile 'LocalAppData'
            New-Item -ItemType Directory -Force $env:APPDATA,$env:LOCALAPPDATA | Out-Null
            $result = Join-Path $results.FullName "$renderer-$iteration.json"
            $flag = if ($renderer -eq 'egui') { '--benchmark-egui' } else { '--prototype-win32' }
            # Quote file arguments so project and output paths can contain spaces.
            $arguments = "$flag `"$scene`" `"$result`""
            $started = [datetime]::UtcNow
            $process = Start-Process target/release/mc630-plt-studio.exe -ArgumentList $arguments -PassThru
            if (-not $process.WaitForExit(60000)) {
                throw "Benchmark timed out. Inspect process $($process.Id)."
            }
            if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $result)) {
                throw "Benchmark failed for $renderer run $iteration."
            }
            if ((Get-Item -LiteralPath $result).LastWriteTimeUtc -lt $started) {
                throw "The benchmark did not produce a new result for $renderer run $iteration."
            }
            $data = Get-Content -LiteralPath $result -Raw | ConvertFrom-Json
            [pscustomobject]@{
                Renderer = $renderer
                Run = $iteration
                OpenMs = [math]::Round($data.open_ready_ms, 1)
                MainMiB = [math]::Round($data.memory_by_phase[0].working_set_mib, 1)
                BackMiB = [math]::Round($data.memory_by_phase[2].working_set_mib, 1)
            }
        }
    }
} finally {
    $env:APPDATA = $oldAppData
    $env:LOCALAPPDATA = $oldLocalAppData
    Pop-Location
}
