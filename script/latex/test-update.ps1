$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ('Zed updater test ' + [guid]::NewGuid())
New-Item -ItemType Directory $root | Out-Null
$fixture = Join-Path $root 'fixture'
$install = Join-Path $root 'Zed-LaTeX'
$source = Join-Path $root 'source'
New-Item -ItemType Directory $fixture,$source | Out-Null
function Assert($condition, $message) { if (!$condition) { throw $message } }
# Exercise the real filesystem/install code with a local release and no network.
function Invoke-RestMethod { param($Uri, $Headers) return $releaseFixture }
function Invoke-WebRequest {
    param($Uri, [switch]$UseBasicParsing, $OutFile)
    Copy-Item (Join-Path $fixture ([IO.Path]::GetFileName($Uri))) $OutFile
}
try {
    foreach ($file in @('zed-latex.exe','cli.exe','tectonic.exe','conpty.dll','BUILD-REVISION.txt','Update-Zed-LaTeX.ps1','remote_servers/linux-x86_64/remote_server.gz')) {
        $path = Join-Path $source $file
        New-Item -ItemType Directory -Force (Split-Path $path) | Out-Null
        'fixture' | Set-Content $path
    }
    Compress-Archive "$source\*" (Join-Path $fixture 'Zed-LaTeX-Windows-x64.zip')
    $hash = (Get-FileHash (Join-Path $fixture 'Zed-LaTeX-Windows-x64.zip')).Hash
    "$hash  Zed-LaTeX-Windows-x64.zip" | Set-Content (Join-Path $fixture 'SHA256SUMS')
    $releaseFixture = @{ tag_name='latex-1'; draft=$false; prerelease=$false; html_url='fixture'; assets=@() }
    foreach ($name in @('Zed-LaTeX-Windows-x64.zip','SHA256SUMS')) {
        $releaseFixture.assets += @{name=$name; browser_download_url="https://github.com/maxmalkin/zed/releases/download/latex-1/$name"}
    }
    & "$PSScriptRoot/update.ps1" -InstallDirectory $install -CheckOnly
    Assert (!(Test-Path $install)) 'CheckOnly installed files'
    & "$PSScriptRoot/update.ps1" -InstallDirectory $install
    Assert ((Get-Content "$install/CUSTOM-RELEASE.txt").Trim() -eq 'latex-1') 'First install failed'
    & "$PSScriptRoot/update.ps1" -InstallDirectory $install
    Assert (!(Test-Path "$install.previous")) 'No-op created backup'
    $releaseFixture.tag_name = 'latex-2'
    foreach ($asset in $releaseFixture.assets) { $asset.browser_download_url = $asset.browser_download_url.Replace('latex-1','latex-2') }
    & "$PSScriptRoot/update.ps1" -InstallDirectory $install
    Assert ((Get-Content "$install.previous/CUSTOM-RELEASE.txt").Trim() -eq 'latex-1') 'Previous version not preserved'
    $releaseFixture.tag_name = 'latex-3'
    foreach ($asset in $releaseFixture.assets) { $asset.browser_download_url = $asset.browser_download_url.Replace('latex-2','latex-3') }
    (('0' * 64) + '  Zed-LaTeX-Windows-x64.zip') | Set-Content (Join-Path $fixture 'SHA256SUMS')
    $rejected = $false
    try { & "$PSScriptRoot/update.ps1" -InstallDirectory $install }
    catch { if ($_.Exception.Message -ne 'Release checksum mismatch.') { throw }; $rejected = $true }
    Assert $rejected 'Bad checksum accepted'
    Assert ((Get-Content "$install/CUSTOM-RELEASE.txt").Trim() -eq 'latex-2') 'Failed update changed installed version'
    Assert ((Get-Content "$install.previous/CUSTOM-RELEASE.txt").Trim() -eq 'latex-1') 'Failed update removed rollback'
    Assert (@(Get-ChildItem $root -Filter 'Zed-LaTeX-stage-*').Count -eq 0) 'Staging files leaked'
    Write-Output 'Updater tests passed.'
} finally { Remove-Item $root -Recurse -Force }
