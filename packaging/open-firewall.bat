@echo off
rem Allow other computers on the LAN to reach SHPH Queue on port 8000.
rem Right-click this file and choose "Run as administrator".
net session >nul 2>&1
if errorlevel 1 (
  echo Please right-click this file and choose "Run as administrator".
  pause
  exit /b 1
)
netsh advfirewall firewall delete rule name="SHPH Queue" >nul 2>&1
netsh advfirewall firewall add rule name="SHPH Queue" dir=in action=allow protocol=TCP localport=8000 profile=private,domain
echo Done. Port 8000 is open for the private network.
pause
