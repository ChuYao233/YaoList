@echo off
chcp 65001 >nul
REM 使用 WSL2 构建 Linux x86_64 可执行文件

set "VERSION=1.0.0"
set "OUTPUT_DIR=E:\CodeProject\YaoList\release"
set "OUTPUT_NAME=yaolist-%VERSION%-linux-x86_64"

echo ========================================
echo  YaoList Linux x86_64 Build
echo ========================================
echo.

REM 创建输出目录
if not exist "%OUTPUT_DIR%" mkdir "%OUTPUT_DIR%"

REM 使用固定的 WSL 路径
set "DRIVE_LETTER=%cd:~0,1%"
set "PATH_REST=%cd:~2%"
set "PATH_REST=%PATH_REST:\=/%"

REM 转换为小写驱动器号
for %%a in (a b c d e f g h i j k l m n o p q r s t u v w x y z) do (
    if /i "%DRIVE_LETTER%"=="%%a" set "DRIVE_LOWER=%%a"
)

set "WSL_PATH=/mnt/%DRIVE_LOWER%%PATH_REST%"

echo [INFO] WSL path: %WSL_PATH%
echo [INFO] Building for Linux x86_64...
echo.

REM 在 WSL 中构建
wsl -u root bash -c "apt-get update && apt-get install -y build-essential pkg-config libssl-dev && source ~/.cargo/env && cd '%WSL_PATH%' && cargo build --release"

if %errorlevel% neq 0 (
    echo.
    echo [ERROR] Build failed!
    pause
    exit /b 1
)

REM 复制到输出目录
copy /Y "target\release\yaolist-backend" "%OUTPUT_DIR%\%OUTPUT_NAME%"

echo.
echo ========================================
echo  Build completed successfully!
echo  Output: %OUTPUT_DIR%\%OUTPUT_NAME%
echo ========================================
echo.

pause
