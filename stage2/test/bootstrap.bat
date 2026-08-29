@echo off
REM ============================================================
REM K5 S8：stage2 自举闭环（全 H 链 + 字节级等价断言）
REM   阶段 A：stage1 interp 执行 stage2 编译器，编译 stage2 全部源码 -> A.hbc
REM           （嵌套解释固有速率 ~12 tok/s，全量 7 文件约 210KB，预计数小时；
REM            耗时仅登记基线，不进验收）
REM   阶段 B：A.hbc（HBC2 VM）执行编译器，编译同一组源码 -> B.hbc
REM   断言 V1：A.hbc 与 B.hbc 逐字节相等（fc /b）
REM 用法：stage2\test\bootstrap.bat
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
