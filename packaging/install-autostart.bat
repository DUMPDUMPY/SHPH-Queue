@echo off
rem Start SHPH Queue automatically when this user logs in to Windows (minimized window).
set "EXE=%~dp0shph-queue.exe"
set "DIR=%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$s = (New-Object -ComObject WScript.Shell).CreateShortcut([Environment]::GetFolderPath('Startup') + '\SHPH Queue.lnk');" ^
  "$s.TargetPath = '%EXE%'; $s.WorkingDirectory = '%DIR%'; $s.WindowStyle = 7; $s.Save()"
if errorlevel 1 (
  echo Could not create the startup shortcut.
) else (
  echo Done. SHPH Queue will start when Windows starts.
)
pause
