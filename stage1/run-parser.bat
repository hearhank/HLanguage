@echo off
REM stage1/run-parser.bat -- Run H parser, output AST tree
REM Usage: run-parser [file.hc]
REM Default: test_simple.hc

set FILE=%1
if "%FILE%"=="" set FILE=test_simple.hc

echo === H parser: %FILE% ===
hc run stage1/parser.hc %FILE%
echo.
echo === Rust parser (reference): %FILE% ===
hc parse %FILE%