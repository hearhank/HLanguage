@echo off
REM ============================================================
REM K5 S8: stage2 self-bootstrap closed loop - host chain (fast)
REM   (ADR-0033: oracle/resume explanation-chain modes retired;
REM    the binary chain supersedes them - see
REM    docs/SPEC/phase4/09-bootstrap-binary-chain-plan.md)
REM
REM   fast [default] host chain: Rust hc runs the stage2
REM   compiler via package mode - tree-walking, ~21s.
REM   Produces A.hbc, then Phase B + V1. Daily gate.
REM
REM   Phase B: A.hbc on the HBC2 VM recompiles the same sources
REM            -> B.hbc. Assert V1: A == B byte-identical (fc /b).
REM Progress: stage2\test\progress.txt (markers per file/phase)
REM Usage: stage2\test\bootstrap.bat
REM Run from the repo root (paths are repo-relative).
REM ============================================================
setlocal
set SRC=stage2\src\main.hc stage2\src\ir.hc stage2\src\lower.hc stage2\src\encode.hc stage2\src\lexer.hc stage2\src\parser.hc stage2\src\checker.hc

set A=stage2\test\A.hbc
set B=stage2\test\B.hbc

echo [A] host chain: Rust hc runs the stage2 compiler - tree-walking, ~21s
hc run stage2 --emit-hbc %A% %SRC%
if errorlevel 1 goto :fail

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
