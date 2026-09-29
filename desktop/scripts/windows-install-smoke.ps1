$ErrorActionPreference = 'Stop'
$andaInstallRoot = Join-Path $env:RUNNER_TEMP 'Anda 安装 smoke'
$andaInstaller = Get-ChildItem "$PSScriptRoot/../release/Anda-win-*.exe" | Select-Object -First 1
if (!$andaInstaller) { throw 'NSIS installer not found' }
# The shared CLI runs from ...\Programs\AndaBot, next to the desktop's
# ...\Programs\Anda. A process in a sibling directory whose name merely starts
# with the install directory must survive install and uninstall.
$siblingDir = $andaInstallRoot + 'Bot'
New-Item -ItemType Directory -Force -Path $siblingDir | Out-Null
Copy-Item (Join-Path $env:SystemRoot 'System32\PING.EXE') (Join-Path $siblingDir 'anda.exe')
$sibling = Start-Process -FilePath (Join-Path $siblingDir 'anda.exe') -ArgumentList @('-n', '600', '127.0.0.1') -WindowStyle Hidden -PassThru
try {
  $andaProcess = Start-Process -FilePath $andaInstaller.FullName -ArgumentList @('/S', "/D=$andaInstallRoot") -Wait -PassThru
  if ($andaProcess.ExitCode -ne 0) { throw "Installer failed: $($andaProcess.ExitCode)" }
  if ($sibling.HasExited) { throw 'Installer stopped a process outside its install directory' }
  if (!(Test-Path (Join-Path $andaInstallRoot 'Anda.exe'))) { throw 'Installed executable missing' }
  $andaRuntime = Join-Path $andaInstallRoot 'resources/runtime/anda.exe'
  & $andaRuntime --version
  if ($LASTEXITCODE -ne 0) { throw 'Bundled runtime did not start' }
  pnpm --dir "$PSScriptRoot/.." exec node scripts/packaged-smoke.mjs "$andaInstallRoot"
  if ($LASTEXITCODE -ne 0) { throw 'Installed Electron application smoke test failed' }
  $andaUninstaller = Get-ChildItem $andaInstallRoot -Filter '*Uninstall*.exe' | Select-Object -First 1
  if (!$andaUninstaller) { throw 'Uninstaller missing' }
  $andaProcess = Start-Process -FilePath $andaUninstaller.FullName -ArgumentList '/S' -Wait -PassThru
  if ($andaProcess.ExitCode -ne 0) { throw "Uninstall failed: $($andaProcess.ExitCode)" }
  if ($sibling.HasExited) { throw 'Uninstaller stopped a process outside its install directory' }
  if (Test-Path (Join-Path $andaInstallRoot 'Anda.exe')) { throw 'Uninstall left the app executable behind' }
} finally {
  Stop-Process -Id $sibling.Id -Force -ErrorAction SilentlyContinue
}
