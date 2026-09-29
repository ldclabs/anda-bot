; electron-builder's default check treats every process whose path starts
; with $INSTDIR as the running app. The default per-user directory
; ...\Programs\Anda is also a prefix of ...\Programs\AndaBot, where the shared
; anda CLI and its daemon run, so installing, updating or uninstalling the
; desktop would terminate them. Match this install directory only.
!macro customCheckAppRunning
  ${if} ${isUpdated}
    ; Let the app exit on its own after quitAndInstall.
    Sleep 1000
  ${endIf}
  nsExec::Exec `"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -Command "$$dir = '$INSTDIR\'; if (@(Get-CimInstance -ClassName Win32_Process | Where-Object { $$_.Path -and $$_.Path.StartsWith($$dir, [StringComparison]::OrdinalIgnoreCase) }).Count -gt 0) { exit 0 } else { exit 1 }"`
  Pop $R0
  ${if} $R0 == 0
    ${ifNot} ${isUpdated}
      MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "$(appRunning)" /SD IDOK IDOK +2
      Quit
    ${endIf}
    DetailPrint "$(appClosing)"
    nsExec::Exec `"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -Command "$$dir = '$INSTDIR\'; Get-CimInstance -ClassName Win32_Process | Where-Object { $$_.Path -and $$_.Path.StartsWith($$dir, [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { Stop-Process -Id $$_.ProcessId -Force -ErrorAction SilentlyContinue }"`
    Pop $R0
    Sleep 1000
  ${endIf}
!macroend
