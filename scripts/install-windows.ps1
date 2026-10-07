# Installs Encore on Windows from GitHub Releases:
#
#   irm https://raw.githubusercontent.com/jvz-devx/encore-yt/main/scripts/install-windows.ps1 | iex
#
# It downloads the newest release's setup program (pre-releases included),
# checks it against the release's checksums.txt, and runs it silently. The
# setup installs for the current user only (no administrator rights), to
# %LOCALAPPDATA%\Programs\encore-yt, with a Start menu entry, and closes a
# running copy first.
#
# The setup program isn't signed. Files saved by Invoke-WebRequest don't get
# the Mark of the Web that browsers add, and SmartScreen's "Windows protected
# your PC" check runs only for files that carry it, so the setup starts
# without that prompt. Smart App Control (Windows 11), when it is on, checks
# every program and can still block it.
#
# To install a given release: $env:ENCORE_VERSION = 'v0.1.0-alpha.1' first.
#
# Works in Windows PowerShell 5.1 and PowerShell 7. Everything runs in its
# own scope, so the settings below don't leak into your session.

& {
    $ErrorActionPreference = 'Stop'
    # Windows PowerShell 5.1 draws its progress bar so slowly that it
    # dominates a 100 MB download.
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'jvz-devx/encore-yt'
    $api = "https://api.github.com/repos/$repo"
    $headers = @{ 'User-Agent' = 'encore-yt-install'; 'Accept' = 'application/vnd.github+json' }
    # The setup program's AppId (packaging/windows/encore-yt.iss).
    $uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{6F1C2B8E-4D1A-4B7E-9A55-2D3F1E0C7A91}_is1'

    if (-not [Environment]::Is64BitOperatingSystem) {
        throw 'Encore needs 64-bit Windows.'
    }

    # The pinned release, or the newest one (the list is newest first and
    # includes pre-releases). Piping enumerates the array in 5.1 as well.
    try {
        if ($env:ENCORE_VERSION) {
            $release = Invoke-RestMethod -UseBasicParsing -Headers $headers -Uri "$api/releases/tags/$env:ENCORE_VERSION"
        } else {
            $release = Invoke-RestMethod -UseBasicParsing -Headers $headers -Uri "$api/releases?per_page=1" | Select-Object -First 1
        }
    } catch {
        if ($env:ENCORE_VERSION) { throw "No release $env:ENCORE_VERSION." }
        throw "Couldn't reach GitHub Releases: $($_.Exception.Message)"
    }
    if (-not $release -or -not $release.tag_name) {
        throw "Couldn't find a release."
    }
    $version = $release.tag_name -replace '^v', ''
    $name = "encore-yt-$version-windows-x86_64-setup.exe"
    $asset = $release.assets | Where-Object { $_.name -eq $name } | Select-Object -First 1
    if (-not $asset) {
        throw "Release $($release.tag_name) has no $name."
    }

    $work = Join-Path ([IO.Path]::GetTempPath()) ("encore-yt-install-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $work | Out-Null
    try {
        $setup = Join-Path $work $name
        Write-Host "Downloading Encore $version..."
        Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri $asset.browser_download_url -OutFile $setup

        # The SHA-256 from checksums.txt, else the digest GitHub records
        # (for releases made before checksums.txt).
        $expected = $null
        $sums = $release.assets | Where-Object { $_.name -eq 'checksums.txt' } | Select-Object -First 1
        if ($sums) {
            $text = (Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri $sums.browser_download_url).Content
            if ($text -is [byte[]]) { $text = [Text.Encoding]::UTF8.GetString($text) }
            foreach ($line in ($text -split "`r?`n")) {
                $fields = $line.Trim() -split '\s+'
                if ($fields.Count -eq 2 -and $fields[1].TrimStart('*') -eq $name) {
                    $expected = $fields[0]
                    break
                }
            }
        }
        if (-not $expected -and $asset.PSObject.Properties['digest'] -and $asset.digest) {
            $expected = $asset.digest -replace '^sha256:', ''
        }
        if (-not $expected) {
            throw "The release lists no checksum for $name."
        }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $setup).Hash
        if ($actual -ne $expected) {
            throw "The download doesn't match the release's checksum (expected $expected, got $actual)."
        }
        Write-Host 'Checksum OK. Installing...'

        $switches = '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CLOSEAPPLICATIONS', '/NORESTARTAPPLICATIONS'
        $process = Start-Process -FilePath $setup -ArgumentList $switches -Wait -PassThru
        if ($process.ExitCode -ne 0) {
            throw "The installer failed (exit code $($process.ExitCode))."
        }
    } finally {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }

    $dir = Join-Path $env:LOCALAPPDATA 'Programs\encore-yt'
    $entry = Get-ItemProperty -Path $uninstallKey -ErrorAction SilentlyContinue
    if ($entry -and $entry.InstallLocation) {
        $dir = $entry.InstallLocation.TrimEnd('\')
    }
    Write-Host "Installed Encore $version in $dir."
    Write-Host "Start it from the Start menu (Encore), or run: & `"$dir\encore-yt.exe`""
}
