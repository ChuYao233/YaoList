#!/bin/bash
#
# YaoList 一键安装管理脚本
#

APP_NAME="yaolist"
GITHUB_REPO="ChuYao233/YaoList"
INSTALL_DIR="/opt/yaolist"
DATA_DIR="${INSTALL_DIR}/data"
CONFIG_FILE="${INSTALL_DIR}/config.json"
LOG_FILE="${INSTALL_DIR}/yaolist.log"
BINARY_NAME="yaolist"
SERVICE_NAME="yaolist"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; }
success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }

check_root() {
    if [ "$EUID" -ne 0 ]; then
        error "请使用 root 用户运行此脚本"
        exit 1
    fi
}

get_arch() {
    arch=$(uname -m)
    case $arch in
        x86_64|amd64) echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        riscv64) echo "riscv64" ;;
        *) error "不支持的架构: $arch"; exit 1 ;;
    esac
}

get_latest_version() {
    curl -fsSL "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" 2>/dev/null | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/'
}

get_current_version() {
    if [ -f "${INSTALL_DIR}/${BINARY_NAME}" ]; then
        "${INSTALL_DIR}/${BINARY_NAME}" --version 2>/dev/null | head -1 || echo "unknown"
    else
        echo "未安装"
    fi
}

download_binary() {
    version=$1
    arch=$(get_arch)
    # 文件名格式: yaolist-{version}-linux-{arch}
    filename="yaolist-${version}-linux-${arch}"
    url="https://github.com/${GITHUB_REPO}/releases/download/${version}/${filename}"
    
    info "下载 YaoList ${version} (linux/${arch})..."
    mkdir -p "$INSTALL_DIR"
    
    if curl -fsSL "$url" -o "${INSTALL_DIR}/${BINARY_NAME}.new"; then
        chmod +x "${INSTALL_DIR}/${BINARY_NAME}.new"
        mv "${INSTALL_DIR}/${BINARY_NAME}.new" "${INSTALL_DIR}/${BINARY_NAME}"
        success "下载完成"
        return 0
    else
        error "下载失败"
        return 1
    fi
}

create_service() {
    info "创建 systemd 服务..."
    cat > /etc/systemd/system/${SERVICE_NAME}.service << EOF
[Unit]
Description=YaoList File Manager
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=${INSTALL_DIR}
ExecStart=${INSTALL_DIR}/${BINARY_NAME}
Restart=on-failure
RestartSec=5
StandardOutput=append:${LOG_FILE}
StandardError=append:${LOG_FILE}

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload
    systemctl enable ${SERVICE_NAME} 2>/dev/null
    success "服务创建完成"
}

create_default_config() {
    if [ ! -f "$CONFIG_FILE" ]; then
        info "创建默认配置文件..."
        mkdir -p "$DATA_DIR"
        cat > "$CONFIG_FILE" << EOF
{
  "server": {
    "host": "0.0.0.0",
    "port": 8180
  },
  "database": {
    "data_dir": "data",
    "db_file": "yaolist.db"
  },
  "search": {
    "db_dir": "search_db",
    "enabled": true
  }
}
EOF
        success "配置文件创建完成"
    fi
}

install_yaolist() {
    echo ""
    info "开始安装 YaoList..."
    
    version=$(get_latest_version)
    if [ -z "$version" ]; then
        error "无法获取最新版本号"
        return 1
    fi
    
    info "最新版本: $version"
    
    if download_binary "$version"; then
        create_default_config
        create_service
        systemctl start ${SERVICE_NAME}
        sleep 3
        
        echo ""
        success "=========================================="
        success "  YaoList 安装完成！"
        success "=========================================="
        echo ""
        ip=$(hostname -I 2>/dev/null | awk '{print $1}')
        info "访问地址: http://${ip:-localhost}:8180"
        echo ""
        admin_pass=$(grep -o 'password: [^ ]*' "${LOG_FILE}" 2>/dev/null | tail -1 | awk '{print $2}')
        if [ -n "$admin_pass" ]; then
            success "管理员账号: admin"
            success "管理员密码: ${admin_pass}"
            echo ""
            warn "请及时修改默认密码！"
        else
            warn "请查看日志获取初始密码: tail ${LOG_FILE}"
        fi
        echo ""
    fi
}

update_yaolist() {
    echo ""
    info "检查更新..."
    
    current=$(get_current_version)
    latest=$(get_latest_version)
    
    if [ -z "$latest" ]; then
        error "无法获取最新版本号"
        return 1
    fi
    
    info "当前版本: $current"
    info "最新版本: $latest"
    
    if [ "$current" = "$latest" ]; then
        success "已是最新版本"
        return 0
    fi
    
    echo ""
    printf "是否更新到 $latest？[y/N] "
    read confirm
    if [ "$confirm" = "y" ] || [ "$confirm" = "Y" ]; then
        systemctl stop ${SERVICE_NAME} 2>/dev/null || true
        if download_binary "$latest"; then
            systemctl start ${SERVICE_NAME}
            success "更新完成！"
        fi
    else
        info "已取消更新"
    fi
}

uninstall_yaolist() {
    echo ""
    warn "即将卸载 YaoList..."
    echo ""
    printf "是否同时删除数据？[y/N] "
    read remove_data
    printf "确认卸载？[y/N] "
    read confirm
    
    if [ "$confirm" != "y" ] && [ "$confirm" != "Y" ]; then
        info "已取消"
        return 0
    fi
    
    systemctl stop ${SERVICE_NAME} 2>/dev/null || true
    systemctl disable ${SERVICE_NAME} 2>/dev/null || true
    rm -f /etc/systemd/system/${SERVICE_NAME}.service
    systemctl daemon-reload
    rm -f "${INSTALL_DIR}/${BINARY_NAME}"
    
    if [ "$remove_data" = "y" ] || [ "$remove_data" = "Y" ]; then
        rm -rf "$INSTALL_DIR"
        success "已完全卸载（包含数据）"
    else
        success "已卸载（保留数据）"
    fi
}

show_status() {
    echo ""
    echo -e "${CYAN}==================== YaoList 状态 ====================${NC}"
    if systemctl is-active --quiet ${SERVICE_NAME} 2>/dev/null; then
        echo -e "服务状态: ${GREEN}运行中${NC}"
    else
        echo -e "服务状态: ${RED}已停止${NC}"
    fi
    echo -e "当前版本: $(get_current_version)"
    echo -e "安装目录: ${INSTALL_DIR}"
    echo -e "日志文件: ${LOG_FILE}"
    echo -e "${CYAN}======================================================${NC}"
}

start_yaolist() {
    info "启动 YaoList..."
    systemctl start ${SERVICE_NAME}
    sleep 2
    if systemctl is-active --quiet ${SERVICE_NAME}; then
        success "启动成功"
    else
        error "启动失败，请查看日志"
    fi
}

stop_yaolist() {
    info "停止 YaoList..."
    systemctl stop ${SERVICE_NAME}
    success "已停止"
}

restart_yaolist() {
    info "重启 YaoList..."
    systemctl restart ${SERVICE_NAME}
    sleep 2
    if systemctl is-active --quiet ${SERVICE_NAME}; then
        success "重启成功"
    else
        error "重启失败"
    fi
}

view_logs() {
    echo ""
    info "实时日志 (Ctrl+C 退出)..."
    echo ""
    tail -f "$LOG_FILE"
}

show_about() {
    echo ""
    echo -e "${CYAN}==================== 关于 YaoList ====================${NC}"
    echo ""
    echo "  YaoList - 现代化高性能文件列表程序"
    echo "  技术栈: Rust + React"
    echo "  许可证: AGPL-3.0"
    echo "  GitHub: https://github.com/ChuYao233/YaoList"
    echo ""
    echo -e "${CYAN}=======================================================${NC}"
}

show_menu() {
    clear
    echo ""
    echo -e "${CYAN}======================================================${NC}"
    echo -e "${CYAN}          欢迎使用 YaoList 管理脚本                   ${NC}"
    echo -e "${CYAN}======================================================${NC}"
    echo ""
    echo -e "${GREEN}基础功能：${NC}"
    echo "  1. 安装 YaoList"
    echo "  2. 更新 YaoList"
    echo "  3. 卸载 YaoList"
    echo -e "${YELLOW}-------------------${NC}"
    echo -e "${GREEN}服务管理：${NC}"
    echo "  4. 查看状态"
    echo "  5. 启动 YaoList"
    echo "  6. 停止 YaoList"
    echo "  7. 重启 YaoList"
    echo "  8. 实时日志"
    echo -e "${YELLOW}-------------------${NC}"
    echo "  9. 关于"
    echo "  0. 退出脚本"
    echo ""
}

main() {
    check_root
    exec < /dev/tty
    
    while true; do
        show_menu
        printf "请输入选择 [0-9]: "
        read choice
        
        case "$choice" in
            1) install_yaolist ;;
            2) update_yaolist ;;
            3) uninstall_yaolist ;;
            4) show_status ;;
            5) start_yaolist ;;
            6) stop_yaolist ;;
            7) restart_yaolist ;;
            8) view_logs ;;
            9) show_about ;;
            0) echo ""; info "再见！"; exit 0 ;;
            *) error "无效选择" ;;
        esac
        
        echo ""
        printf "按回车键继续..."
        read tmp
    done
}

main
