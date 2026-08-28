@echo off
REM stage1/run-all.bat -- Run lexer + parser + checker sequentially
REM Usage: run-all [file.hc]
REM Default: test_simple.hc

set FILE=%1
if "%FILE%"=="" set FILE=test_simple.hc

echo ========================================
echo  H toolchain (stage1) self-hosting suite
echo  Input: %FILE%
echo ========================================
echo.

echo [1/3] Lexer
call %~dp0run-lexer.bat %FILE%
echo.

echo [2/3] Parser
call %~dp0run-parser.bat %FILE%
echo.

echo [3/3] Checker
call %~dp0run-checker.bat %FILE%
echo.
echo ========================================
echo  Done
echo ========================================