@echo off
chcp 65001 >nul
REM 使用 WSL2 交叉编译 Linux ARM64 可执行文件

set "VERSION=1.0.0"
set "OUTPUT_DIR=E:\CodeProject\YaoList\release"
set "OUTPUT_NAME=yaolist-%VERSION%-linux-aarch64"

echo ========================================
echo  YaoList Linux ARM64 Build
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
echo [INFO] Building for Linux ARM64 (aarch64)...
echo.

REM 在 WSL 中安装交叉编译工具链并构建
wsl -u root bash -c "dpkg --add-architecture arm64 && apt-get update && apt-get install -y build-essential pkg-config gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libssl-dev:arm64 && source ~/.cargo/env && rustup target add aarch64-unknown-linux-gnu && cd '%WSL_PATH%' && export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc && export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc && export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ && export PKG_CONFIG_ALLOW_CROSS=1 && export PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig && export OPENSSL_LIB_DIR=/usr/lib/aarch64-linux-gnu && export OPENSSL_INCLUDE_DIR=/usr/include/aarch64-linux-gnu && cargo build --release --target aarch64-unknown-linux-gnu"

if %errorlevel% neq 0 (
    echo.
    echo [ERROR] Build failed!
    pause
    exit /b 1
)

REM 复制到输出目录
copy /Y "target\aarch64-unknown-linux-gnu\release\yaolist-backend" "%OUTPUT_DIR%\%OUTPUT_NAME%"

echo.
echo ========================================
echo  Build completed successfully!
echo  Output: %OUTPUT_DIR%\%OUTPUT_NAME%
echo ========================================
echo.

pause
