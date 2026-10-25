# 골목 섞기 실험. ROUTE_ALLEY_MIX(maps/골목 아래 폴더 이름, 쉼표 구분)를 주고 importer-knl을 실행하여 걸린 시간과 최대 메모리를 출력한다.
# 그래프 폴더의 파일을 덮어쓰므로 실험 뒤에는 ROUTE_ALLEY_MIX 없이 다시 실행하고, 섞은 그래프에서는 alley.rdb를 그래프 폴더 밖으로 옮긴다
# powershell -File scripts/alley/run_mix.ps1 -Mix "서울특별시,경기도" -Log mix.log
param([string]$Mix, [string]$Log)
$env:ROUTE_ALLEY_MIX = $Mix
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath "C:\Users\nocdu\Desktop\dev\route\rust\target\release\importer-knl.exe" -RedirectStandardOutput $Log -RedirectStandardError "$Log.err" -PassThru -NoNewWindow
$peak = 0
while (-not $p.HasExited) {
  try { $p.Refresh(); if ($p.PeakWorkingSet64 -gt $peak) { $peak = $p.PeakWorkingSet64 } } catch {}
  Start-Sleep -Milliseconds 500
}
"exit {0} time {1:N0}s peak {2:N0}MB" -f $p.ExitCode, $sw.Elapsed.TotalSeconds, ($peak/1MB)
