# Installs the `rustcms` command (Windows, PowerShell 5.1 or newer).
#
#   irm https://irisvn.github.io/RustCMS/install.ps1 | iex
#
# Environment:
#   RUSTCMS_VERSION   a release tag (v0.4.0, v0.4.0-rc.1); default: the latest release
#   RUSTCMS_INSTALL   install root (default %USERPROFILE%\.rustcms; the command goes in its bin\)
#
# The archive is checked against the release's SHA256SUMS before it is opened.
# That detects a corrupted download, not a compromised release.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$RepoUrl = 'https://github.com/irisvn/RustCMS'
$PagesUrl = 'https://irisvn.github.io/RustCMS'
# Tests only: a local mirror of the release assets (a directory path).
$Mirror = $env:RUSTCMS_DOWNLOAD_BASE
$Root = if ($env:RUSTCMS_INSTALL) { $env:RUSTCMS_INSTALL } else { Join-Path $env:USERPROFILE '.rustcms' }
$Target = 'x86_64-pc-windows-msvc'

function Say($Message) { Write-Host "rustcms-install: $Message" }
function Fail($Message) { throw "rustcms-install: error: $Message" }

function Fetch($Url, $Out) {
    if ($Mirror) {
        Copy-Item -LiteralPath $Url -Destination $Out
        return
    }
    if (-not $Url.StartsWith('https://')) { Fail "refusing a non-HTTPS URL: $Url" }
    for ($attempt = 1; ; $attempt++) {
        try {
            Invoke-WebRequest -Uri $Url -OutFile $Out -UseBasicParsing -TimeoutSec 300
            return
        } catch {
            if ($attempt -ge 3) { Fail "download failed: $Url ($($_.Exception.Message))" }
            Start-Sleep -Seconds (2 * $attempt)
        }
    }
}

$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne 'AMD64' -and $arch -ne 'ARM64') { Fail "unsupported processor $arch" }
# Windows on ARM runs the x86_64 binary under emulation.

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("rustcms-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $tag = $env:RUSTCMS_VERSION
    if (-not $tag) {
        $latest = Join-Path $tmp 'latest.txt'
        $latestUrl = if ($Mirror) { Join-Path $Mirror 'latest.txt' } else { "$PagesUrl/latest.txt" }
        Fetch $latestUrl $latest
        $tag = (Get-Content -LiteralPath $latest -Raw).Trim()
    }
    if ($tag -notmatch '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-rc\.(0|[1-9][0-9]*))?$') {
        Fail "not a release tag: '$tag' (no stable release yet? set RUSTCMS_VERSION=vX.Y.Z-rc.N)"
    }

    $archive = "rustcms-$tag-$Target.zip"
    Say "installing rustcms $tag ($Target)"
    $base = if ($Mirror) { Join-Path $Mirror $tag } else { "$RepoUrl/releases/download/$tag" }
    $sep = if ($Mirror) { '\' } else { '/' }
    Fetch "$base$sep$archive" (Join-Path $tmp $archive)
    Fetch "$base${sep}SHA256SUMS" (Join-Path $tmp 'SHA256SUMS')

    # Exactly one line names the archive.
    $entries = @(Get-Content -LiteralPath (Join-Path $tmp 'SHA256SUMS') | ForEach-Object {
        $parts = $_ -split '\s+', 2
        if ($parts.Count -eq 2 -and ($parts[1] -eq $archive -or $parts[1] -eq "*$archive")) { $parts[0] }
    })
    if ($entries.Count -ne 1) { Fail "SHA256SUMS has no single entry for $archive" }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $tmp $archive)).Hash
    if ($actual -ne $entries[0].ToUpperInvariant()) { Fail "checksum mismatch for ${archive}: the download is corrupted" }

    # Only the command is taken from the archive.
    $out = Join-Path $tmp 'out'
    Expand-Archive -LiteralPath (Join-Path $tmp $archive) -DestinationPath $out
    $exe = Join-Path $out 'rustcms.exe'
    if (-not (Test-Path -LiteralPath $exe)) { Fail "$archive has no rustcms.exe" }
    & $exe --version | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail 'the downloaded rustcms does not run on this system' }

    # Next to the destination, then moved: an existing install is kept if anything fails.
    $bin = Join-Path $Root 'bin'
    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    $staged = Join-Path $bin ".rustcms.$PID.exe"
    Copy-Item -LiteralPath $exe -Destination $staged
    $dest = Join-Path $bin 'rustcms.exe'
    if (Test-Path -LiteralPath $dest) {
        # A running rustcms.exe cannot be overwritten, but it can be renamed.
        $old = Join-Path $bin ("rustcms.old-" + [Guid]::NewGuid() + ".exe")
        Move-Item -LiteralPath $dest -Destination $old
        try { Move-Item -LiteralPath $staged -Destination $dest }
        catch { Move-Item -LiteralPath $old -Destination $dest; throw }
        Remove-Item -LiteralPath $old -ErrorAction SilentlyContinue
    } else {
        Move-Item -LiteralPath $staged -Destination $dest
    }
    $json = '{"version":"' + $tag + '","target":"' + $Target + '","installer":"install.ps1"}'
    Set-Content -LiteralPath (Join-Path $Root 'install.json') -Value $json -Encoding ASCII
    Say "installed $dest ($(& $dest --version))"

    # The user PATH, once.
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entriesInPath = @($userPath -split ';' | Where-Object { $_ -ne '' })
    if ($entriesInPath -notcontains $bin) {
        [Environment]::SetEnvironmentVariable('Path', (($entriesInPath + $bin) -join ';'), 'User')
        Say "added $bin to your PATH (open a new terminal)"
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Say 'Rust is not installed: install it from https://rustup.rs, then run `rustcms doctor`'
    }
    Say 'next: rustcms new blog'
} finally {
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
