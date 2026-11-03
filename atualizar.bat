@echo off
cd /d "%~dp0"
title MyDigitalBuddy - atualizar EXE
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0atualizar.ps1" %*
exit /b %ERRORLEVEL%
