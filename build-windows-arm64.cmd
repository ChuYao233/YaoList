@echo off
setlocal enabledelayedexpansion
chcp 65001 >nul
REM 构建 Windows ARM64 可执行文件
REM 需要安装 Visual Studio ARM64 构建工具

set "VERSION=1.0.0"
set "OUTPUT_DIR=E:\CodeProject\YaoList\release"
set "OUTPUT_NAME=yaolist-%VERSION%-windows-aarch64.exe"

echo ========================================
echo  YaoList Windows ARM64 Build
echo ========================================
echo.

REM 创建输出目录
if not exist "%OUTPUT_DIR%" mkdir "%OUTPUT_DIR%"

REM 查找 Visual Studio
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if exist "%VSWHERE%" (
    for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -property installationPath`) do set "VS_PATH=%%i"
)

if defined VS_PATH (
    echo [INFO] Found Visual Studio at: !VS_PATH!
    REM 调用 vcvarsall 设置 ARM64 交叉编译环境
    call "!VS_PATH!\VC\Auxiliary\Build\vcvarsall.bat" x64_arm64
) else (
    echo [ERROR] Visual Studio not found!
    pause
    exit /b 1
)

echo [INFO] Adding Windows ARM64 target...
rustup target add aarch64-pc-windows-msvc

echo [INFO] Building for Windows ARM64...
echo.

cargo build --release --target aarch64-pc-windows-msvc

if %errorlevel% neq 0 (
    echo.
    echo [ERROR] Build failed!
    pause
    exit /b 1
)

REM 复制到输出目录
copy /Y "target\aarch64-pc-windows-msvc\release\yaolist-backend.exe" "%OUTPUT_DIR%\%OUTPUT_NAME%"

echo.
echo ========================================
echo  Build completed successfully!
echo  Output: %OUTPUT_DIR%\%OUTPUT_NAME%
echo ========================================
echo.

pause
