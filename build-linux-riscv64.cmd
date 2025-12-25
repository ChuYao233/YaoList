@echo off
chcp 65001 >nul
REM 使用 WSL2 交叉编译 Linux RISC-V 64 可执行文件

set "VERSION=1.0.0"
set "OUTPUT_DIR=E:\CodeProject\YaoList\release"
set "OUTPUT_NAME=yaolist-%VERSION%-linux-riscv64"

echo ========================================
echo  YaoList Linux RISC-V 64 Build
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
echo [INFO] Building for Linux RISC-V 64...
echo.

REM 在 WSL 中安装交叉编译工具链并构建 (安装 riscv64 版本 OpenSSL)
wsl -u root bash -c "dpkg --add-architecture riscv64 && apt-get update && apt-get install -y build-essential pkg-config gcc-riscv64-linux-gnu g++-riscv64-linux-gnu perl make libssl-dev:riscv64 && source ~/.cargo/env && rustup target add riscv64gc-unknown-linux-gnu && cd '%WSL_PATH%' && export CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER=riscv64-linux-gnu-gcc && export CC_riscv64gc_unknown_linux_gnu=riscv64-linux-gnu-gcc && export CXX_riscv64gc_unknown_linux_gnu=riscv64-linux-gnu-g++ && export PKG_CONFIG_ALLOW_CROSS=1 && export PKG_CONFIG_PATH=/usr/lib/riscv64-linux-gnu/pkgconfig && export OPENSSL_LIB_DIR=/usr/lib/riscv64-linux-gnu && export OPENSSL_INCLUDE_DIR=/usr/include/riscv64-linux-gnu && cargo build --release --target riscv64gc-unknown-linux-gnu"

if %errorlevel% neq 0 (
    echo.
    echo [ERROR] Build failed!
    pause
    exit /b 1
)

REM 复制到输出目录
copy /Y "target\riscv64gc-unknown-linux-gnu\release\yaolist-backend" "%OUTPUT_DIR%\%OUTPUT_NAME%"

echo.
echo ========================================
echo  Build completed successfully!
echo  Output: %OUTPUT_DIR%\%OUTPUT_NAME%
echo ========================================
echo.

pause
