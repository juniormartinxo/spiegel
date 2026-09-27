# SPIKE — throwaway. Must run elevated. Installs the spike media source DLL where the
# Frame Server services (LocalService / LocalSystem) can read it, and registers it in HKLM.
#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$dest = Join-Path $env:ProgramFiles 'SpiegelVCamSpike'
$data = 'C:\ProgramData\SpiegelVCamSpike'
New-Item -ItemType Directory -Force "$data\logs" | Out-Null
Start-Transcript -Path "$data\install-log.txt" -Force | Out-Null
try {
    # Logs: LocalService (Frame Server), SYSTEM (Frame Server Monitor) and apps must be able to write.
    icacls "$data\logs" /grant '*S-1-5-19:(OI)(CI)M' '*S-1-5-18:(OI)(CI)M' '*S-1-5-11:(OI)(CI)M' | Out-Host

    # Frame Server may still hold an old copy of the DLL.
    Stop-Service FrameServer, FrameServerMonitor -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $dest | Out-Null
    # A loaded DLL cannot be overwritten, but it can be renamed; running processes keep the old one.
    $target = "$dest\spiegel_vcam_spike.dll"
    if (Test-Path $target) {
        try { Remove-Item $target -Force }
        catch { Rename-Item $target "spiegel_vcam_spike.dll.old-$([DateTime]::Now.Ticks)"; "DLL em uso: renomeada" }
    }
    Get-ChildItem "$dest\*.old-*" | Remove-Item -Force -ErrorAction SilentlyContinue
    Copy-Item "$root\build\Release\spiegel_vcam_spike.dll" $dest -Force
    icacls "$dest\spiegel_vcam_spike.dll" | Out-Host

    & regsvr32.exe /s "$dest\spiegel_vcam_spike.dll"
    "regsvr32 exit code: $LASTEXITCODE"
    Get-ItemProperty 'HKLM:\SOFTWARE\Classes\CLSID\{6E2F4C1B-9A3D-4E57-B0C8-2D7A15F3E901}\InprocServer32' | Format-List | Out-Host

    if ($args -contains '--system-camera') {
        & "$root\build\Release\vcamctl.exe" add --lifetime system --access all
        "vcamctl add (system, all users) exit code: $LASTEXITCODE"
    }
} finally {
    Stop-Transcript | Out-Null
}
