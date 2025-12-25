@echo off
chcp 65001 >nul
REM 构建 Windows x86_64 可执行文件

set "VERSION=1.0.0"
set "OUTPUT_DIR=E:\CodeProject\YaoList\release"
set "OUTPUT_NAME=yaolist-%VERSION%-windows-x86_64.exe"

echo ========================================
echo  YaoList Windows x86_64 Build
echo ========================================
echo.

REM 创建输出目录
if not exist "%OUTPUT_DIR%" mkdir "%OUTPUT_DIR%"

echo [INFO] Building for Windows x86_64...
echo.

cargo build --release

if %errorlevel% neq 0 (
    echo.
    echo [ERROR] Build failed!
    pause
    exit /b 1
)

REM 复制到输出目录
copy /Y "target\release\yaolist-backend.exe" "%OUTPUT_DIR%\%OUTPUT_NAME%"

echo.
echo ========================================
echo  Build completed successfully!
echo  Output: %OUTPUT_DIR%\%OUTPUT_NAME%
echo ========================================
echo.

pause
