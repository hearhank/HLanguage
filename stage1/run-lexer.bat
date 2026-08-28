@echo off
REM stage1/run-lexer.bat -- Run H lexer, output token stream
REM Usage: run-lexer [file.hc]
REM Default: test_simple.hc

set FILE=%1
if "%FILE%"=="" set FILE=test_simple.hc

echo === H lexer: %FILE% ===
hc run stage1/lexer.hc %FILE%
echo.
echo === Rust lexer (reference): %FILE% ===
hc lex %FILE%