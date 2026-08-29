@echo off
REM stage1/k4test/run-compare.bat -- K4 parity: Rust reference vs H interp, diff per corpus file
REM Usage: run-compare.bat   (any cwd; script locates repo root)
REM Output: per-file MATCH/DIFF table; snapshot stage1/k4test/compare-latest.txt
REM         DIFF outputs kept as diff-<name>-ref.txt / diff-<name>-int.txt for inspection
REM Exit code: 0 = all MATCH, 1 = any DIFF

setlocal enabledelayedexpansion
cd /d %~dp0..\..
set SNAP=stage1\k4test\compare-latest.txt
if exist %SNAP% del %SNAP%
set /a PASS=0
set /a FAIL=0

for %%F in (stage1\exec-corpus\*.hc) do call :compare %%F

echo.
echo === Summary: !PASS! MATCH / !FAIL! DIFF ===
echo === Summary: !PASS! MATCH / !FAIL! DIFF ===>> %SNAP%
if !FAIL! GTR 0 exit /b 1
exit /b 0

:compare
set F=%1
set NAME=%~n1
hc run %F% > stage1\k4test\tmp-ref-%NAME%.txt 2>&1
hc run stage1\interp.hc %F% > stage1\k4test\tmp-int-%NAME%.txt 2>&1
fc /b stage1\k4test\tmp-ref-%NAME%.txt stage1\k4test\tmp-int-%NAME%.txt > nul 2>&1
if errorlevel 1 (
    echo [DIFF] %NAME%
    echo [DIFF] %NAME%>> %SNAP%
    move /y stage1\k4test\tmp-ref-%NAME%.txt stage1\k4test\diff-%NAME%-ref.txt > nul
    move /y stage1\k4test\tmp-int-%NAME%.txt stage1\k4test\diff-%NAME%-int.txt > nul
    set /a FAIL+=1
) else (
    echo [MATCH] %NAME%
    echo [MATCH] %NAME%>> %SNAP%
    del stage1\k4test\tmp-ref-%NAME%.txt stage1\k4test\tmp-int-%NAME%.txt 2> nul
    set /a PASS+=1
)
exit /b 0
