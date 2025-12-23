@echo off
chcp 65001 >nul
REM 使用 WSL2 构建 Linux x86_64 可执行文件

echo ========================================
echo  YaoList Backend Linux Build Script
echo ========================================
echo.

REM 使用固定的 WSL 路径（Windows 驱动器在 WSL 中挂载为 /mnt/盘符）
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

REM 在 WSL 中使用 root 用户构建
REM SMB 使用系统原生 CIFS 挂载，无需 libsmbclient
wsl -u root bash -c "apt-get update && apt-get install -y build-essential pkg-config libssl-dev cifs-utils && source ~/.cargo/env && cd '%WSL_PATH%' && cargo build --release"

if %errorlevel% neq 0 (
    echo.
    echo [ERROR] Build failed!
    pause
    exit /b 1
)

echo.
echo ========================================
echo  Build completed successfully!
echo  Output: target/release/yaolist-backend
echo ========================================
echo.

pause
