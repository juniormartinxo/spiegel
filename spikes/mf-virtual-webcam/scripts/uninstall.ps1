# SPIKE — throwaway. Must run elevated. Removes every trace of the spike camera.
#Requires -RunAsAdministrator
$root = Split-Path $PSScriptRoot
$ctl = "$root\build\Release\vcamctl.exe"
foreach ($lt in 'system', 'session') { foreach ($ac in 'all', 'user') { & $ctl remove --lifetime $lt --access $ac } }
$dll = Join-Path $env:ProgramFiles 'SpiegelVCamSpike\spiegel_vcam_spike.dll'
if (Test-Path $dll) { & regsvr32.exe /s /u $dll; "regsvr32 /u exit code: $LASTEXITCODE" }
Stop-Service FrameServer, FrameServerMonitor -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $env:ProgramFiles 'SpiegelVCamSpike') -Recurse -Force -ErrorAction SilentlyContinue
"Logs kept in C:\ProgramData\SpiegelVCamSpike (delete by hand)."
