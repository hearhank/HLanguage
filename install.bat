@echo off
setlocal EnableExtensions
title H 语言工具链 · 编译并安装

REM ================================================================
REM  H 语言工具链 · 一键编译 + 安装
REM
REM  功能：
REM    1) cargo build --release 编译 tag1/ 工作区
REM    2) 把 hc.exe 复制到 Cargo bin 目录（%CARGO_HOME%\bin，
REM       默认 %USERPROFILE%\.cargo\bin，该目录通常在 PATH 中）
REM    3) 运行 hc --version 验证安装
REM
REM  用法：双击本文件，或命令行执行  install.bat
REM  依赖：Rust / cargo（zig 可选，仅原生编译模式需要）
REM  注意：本文件须放在仓库根目录（与 tag1/ 同级）。
REM ================================================================

set "TAG1=%~dp0tag1"
set "BINDIR=%CARGO_HOME%\bin"
if "%BINDIR%"=="\bin" set "BINDIR=%USERPROFILE%\.cargo\bin"

echo.
echo === H 语言工具链：编译并安装 ===
echo   源码目录 : %TAG1%
echo   安装目录 : %BINDIR%
echo.

REM ---------- 1. 编译 Release ----------
echo [1/3] cargo build --release ...
pushd "%TAG1%"
if errorlevel 1 (
    echo [错误] 找不到目录：%TAG1%
    echo        本文件须放在仓库根目录（与 tag1/ 同级）。
    exit /b 1
)
call cargo build --release
if errorlevel 1 (
    echo [错误] 编译失败。请确认已安装 Rust 工具链（rustup/cargo）。
    popd
    exit /b 1
)
popd

REM ---------- 2. 安装 ----------
if not exist "%BINDIR%" mkdir "%BINDIR%"
echo [2/3] 安装 hc.exe 到 %BINDIR% ...
copy /y "%TAG1%\target\release\hc.exe" "%BINDIR%\hc.exe" >nul
if errorlevel 1 (
    echo [错误] 复制失败。若 hc 正在运行，请先关闭（taskkill /IM hc.exe）后重试。
    exit /b 1
)

REM ---------- 3. 验证 ----------
echo [3/3] 验证安装 ...
"%BINDIR%\hc.exe" --version
if errorlevel 1 (
    echo [警告] hc --version 执行异常，请检查安装目录权限。
    exit /b 1
)

REM ---------- PATH 提示（不自动改写注册表，安全优先） ----------
where hc >nul 2>nul
if errorlevel 1 (
    echo.
    echo [提示] %BINDIR% 不在当前 PATH 中。请在系统设置中追加该目录，
    echo        或在命令行执行：
    echo            setx PATH "%BINDIR%;%%PATH%%"
    echo        然后新开一个终端使用 hc。
) else (
    echo.
    echo 安装完成！hc 已在 PATH，可直接使用：
    echo   hc run examples/hello.hc      解释运行
    echo   hc build examples/hello.hc    原生编译（需 zig）
    echo   hc test examples/             运行测试
    echo   hc --help                     更多命令
)

REM ---------- zig 检查（仅提示） ----------
where zig >nul 2>nul
if errorlevel 1 (
    echo.
    echo [提示] 未检测到 zig。原生编译（hc build / hc test --mode=compile）需要 zig cc；
    echo        缺失时 hc build 自动回退字节码产物，脚本模式不受影响。
)

echo.
pause
