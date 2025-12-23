#!/bin/bash
#
# YaoList 一键安装管理脚本
# YaoList One-Click Installation & Management Script
#
# 使用方法 / Usage:
#   curl -fsSL https://raw.githubusercontent.com/ChuYao233/YaoList/main/scripts/install.sh | sudo bash
#

# ==================== 配置 ====================
APP_NAME="yaolist"
GITHUB_REPO="ChuYao233/YaoList"
INSTALL_DIR="/opt/yaolist"
DATA_DIR="${INSTALL_DIR}/data"
CONFIG_FILE="${INSTALL_DIR}/config.json"
LOG_FILE="${INSTALL_DIR}/yaolist.log"
BINARY_NAME="yaolist-backend"
SERVICE_NAME="yaolist"

# ==================== 颜色定义 ====================
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# ==================== 工具函数 ====================
info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; }
success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }

check_root() {
    if [[ $EUID -ne 0 ]]; then
        error "请使用 root 用户运行此脚本"
        error "Please run this script as root"
        exit 1
    fi
}

get_arch() {
    local arch=$(uname -m)
    case $arch in
        x86_64|amd64) echo "amd64" ;;
        aarch64|arm64) echo "arm64" ;;
        armv7l) echo "armv7" ;;
        *) error "不支持的架构: $arch"; exit 1 ;;
    esac
}

check_dependencies() {
    for dep in curl; do
        if ! command -v $dep &> /dev/null; then
            warn "正在安装依赖: $dep"
            if command -v apt-get &> /dev/null; then
                apt-get update && apt-get install -y $dep
            elif command -v yum &> /dev/null; then
                yum install -y $dep
            elif command -v dnf &> /dev/null; then
                dnf install -y $dep
            fi
        fi
    done
}

get_latest_version() {
    curl -fsSL "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" 2>/dev/null | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/' || echo ""
}

get_current_version() {
    if [[ -f "${INSTALL_DIR}/${BINARY_NAME}" ]]; then
        "${INSTALL_DIR}/${BINARY_NAME}" --version 2>/dev/null | head -1 || echo "unknown"
    else
        echo "未安装"
    fi
}

download_binary() {
    local version=$1
    local arch=$(get_arch)
    local filename="${BINARY_NAME}-linux-${arch}"
    local url="https://github.com/${GITHUB_REPO}/releases/download/${version}/${filename}"
    
    info "下载 YaoList ${version} (linux/${arch})..."
    
    mkdir -p "$INSTALL_DIR"
    
    if curl -fsSL "$url" -o "${INSTALL_DIR}/${BINARY_NAME}.new"; then
        chmod +x "${INSTALL_DIR}/${BINARY_NAME}.new"
        mv "${INSTALL_DIR}/${BINARY_NAME}.new" "${INSTALL_DIR}/${BINARY_NAME}"
        success "下载完成"
        return 0
    else
        error "下载失败，请检查网络连接"
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
    if [[ ! -f "$CONFIG_FILE" ]]; then
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

# ==================== 功能函数 ====================

install_yaolist() {
    echo ""
    info "开始安装 YaoList..."
    
    check_dependencies
    
    local version=$(get_latest_version)
    if [[ -z "$version" ]]; then
        error "无法获取最新版本号，请检查网络"
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
        local ip=$(hostname -I 2>/dev/null | awk '{print $1}' || echo "localhost")
        info "访问地址: http://${ip}:8180"
        echo ""
        # 从日志中提取初始密码
        local admin_pass=$(grep -o 'password: [^ ]*' "${LOG_FILE}" 2>/dev/null | tail -1 | awk '{print $2}')
        if [[ -n "$admin_pass" ]]; then
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
    
    local current=$(get_current_version)
    local latest=$(get_latest_version)
    
    if [[ -z "$latest" ]]; then
        error "无法获取最新版本号"
        return 1
    fi
    
    info "当前版本: $current"
    info "最新版本: $latest"
    
    if [[ "$current" == "$latest" ]]; then
        success "已是最新版本"
        return 0
    fi
    
    echo ""
    read -p "是否更新到 $latest？[y/N] " confirm
    if [[ "$confirm" =~ ^[Yy]$ ]]; then
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
    read -p "是否同时删除数据？[y/N] " remove_data
    read -p "确认卸载？[y/N] " confirm
    
    if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
        info "已取消"
        return 0
    fi
    
    systemctl stop ${SERVICE_NAME} 2>/dev/null || true
    systemctl disable ${SERVICE_NAME} 2>/dev/null || true
    rm -f /etc/systemd/system/${SERVICE_NAME}.service
    systemctl daemon-reload
    
    rm -f "${INSTALL_DIR}/${BINARY_NAME}"
    
    if [[ "$remove_data" =~ ^[Yy]$ ]]; then
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
    echo -e "配置文件: ${CONFIG_FILE}"
    echo -e "日志文件: ${LOG_FILE}"
    
    if [[ -f "$CONFIG_FILE" ]]; then
        local port=$(grep -o '"port":[[:space:]]*[0-9]*' "$CONFIG_FILE" 2>/dev/null | grep -o '[0-9]*' || echo "8180")
        echo -e "监听端口: ${port}"
    fi
    
    echo -e "${CYAN}======================================================${NC}"
}

start_yaolist() {
    info "启动 YaoList..."
    systemctl start ${SERVICE_NAME}
    sleep 2
    if systemctl is-active --quiet ${SERVICE_NAME}; then
        success "启动成功"
    else
        error "启动失败，请查看日志: tail -f ${LOG_FILE}"
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
        error "重启失败，请查看日志: tail -f ${LOG_FILE}"
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
    echo "  A modern, high-performance file list program"
    echo ""
    echo "  技术栈: Rust + React"
    echo "  许可证: AGPL-3.0"
    echo ""
    echo "  GitHub: https://github.com/ChuYao233/YaoList"
    echo ""
    echo -e "${CYAN}=======================================================${NC}"
}

# ==================== 主菜单 ====================

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
    
    # 确保从终端读取输入
    exec < /dev/tty
    
    while true; do
        show_menu
        read -p "请输入选择 [0-9]: " choice
        
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
            0) 
                echo ""
                info "感谢使用，再见！"
                exit 0
                ;;
            *)
                error "无效选择，请重新输入"
                ;;
        esac
        
        echo ""
        read -p "按回车键继续..."
    done
}

# 运行主函数
main
