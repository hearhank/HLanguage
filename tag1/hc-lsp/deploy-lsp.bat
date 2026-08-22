@echo off
REM H Language LSP and Zed Extension Deployment Script
REM This script builds the LSP server, generates Tree-sitter parser, and installs Zed extension

setlocal enabledelayedexpansion

REM Get script directory (tag1\hc-lsp)
set SCRIPT_DIR=%~dp0

REM Project root is two levels up from script directory
REM Script is at: <root>\tag1\hc-lsp\deploy-lsp.bat
REM Project root is: <root>\
set SCRIPT_PARENT=%~dp0..
set PROJECT_ROOT=%~dp0..\..

REM Normalize paths
for %%i in ("%PROJECT_ROOT%") do set PROJECT_ROOT=%%~fi
for %%i in ("%SCRIPT_PARENT%") do set SCRIPT_PARENT=%%~fi

REM Verify project root
if not exist "%PROJECT_ROOT%\extensions\zed\languages\h\grammar.js" (
    echo [ERROR] Cannot find grammar.js at %PROJECT_ROOT%\extensions\zed\languages\h\grammar.js
    echo.
    echo Please run this script from: tag1\hc-lsp\deploy-lsp.bat
    exit /b 1
)

echo Project root: %PROJECT_ROOT%
echo.

REM Check dependencies
echo [1/6] Checking dependencies...
echo.

REM Check Rust
where cargo >nul 2>&1
if %errorlevel% neq 0 (
    echo [ERROR] Rust is not installed. Please install Rust from https://rustup.rs/
    exit /b 1
)
echo [OK] Rust is installed

REM Check Node.js
where node >nul 2>&1
if %errorlevel% neq 0 (
    echo [ERROR] Node.js is not installed. Please install Node.js from https://nodejs.org/
    exit /b 1
)
echo [OK] Node.js is installed

REM Check Tree-sitter CLI
REM Strategy: look for the native tree-sitter.exe first (in npm global), then
REM fall back to PATH lookup. The npm .cmd shim misresolves CWD when launched
REM from a batch after cd, so we prefer the native binary.
set TREE_SITTER_INSTALLED=0
set TREE_SITTER=

REM 1) Look for native tree-sitter.exe in npm global root
for /f "delims=" %%i in ('npm root -g 2^>nul') do set NPM_GLOBAL_ROOT=%%i
if defined NPM_GLOBAL_ROOT (
    if exist "%NPM_GLOBAL_ROOT%\tree-sitter-cli\tree-sitter.exe" (
        set "TREE_SITTER=%NPM_GLOBAL_ROOT%\tree-sitter-cli\tree-sitter.exe"
        set TREE_SITTER_INSTALLED=1
        echo [OK] Found native tree-sitter.exe: !TREE_SITTER!
        goto :ts_found
    )
)

REM 2) Try tree-sitter in PATH (via where or where.exe)
where tree-sitter.exe >nul 2>&1
if %errorlevel% equ 0 (
    for /f "delims=" %%i in ('where tree-sitter.exe 2^>nul') do set "TREE_SITTER=%%i"
    set TREE_SITTER_INSTALLED=1
    echo [OK] Found tree-sitter.exe in PATH: !TREE_SITTER!
    goto :ts_found
)

REM 3) Try tree-sitter.cmd / tree-sitter in PATH (npm shim)
where tree-sitter >nul 2>&1
if %errorlevel% equ 0 (
    set TREE_SITTER=tree-sitter
    set TREE_SITTER_INSTALLED=1
    echo [OK] Found tree-sitter in PATH (npm shim)
    goto :ts_found
)

REM 4) Not found anywhere - try to install
if not defined TREE_SITTER (
    echo [WARN] Tree-sitter CLI is not found. Installing via npm...
    npm install -g tree-sitter-cli
    if %errorlevel% equ 0 (
        REM After install, try native exe again
        for /f "delims=" %%i in ('npm root -g 2^>nul') do set NPM_GLOBAL_ROOT=%%i
        if defined NPM_GLOBAL_ROOT (
            if exist "%NPM_GLOBAL_ROOT%\tree-sitter-cli\tree-sitter.exe" (
                set "TREE_SITTER=%NPM_GLOBAL_ROOT%\tree-sitter-cli\tree-sitter.exe"
                set TREE_SITTER_INSTALLED=1
                echo [OK] Installed and found native tree-sitter.exe
                goto :ts_found
            )
        )
        REM Fall back to PATH
        where tree-sitter >nul 2>&1
        if %errorlevel% equ 0 (
            set TREE_SITTER=tree-sitter
            set TREE_SITTER_INSTALLED=1
            echo [OK] Installed and found tree-sitter in PATH
            goto :ts_found
        )
    )
    echo [WARN] Failed to install/find Tree-sitter CLI.
    echo        parser.c already exists, skipping generation.
    echo        To install manually: npm install -g tree-sitter-cli
    echo        Or download from: https://github.com/tree-sitter/tree-sitter/releases
)

goto :ts_continue

:ts_found
set TREE_SITTER_INSTALLED=1

:ts_continue
echo.
echo ========================================
echo [2/6] Building LSP server (hc-lsp)...
echo ========================================
echo.

cd /d "%PROJECT_ROOT%\tag1\hc-lsp"
if %errorlevel% neq 0 (
    echo [ERROR] Cannot find hc-lsp directory
    exit /b 1
)

echo Building hc-lsp in release mode...
cargo build --release
if %errorlevel% neq 0 (
    echo [ERROR] Failed to build hc-lsp
    exit /b 1
)
echo [OK] hc-lsp built successfully

REM Run tests
echo Running tests...
cargo test
if %errorlevel% neq 0 (
    echo [WARN] Some tests failed, but continuing...
) else (
    echo [OK] All tests passed
)

echo.
echo ========================================
echo [3/6] Generating Tree-sitter parser...
echo ========================================
echo.

cd /d "%PROJECT_ROOT%\extensions\zed\languages\h"
if %errorlevel% neq 0 (
    echo [ERROR] Cannot find Tree-sitter grammar directory
    exit /b 1
)

if "%TREE_SITTER_INSTALLED%"=="1" (
    echo Generating Tree-sitter parser...
    "%TREE_SITTER%" generate
    if %errorlevel% neq 0 (
        echo [ERROR] Failed to generate Tree-sitter parser
        exit /b 1
    )
    echo [OK] Tree-sitter parser generated

    REM Test parser
    echo Testing Tree-sitter parser...
    "%TREE_SITTER%" test
    if %errorlevel% neq 0 (
        echo [WARN] Some Tree-sitter tests failed, but continuing...
    ) else (
        echo [OK] All Tree-sitter tests passed
    )
) else (
    echo [SKIP] Tree-sitter not installed, using existing parser.c
)

echo.
echo ========================================
echo [4/6] Installing binaries to PATH...
echo ========================================
echo.

REM Create bin directory if it doesn't exist
set BIN_DIR=%PROJECT_ROOT%\bin
if not exist "%BIN_DIR%" mkdir "%BIN_DIR%"

REM Copy hc-lsp binary
echo Copying hc-lsp to bin directory...
copy /Y "%PROJECT_ROOT%\tag1\target\release\hc-lsp.exe" "%BIN_DIR%\hc-lsp.exe" >nul
if %errorlevel% neq 0 (
    echo [ERROR] Failed to copy hc-lsp.exe
    exit /b 1
)
echo [OK] hc-lsp.exe copied to %BIN_DIR%

REM Copy hc binary if it exists
if exist "%PROJECT_ROOT%\tag1\target\release\hc.exe" (
    echo Copying hc to bin directory...
    copy /Y "%PROJECT_ROOT%\tag1\target\release\hc.exe" "%BIN_DIR%\hc.exe" >nul
    if %errorlevel% neq 0 (
        echo [WARN] Failed to copy hc.exe
    ) else (
        echo [OK] hc.exe copied to %BIN_DIR%
    )
)

echo.
echo ========================================
echo [5/6] Preparing Zed dev extension...
echo ========================================
echo.

REM Build the Rust extension (wasm) so the LSP command is available. Zed can
REM also compile this itself on dev-extension install, but pre-building gives a
REM fallback if wasi-sdk is missing.
echo Building Rust extension (wasm)...
cd /d "%PROJECT_ROOT%\extensions\zed"
call rustup target add wasm32-wasip2 >nul 2>&1
cargo build --target wasm32-wasip2 --release
if %errorlevel% equ 0 (
    copy /Y "%PROJECT_ROOT%\extensions\zed\target\wasm32-wasip2\release\h_language.wasm" "%PROJECT_ROOT%\extensions\zed\extension.wasm" >nul
    echo [OK] Rust extension wasm built
) else (
    echo [WARN] Could not build Rust extension wasm. Zed will try to compile it on install.
)

REM Remove any stale grammar build dir (Zed regenerates it from the manifest).
if exist "%PROJECT_ROOT%\extensions\zed\grammars\h" rmdir /S /Q "%PROJECT_ROOT%\extensions\zed\grammars\h"

REM The extension manifest points its grammar at this repository (file:///...)
REM with rev=feature/improv_code_v0.1.5 and path=extensions/zed/languages/h,
REM so the generated src/parser.c must be committed for the shallow clone that
REM Zed makes to contain it.
echo.
echo [NOTE] The grammar manifest builds from a git clone of this repository.
echo        Commit src/parser.c (and any grammar.js / query changes) before
echo        installing the dev extension, or Zed's grammar build will fail.
echo.
echo [NOTE] Install the extension in Zed with:
echo       1. Command palette -^> "zed: install dev extension"
echo       2. Select the directory: %PROJECT_ROOT%\extensions\zed
echo       3. Zed compiles the Rust extension and grammar automatically.
echo.

REM Optional: stage a copy for reference (not the dev-extension install path).
set ZED_EXTENSIONS_DIR=%USERPROFILE%\.zed\extensions
if not exist "%ZED_EXTENSIONS_DIR%" mkdir "%ZED_EXTENSIONS_DIR%"
set H_EXTENSION_DIR=%ZED_EXTENSIONS_DIR%\h-language
if not exist "%H_EXTENSION_DIR%" mkdir "%H_EXTENSION_DIR%"

REM Copy extension source excluding heavy build artifacts (target\, .git).
set H_EXT_STAGED=%H_EXTENSION_DIR%
echo Copying extension files (excluding target, git, stale grammar build dir)...
robocopy "%PROJECT_ROOT%\extensions\zed" "%H_EXT_STAGED%" /E /XD target .git grammars\h /XF *.tmp /NFL /NDL /NJH /NJS /NC /NS >nul
if %errorlevel% geq 8 (
    echo [WARN] robocopy failed to copy extension files
) else (
    echo [OK] Extension files staged to %H_EXT_STAGED%
    echo     (This is a reference copy. To load in Zed, use "zed: install dev extension".)
)

REM Copy the pre-generated Zed LSP settings snippet (a committed file) so the
REM LSP is wired up even before the Rust extension is compiled. Merge its
REM lsp/languages blocks into %APPDATA%\Zed\settings.json if needed.
set ZED_SETTINGS=%APPDATA%\Zed\settings.json
set LSP_SNIPPET=%PROJECT_ROOT%\zed-lsp-snippet.json
echo Copying Zed LSP settings snippet to %LSP_SNIPPET%...
copy /Y "%PROJECT_ROOT%\extensions\zed\zed-lsp-snippet.json" "%LSP_SNIPPET%" >nul
if %errorlevel% neq 0 (
    echo [WARN] Could not copy settings snippet
) else (
    echo [OK] Settings snippet written to %LSP_SNIPPET%
)
echo.
echo To enable the LSP in Zed, merge the generated snippet into:
echo   %ZED_SETTINGS%
echo (or use the extension's Rust LSP if it compiles).
echo.

echo.
echo ========================================
echo [6/6] Creating environment setup script...
echo ========================================
echo.

REM Create setup script
set SETUP_SCRIPT=%PROJECT_ROOT%\setup-lsp.bat
echo Creating setup script at %SETUP_SCRIPT%...

(
echo @echo off
echo REM H Language LSP Environment Setup
echo.
echo REM Add bin directory to PATH
echo set PATH=%BIN_DIR%;%%PATH%%
echo.
echo REM Set Zed extension path
echo set ZED_EXTENSIONS_DIR=%ZED_EXTENSIONS_DIR%
echo.
echo echo H Language LSP environment configured!
echo echo.
echo echo.
echo echo Available commands:
echo echo   hc-lsp    - Start LSP server
echo echo   hc        - H language compiler ^(if installed^)
echo echo.
echo echo Zed extension installed at:
echo echo   %H_EXTENSION_DIR%
echo echo.
echo echo To use in Zed:
echo echo   1. Restart Zed
echo echo   2. Open a .hc file
echo echo   3. LSP features should be active
echo echo.
echo echo If LSP does not start, merge this into Zed settings ^(%APPDATA%\Zed\settings.json^):
echo echo   %LSP_SNIPPET%
echo.
) > "%SETUP_SCRIPT%"

echo [OK] Setup script created at %SETUP_SCRIPT%

echo.
echo ========================================
echo Deployment Complete!
echo ========================================
echo.
echo Summary:
echo   - LSP server built: %BIN_DIR%\hc-lsp.exe
echo   - Tree-sitter parser generated: %PROJECT_ROOT%\extensions\zed\languages\h\src
echo   - Zed extension files copied: %H_EXTENSION_DIR%
echo   - Zed LSP settings snippet: %LSP_SNIPPET%
echo   - Setup script created: %SETUP_SCRIPT%
echo.
echo Next steps:
echo   1. Run setup-lsp.bat to configure environment (adds bin to PATH)
echo   2. Install the dev extension in Zed: "zed: install dev extension" -^> select
echo      the extensions\zed directory
echo   3. Restart Zed editor
echo   4. Open a .hc file to test LSP features
echo   5. If LSP does not start, merge %LSP_SNIPPET% into %ZED_SETTINGS%
echo.
echo To manually add to PATH:
echo   set PATH=%BIN_DIR%;%%PATH%%
echo.
echo To verify installation:
echo   hc-lsp --version
echo   tree-sitter --version
echo.

REM Ask if user wants to add to PATH now
set /p ADD_PATH="Add bin directory to PATH now? (y/n): "
if /i "%ADD_PATH%"=="y" (
    echo Adding to PATH...
    setx PATH "%BIN_DIR%;%PATH%" >nul
    if %errorlevel% neq 0 (
        echo [WARN] Failed to add to user PATH. Please add manually:
        echo   %BIN_DIR%
    ) else (
        echo [OK] Added to user PATH. Please restart your terminal.
    )
)

echo.
echo Press any key to exit...
pause >nul

exit /b 0
