@echo off
REM ============================================================
REM K5 S8: stage2 self-bootstrap closed loop (full H chain)
REM   Phase A: stage1 interp runs stage2 compiler, compiles all
REM            stage2 sources -> A.hbc  (nested interpretation,
REM            ~hours; time recorded as baseline only)
REM   Phase B: A.hbc (HBC2 VM) recompiles same sources -> B.hbc
REM   Assert V1: A.hbc == B.hbc byte-identical (fc /b)
REM Progress: stage2\test\progress.txt (markers per file/phase)
REM Usage: stage2\test\bootstrap.bat
REM ============================================================
setlocal
set SRC=stage2\src\main.hc stage2\src\ir.hc stage2\src\lower.hc stage2\src\encode.hc stage2\src\lexer.hc stage2\src\parser.hc stage2\src\checker.hc

echo [A] stage1 interp runs stage2 compiler (slow, hours expected)...
hc run stage1\interp.hc stage2\src\main.hc --emit-hbc stage2\test\A.hbc %SRC%
if errorlevel 1 goto :fail

echo [B] A.hbc (HBC2 VM) recompiles stage2...
hc run stage2\test\A.hbc --emit-hbc stage2\test\B.hbc %SRC%
if errorlevel 1 goto :fail

fc /b stage2\test\A.hbc stage2\test\B.hbc > nul
if errorlevel 1 (
    echo V1 FAIL: A.hbc ^= B.hbc
    exit /b 1
)
echo V1 PASS: A.hbc == B.hbc byte-identical
exit /b 0

:fail
echo BOOTSTRAP FAILED
exit /b 1
