@echo off
REM H Language LSP and Zed Extension Deployment Script
REM This script builds the LSP server, generates Tree-sitter parser, and installs Zed extension

setlocal enabledelayedexpansion

echo ========================================
echo H Language LSP Deployment Script
echo ========================================
echo.

REM Get script directory
set SCRIPT_DIR=%~dp0
set PROJECT_ROOT=%SCRIPT_DIR%..\..

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
where tree-sitter >nul 2>&1
if %errorlevel% neq 0 (
    echo [WARN] Tree-sitter CLI is not installed. Installing...
    echo Installing Tree-sitter CLI with allow-scripts...
    npm install -g --allow-scripts=tree-sitter-cli tree-sitter-cli
    if %errorlevel% neq 0 (
        echo [ERROR] Failed to install Tree-sitter CLI
        echo.
        echo Please try one of the following:
        echo   1. Run: npm install -g --allow-scripts=tree-sitter-cli tree-sitter-cli
        echo   2. Or: npm config set allow-scripts=tree-sitter-cli --location=user
        echo      Then: npm install -g tree-sitter-cli
        echo   3. Or install manually from: https://github.com/tree-sitter/tree-sitter/releases
        exit /b 1
    )
    echo [OK] Tree-sitter CLI installed
) else (
    echo [OK] Tree-sitter CLI is installed
)

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

echo Generating Tree-sitter parser...
tree-sitter generate
if %errorlevel% neq 0 (
    echo [ERROR] Failed to generate Tree-sitter parser
    exit /b 1
)
echo [OK] Tree-sitter parser generated

REM Test parser
echo Testing Tree-sitter parser...
tree-sitter test
if %errorlevel% neq 0 (
    echo [WARN] Some Tree-sitter tests failed, but continuing...
) else (
    echo [OK] All Tree-sitter tests passed
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
echo [5/6] Configuring Zed extension...
echo ========================================
echo.

REM Get Zed extensions directory
set ZED_EXTENSIONS_DIR=%USERPROFILE%\.zed\extensions
if not exist "%ZED_EXTENSIONS_DIR%" mkdir "%ZED_EXTENSIONS_DIR%"

REM Create extension directory
set H_EXTENSION_DIR=%ZED_EXTENSIONS_DIR%\h-language
if not exist "%H_EXTENSION_DIR%" mkdir "%H_EXTENSION_DIR%"

REM Copy extension files
echo Copying Zed extension files...
xcopy /E /I /Y "%PROJECT_ROOT%\extensions\zed\*" "%H_EXTENSION_DIR%\"
if %errorlevel% neq 0 (
    echo [ERROR] Failed to copy extension files
    exit /b 1
)
echo [OK] Extension files copied to %H_EXTENSION_DIR%

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
echo echo Available commands:
echo echo   hc-lsp    - Start LSP server
echo echo   hc        - H language compiler ^(if installed^)
echo echo.
echo echo Zed extension installed at:
echo echo   %H_EXTENSION_DIR%
echo echo.
echo echo To use in Zed:
echo echo   1. Restart Zed
echo echo   2. Open a .h file
echo echo   3. LSP features should be active
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
echo   - Zed extension installed: %H_EXTENSION_DIR%
echo   - Setup script created: %SETUP_SCRIPT%
echo.
echo Next steps:
echo   1. Run setup-lsp.bat to configure environment
echo   2. Restart Zed editor
echo   3. Open a .h file to test LSP features
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
