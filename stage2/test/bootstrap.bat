@echo off
REM ============================================================
REM K5 S8: stage2 self-bootstrap closed loop - chain tiers
REM   (ADR-0032: host chain = daily gate, full H chain = milestone)
REM
REM   fast    [default] host chain: Rust hc runs the stage2
REM           compiler via package mode - tree-walking, ~21s.
REM           Produces A.hbc, then Phase B + V1. Daily gate.
REM   oracle  full H chain: stage1 interp runs the stage2
REM           compiler - nested interpretation, hours. One-off
REM           milestone run; re-runs Phase A unconditionally.
REM           Produces A_oracle.hbc, then Phase B + V1.
REM   resume  same as oracle, but skips Phase A when
REM           A_oracle.hbc already exists - stage-level resume
REM           only; per-file markers in progress.txt are
REM           diagnostics, there is no file-level checkpointing.
REM
REM   Phase B: the A artifact on the HBC2 VM recompiles the same
REM            sources -> B artifact. Assert V1: A == B
REM            byte-identical (fc /b).
REM Progress: stage2\test\progress.txt (markers per file/phase)
REM Usage: stage2\test\bootstrap.bat [fast / oracle / resume]
REM Run from the repo root (paths are repo-relative).
REM ============================================================
setlocal
set SRC=stage2\src\main.hc stage2\src\ir.hc stage2\src\lower.hc stage2\src\encode.hc stage2\src\lexer.hc stage2\src\parser.hc stage2\src\checker.hc
set MODE=%1
if "%MODE%"=="" set MODE=fast

if /i "%MODE%"=="fast" goto :fast
if /i "%MODE%"=="oracle" goto :oracle
if /i "%MODE%"=="resume" goto :resume
echo Unknown mode: %MODE% - use fast, oracle or resume
exit /b 2

:fast
set A=stage2\test\A.hbc
set B=stage2\test\B.hbc
echo [A] host chain: Rust hc runs the stage2 compiler - tree-walking, ~21s
hc run stage2 --emit-hbc %A% %SRC%
if errorlevel 1 goto :fail
goto :phase_b

:oracle
set A=stage2\test\A_oracle.hbc
set B=stage2\test\B_oracle.hbc
echo [A] full H chain: stage1 interp runs the stage2 compiler - hours, one-off milestone
del /q %A% > nul 2>&1
hc run stage1\interp.hc stage2\src\main.hc --emit-hbc %A% %SRC%
if errorlevel 1 goto :fail
goto :phase_b

:resume
set A=stage2\test\A_oracle.hbc
set B=stage2\test\B_oracle.hbc
if exist %A% (
    echo [A] skipped: %A% exists - stage-level resume
    goto :phase_b
)
echo [A] full H chain: stage1 interp runs the stage2 compiler - hours
hc run stage1\interp.hc stage2\src\main.hc --emit-hbc %A% %SRC%
if errorlevel 1 goto :fail

:phase_b
echo [B] %A% on the HBC2 VM recompiles stage2...
hc run %A% --emit-hbc %B% %SRC%
if errorlevel 1 goto :fail

echo [V1] fc /b %A% %B%
fc /b %A% %B% > nul
if errorlevel 1 (
    echo V1 FAIL: A ^= B
    exit /b 1
)
echo V1 PASS: byte-identical
exit /b 0

:fail
echo BOOTSTRAP FAILED
exit /b 1
