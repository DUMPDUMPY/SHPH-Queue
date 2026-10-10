@echo off
rem Stop SHPH Queue from starting with Windows.
powershell -NoProfile -Command "Remove-Item -ErrorAction SilentlyContinue ([Environment]::GetFolderPath('Startup') + '\SHPH Queue.lnk')"
echo Done. SHPH Queue will no longer start with Windows.
pause
