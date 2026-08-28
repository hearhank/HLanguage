@echo off
REM stage1/run-checker.bat -- Run H checker (semantic analysis)
REM Usage: run-checker [file.hc]
REM Default: test_simple.hc

set FILE=%1
if "%FILE%"=="" set FILE=test_simple.hc

echo === H checker: %FILE% ===
hc run stage1/checker.hc %FILE%
echo.
echo === Rust checker (reference): %FILE% ===
hc check %FILE%