# Observa origin/master e substitui MyDigitalBuddy.exe quando entram commits novos.
# Duplo clique em atualizar.bat: verifica agora e de novo a cada 5 minutos.
# Uma vez: atualizar.bat -UmaVez
# Outro intervalo (segundos, minimo 60): atualizar.bat -Intervalo 120

param(
    [Alias('h', 'Help')]
    [switch]$Ajuda,
    [switch]$UmaVez,
    [int]$Intervalo = 300
)

$ErrorActionPreference = 'Stop'
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}
Set-Location -LiteralPath $PSScriptRoot

if ($Ajuda) {
    Write-Host @'

Atualiza MyDigitalBuddy.exe quando a branch master recebe commits novos.

  atualizar.bat                 verifica agora e repete a cada 5 minutos
  atualizar.bat -UmaVez         verifica uma vez e termina
  atualizar.bat -Intervalo 120  intervalo em segundos (mínimo 60)

A compilação usa a master já atualizada. O arquivo MyDigitalBuddy.exe é
trocado no lugar. Se esse EXE estiver aberto, ele é fechado depois da
compilação e iniciado de novo. Uma falha de compilação não apaga o EXE antigo.

'@
    exit 0
}

if ($Intervalo -lt 60) { $Intervalo = 60 }

function Write-Log([string]$Message) {
    $ts = Get-Date -Format 'yyyy-MM-dd HH:mm:ss'
    Write-Host "[$ts] $Message"
}

function Invoke-Git {
    param([Parameter(Mandatory = $true)][string[]]$ArgumentList)
    & git --no-pager @ArgumentList
    if ($LASTEXITCODE -ne 0) {
        throw "git $($ArgumentList -join ' ') falhou (codigo $LASTEXITCODE)."
    }
}

function Get-GitText {
    param([Parameter(Mandatory = $true)][string[]]$ArgumentList)
    $output = & git --no-pager @ArgumentList
    if ($LASTEXITCODE -ne 0) {
        throw "git $($ArgumentList -join ' ') falhou (codigo $LASTEXITCODE)."
    }
    return (($output | Out-String).Trim())
}

function Get-GitLines {
    param([Parameter(Mandatory = $true)][string[]]$ArgumentList)
    $output = @(& git --no-pager @ArgumentList)
    if ($LASTEXITCODE -ne 0) {
        throw "git $($ArgumentList -join ' ') falhou (codigo $LASTEXITCODE)."
    }
    return @($output | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) })
}

function Set-BuildToolchain {
    $channel = '1.99.1'
    $file = Join-Path $PSScriptRoot 'rust-toolchain.toml'
    if (Test-Path -LiteralPath $file) {
        foreach ($line in Get-Content -LiteralPath $file) {
            if ($line -match '^\s*channel\s*=\s*"([^"]+)"') {
                $channel = $Matches[1]
                break
            }
        }
    }
    if (Get-Command link.exe -ErrorAction SilentlyContinue) {
        return
    }
    if (-not (Get-Command gcc.exe -ErrorAction SilentlyContinue)) {
        throw 'Linker ausente. Instale o Visual Studio Build Tools com a carga C++, ou o MinGW-w64 (gcc no PATH).'
    }
    $gnu = "$channel-x86_64-pc-windows-gnu"
    $installed = & rustup toolchain list
    if ($LASTEXITCODE -ne 0) { throw 'rustup toolchain list falhou.' }
    if (($installed -join "`n") -notmatch [regex]::Escape($gnu)) {
        Write-Log "Instalando a toolchain $gnu"
        & rustup toolchain install $gnu
        if ($LASTEXITCODE -ne 0) { throw "Não foi possível instalar $gnu." }
    }
    $env:RUSTUP_TOOLCHAIN = $gnu
    Write-Log "Compilando com $gnu"
}

function Wait-ProcessesExit {
    param([int[]]$ProcessIds)
    $deadline = (Get-Date).AddSeconds(20)
    do {
        $alive = @($ProcessIds | Where-Object {
            Get-Process -Id $_ -ErrorAction SilentlyContinue
        })
        if ($alive.Count -eq 0) { return $true }
        Start-Sleep -Milliseconds 200
    } while ((Get-Date) -lt $deadline)
    return $false
}

function Install-UpdatedExe {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Destination
    )
    $incoming = "$Destination.new"
    $backup = "$Destination.bak"
    Copy-Item -LiteralPath $Source -Destination $incoming -Force

    $full = [System.IO.Path]::GetFullPath($Destination)
    $running = @(Get-CimInstance Win32_Process -Filter "Name = 'MyDigitalBuddy.exe'" |
        Where-Object {
            $_.ExecutablePath -and ([System.IO.Path]::GetFullPath($_.ExecutablePath) -ieq $full)
        })
    $wasRunning = $running.Count -gt 0
    $processIds = @($running | ForEach-Object { [int]$_.ProcessId })

    try {
        if (Test-Path -LiteralPath $backup) {
            Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue
        }
        if (Test-Path -LiteralPath $Destination) {
            try {
                Move-Item -LiteralPath $Destination -Destination $backup -Force
            }
            catch {
                foreach ($proc in $running) {
                    Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue
                }
                if (-not (Wait-ProcessesExit -ProcessIds $processIds)) {
                    throw 'MyDigitalBuddy.exe continua em uso.'
                }
                Move-Item -LiteralPath $Destination -Destination $backup -Force
            }
        }
        try {
            Move-Item -LiteralPath $incoming -Destination $Destination -Force
        }
        catch {
            if ((Test-Path -LiteralPath $backup) -and -not (Test-Path -LiteralPath $Destination)) {
                Move-Item -LiteralPath $backup -Destination $Destination -Force
            }
            throw
        }
    }
    catch {
        if (Test-Path -LiteralPath $incoming) {
            Remove-Item -LiteralPath $incoming -Force -ErrorAction SilentlyContinue
        }
        if ($wasRunning -and (Test-Path -LiteralPath $Destination)) {
            Start-Process -FilePath $Destination -WorkingDirectory $PSScriptRoot | Out-Null
        }
        throw "Não foi possível substituir MyDigitalBuddy.exe. $($_.Exception.Message)"
    }

    if ($wasRunning) {
        foreach ($proc in $running) {
            Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue
        }
        if (-not (Wait-ProcessesExit -ProcessIds $processIds)) {
            Write-Log 'O processo antigo ainda não encerrou. Abra MyDigitalBuddy.exe de novo se a janela não voltar.'
        }
        Start-Process -FilePath $Destination -WorkingDirectory $PSScriptRoot | Out-Null
        Write-Log 'MyDigitalBuddy.exe foi substituído e o programa foi iniciado de novo.'
    }
    else {
        Write-Log 'MyDigitalBuddy.exe foi substituído.'
    }

    Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $incoming -Force -ErrorAction SilentlyContinue
}

function Update-MasterExe {
    param(
        [Parameter(Mandatory = $true)][string]$StampFile,
        [Parameter(Mandatory = $true)][string]$ExePath
    )

    Write-Log 'Buscando origin/master'
    Invoke-Git -ArgumentList @('-c', 'http.lowSpeedLimit=1000', '-c', 'http.lowSpeedTime=30', 'fetch', 'origin', 'master')

    $inside = Get-GitText -ArgumentList @('rev-parse', '--is-inside-work-tree')
    if ($inside -ne 'true') { throw 'Esta pasta não é um repositório Git.' }

    $branch = Get-GitText -ArgumentList @('branch', '--show-current')
    if ($branch -ne 'master') {
        Write-Log "Branch atual: $branch. O atualizador acompanha somente a master, e o EXE foi mantido."
        return
    }

    Invoke-Git -ArgumentList @('rev-parse', '--verify', '--quiet', 'origin/master') | Out-Null
    $counts = (Get-GitText -ArgumentList @('rev-list', '--left-right', '--count', 'HEAD...origin/master')) -split '\s+'
    if ($counts.Count -lt 2) { throw "Comparação inesperada com origin/master: $($counts -join ' ')" }
    $ahead = [int]$counts[0]
    $behind = [int]$counts[1]
    $dirty = @(Get-GitLines -ArgumentList @('status', '--porcelain', '--untracked-files=no'))

    if ($ahead -gt 0 -and $behind -gt 0) {
        Write-Log 'A master local divergiu de origin/master. Nenhum pull e nenhuma compilação.'
        return
    }
    if ($ahead -gt 0) {
        Write-Log 'origin/master não tem commits novos. Commits locais ainda não publicados ficam de fora desta atualização.'
        return
    }

    $originSha = Get-GitText -ArgumentList @('rev-parse', 'origin/master')
    $stamp = ''
    if (Test-Path -LiteralPath $StampFile) {
        $stamp = (Get-Content -LiteralPath $StampFile -Raw).Trim()
    }

    $needsBuild = $behind -gt 0 -or -not (Test-Path -LiteralPath $ExePath)
    if ((Test-Path -LiteralPath $ExePath) -and $stamp -eq $originSha -and $behind -eq 0) {
        $needsBuild = $false
    }
    elseif ((Test-Path -LiteralPath $ExePath) -and -not $stamp -and $behind -eq 0) {
        $commitRaw = Get-GitText -ArgumentList @('show', '-s', '--format=%cI', 'HEAD')
        $commitTime = [datetimeoffset]::Parse($commitRaw, [Globalization.CultureInfo]::InvariantCulture)
        $exeTime = (Get-Item -LiteralPath $ExePath).LastWriteTimeUtc
        if ($exeTime -ge $commitTime.UtcDateTime.AddMinutes(-2)) {
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $StampFile) | Out-Null
            Set-Content -LiteralPath $StampFile -Value $originSha -Encoding ascii -NoNewline
            Write-Log 'MyDigitalBuddy.exe já está alinhado com origin/master.'
            return
        }
        $needsBuild = $true
    }
    elseif ($stamp -ne $originSha) {
        $needsBuild = $true
    }

    if (-not $needsBuild) {
        Write-Log 'origin/master não tem commits novos.'
        return
    }
    if ($dirty.Count -gt 0) {
        Write-Log 'Há commits novos ou o EXE está atrás da master, mas existem alterações locais. O EXE antigo foi mantido.'
        return
    }

    if ($behind -gt 0) {
        Write-Log "origin/master tem $behind commit(s) novo(s):"
        Get-GitLines -ArgumentList @('log', '--oneline', '--max-count=15', 'HEAD..origin/master') |
            ForEach-Object { Write-Host "  $_" }
        Invoke-Git -ArgumentList @('pull', '--ff-only', 'origin', 'master')
    }

    $sha = Get-GitText -ArgumentList @('rev-parse', 'HEAD')
    $built = Join-Path $PSScriptRoot 'target\release\mydigitalbuddy.exe'
    Set-BuildToolchain
    $short = $sha.Substring(0, [Math]::Min(12, $sha.Length))
    Write-Log "Compilando revisão $short"
    & cargo build --release --locked
    if ($LASTEXITCODE -ne 0) {
        throw "A compilação falhou (código $LASTEXITCODE). MyDigitalBuddy.exe antigo foi mantido."
    }
    if (-not (Test-Path -LiteralPath $built)) {
        throw 'A compilação não gerou target\release\mydigitalbuddy.exe.'
    }

    Install-UpdatedExe -Source $built -Destination $ExePath
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $StampFile) | Out-Null
    Set-Content -LiteralPath $StampFile -Value $sha -Encoding ascii -NoNewline
    Write-Log "EXE atualizado para $sha"
}

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Log 'Git não está no PATH.'
    exit 1
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Log 'Cargo não está no PATH. Instale o Rust em https://rustup.rs'
    exit 1
}
if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    Write-Log 'rustup não está no PATH.'
    exit 1
}

try { $Host.UI.RawUI.WindowTitle = 'MyDigitalBuddy - atualizar EXE' } catch { }

$repoKeySource = $PSScriptRoot.TrimEnd('\').ToLowerInvariant()
$sha256 = [Security.Cryptography.SHA256]::Create()
try {
    $repoKey = ([BitConverter]::ToString($sha256.ComputeHash([Text.Encoding]::UTF8.GetBytes($repoKeySource)))).Replace('-', '').Substring(0, 16)
}
finally {
    $sha256.Dispose()
}

$stampDir = Join-Path $env:LOCALAPPDATA 'MyDigitalBuddy\updater'
$stampFile = Join-Path $stampDir "$repoKey.rev"
$exePath = Join-Path $PSScriptRoot 'MyDigitalBuddy.exe'
$env:GIT_TERMINAL_PROMPT = '0'

$mutex = New-Object System.Threading.Mutex($false, "Local\MyDigitalBuddyAtualizar-$repoKey")
$ownsMutex = $false
try {
    $ownsMutex = $mutex.WaitOne(0)
    if (-not $ownsMutex) {
        Write-Log 'Já existe um atualizador em execução para este repositório.'
        exit 0
    }

    while ($true) {
        try {
            Update-MasterExe -StampFile $stampFile -ExePath $exePath
        }
        catch {
            Write-Log "ERRO: $($_.Exception.Message)"
            if ($UmaVez) { exit 1 }
        }
        if ($UmaVez) { exit 0 }
        Write-Log "Próxima verificação em $Intervalo segundos. Feche esta janela para parar."
        Start-Sleep -Seconds $Intervalo
    }
}
finally {
    if ($ownsMutex) { [void]$mutex.ReleaseMutex() }
    $mutex.Dispose()
}
