param(
    [string]$InstallDirectory = "$env:LOCALAPPDATA\Programs\Zed-LaTeX",
    [switch]$CheckOnly
)
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$repo = 'maxmalkin/zed'
$headers = @{ 'User-Agent' = 'Zed-LaTeX-Updater'; Accept = 'application/vnd.github+json' }
$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers $headers
if ($release.tag_name -notmatch '^latex-[0-9]+$' -or $release.draft -or $release.prerelease) {
    throw 'No published custom Zed release was found.'
}
$install = [IO.Path]::GetFullPath($InstallDirectory).TrimEnd('\')
if ([IO.Path]::GetFileName($install) -ne 'Zed-LaTeX') { throw 'Installation folder must be named Zed-LaTeX.' }
$marker = Join-Path $install 'CUSTOM-RELEASE.txt'
if ((Test-Path $marker) -and (Get-Content $marker -Raw).Trim() -eq $release.tag_name) {
    Write-Output "Already installed: $($release.tag_name)"
    return
}
Write-Output "Available: $($release.tag_name) - $($release.html_url)"
if ($CheckOnly) { return }
# Never terminate an editor with potentially unsaved work.
$running = Get-Process -Name zed-latex,cli,tectonic -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($install + '\', [StringComparison]::OrdinalIgnoreCase) }
if ($running) { throw 'Close Zed LaTeX and its compiler/CLI before updating.' }
if ((Test-Path $install) -and !(Test-Path (Join-Path $install 'zed-latex.exe'))) {
    throw 'Existing directory is not a Zed LaTeX installation.'
}
$parent = Split-Path $install -Parent
New-Item -ItemType Directory -Force $parent | Out-Null
$lock = [IO.File]::Open((Join-Path $parent '.Zed-LaTeX.update.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
$stage = Join-Path $parent ('Zed-LaTeX-stage-' + [guid]::NewGuid())
$backup = $install + '.previous'
try {
    New-Item -ItemType Directory $stage | Out-Null
    foreach ($name in @('Zed-LaTeX-Windows-x64.zip', 'SHA256SUMS')) {
        $assets = @($release.assets | Where-Object name -eq $name)
        if ($assets.Count -ne 1) { throw "Missing or ambiguous release asset: $name" }
        $url = $assets[0].browser_download_url
        if (!$url.StartsWith("https://github.com/$repo/releases/download/$($release.tag_name)/", [StringComparison]::Ordinal)) {
            throw 'Unexpected release download URL.'
        }
        Invoke-WebRequest $url -UseBasicParsing -OutFile (Join-Path $stage $name)
    }
    $zip = Join-Path $stage 'Zed-LaTeX-Windows-x64.zip'
    $sum = (Get-Content (Join-Path $stage 'SHA256SUMS') -Raw).Trim()
    if ($sum -notmatch '^([a-fA-F0-9]{64})  Zed-LaTeX-Windows-x64.zip$') { throw 'Invalid checksum manifest.' }
    if ((Get-FileHash $zip -Algorithm SHA256).Hash -ne $Matches[1]) { throw 'Release checksum mismatch.' }
    $payload = Join-Path $stage 'app'
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($zip)
    try {
        foreach ($entry in $archive.Entries) {
            $path = [IO.Path]::GetFullPath((Join-Path $payload $entry.FullName))
            if (!$path.StartsWith($payload + '\', [StringComparison]::OrdinalIgnoreCase) -or $entry.FullName.Contains(':')) {
                throw 'Unsafe archive path.'
            }
        }
    } finally { $archive.Dispose() }
    Expand-Archive $zip $payload
    foreach ($file in @('zed-latex.exe', 'cli.exe', 'tectonic.exe', 'conpty.dll', 'BUILD-REVISION.txt', 'Update-Zed-LaTeX.ps1', 'remote_servers/linux-x86_64/remote_server.gz')) {
        if (!(Test-Path (Join-Path $payload $file) -PathType Leaf)) { throw "Incomplete release: $file" }
    }
    $release.tag_name | Set-Content (Join-Path $payload 'CUSTOM-RELEASE.txt')
    # Keep one complete previous bundle, including its matching remote server.
    if (Test-Path $backup) {
        if (!(Test-Path (Join-Path $backup 'zed-latex.exe'))) { throw 'Unrecognized backup folder; leaving it untouched.' }
        Remove-Item $backup -Recurse -Force
    }
    if (Test-Path $install) { Move-Item $install $backup }
    try { Move-Item $payload $install }
    catch {
        if ((Test-Path $backup) -and !(Test-Path $install)) { Move-Item $backup $install }
        throw
    }
    Write-Output "Installed $($release.tag_name) at $install. Previous bundle: $backup"
} finally {
    try { if (Test-Path $stage) { Remove-Item $stage -Recurse -Force } }
    finally { $lock.Dispose() }
}
