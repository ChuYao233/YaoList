#!/bin/bash
#
# YaoList 一键安装管理脚本
# YaoList One-Click Installation & Management Script
#
# 使用方法 / Usage:
#   curl -fsSL https://raw.githubusercontent.com/chuyao233/yaolist/main/scripts/install.sh | sudo bash
#
# 或下载后执行 / Or download and run:
#   chmod +x install.sh && sudo ./install.sh
#

set -e

# ==================== 配置 ====================
APP_NAME="yaolist"
GITHUB_REPO="chuyao233/yaolist"
INSTALL_DIR="/opt/yaolist"
DATA_DIR="${INSTALL_DIR}/data"
CONFIG_FILE="${INSTALL_DIR}/config.json"
LOG_FILE="${INSTALL_DIR}/yaolist.log"
PID_FILE="${INSTALL_DIR}/yaolist.pid"
BINARY_NAME="yaolist-backend"
SERVICE_NAME="yaolist"

# ==================== 颜色定义 ====================
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
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

get_os() {
    local os=$(uname -s | tr '[:upper:]' '[:lower:]')
    case $os in
        linux) echo "linux" ;;
        darwin) echo "darwin" ;;
        *) error "不支持的操作系统: $os"; exit 1 ;;
    esac
}

check_dependencies() {
    local deps=("curl" "wget" "tar" "gzip")
    for dep in "${deps[@]}"; do
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
    local version=$(curl -fsSL "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
    echo "$version"
}

get_current_version() {
    if [[ -f "${INSTALL_DIR}/${BINARY_NAME}" ]]; then
        local version=$("${INSTALL_DIR}/${BINARY_NAME}" --version 2>/dev/null | head -1 || echo "unknown")
        echo "$version"
    else
        echo "未安装"
    fi
}

download_binary() {
    local version=$1
    local os=$(get_os)
    local arch=$(get_arch)
    local filename="${BINARY_NAME}-${os}-${arch}"
    local url="https://github.com/${GITHUB_REPO}/releases/download/${version}/${filename}"
    
    info "下载 YaoList ${version} (${os}/${arch})..."
    
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
    systemctl enable ${SERVICE_NAME}
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
        error "无法获取最新版本号"
        return 1
    fi
    
    info "最新版本: $version"
    
    if download_binary "$version"; then
        create_default_config
        create_service
        
        systemctl start ${SERVICE_NAME}
        
        echo ""
        success "=========================================="
        success "  YaoList 安装完成！"
        success "=========================================="
        echo ""
        info "访问地址: http://$(hostname -I | awk '{print $1}'):8180"
        info "默认管理员: admin / admin"
        info "请及时修改默认密码！"
        echo ""
    fi
}

update_yaolist() {
    echo ""
    info "检查更新..."
    
    local current=$(get_current_version)
    local latest=$(get_latest_version)
    
    info "当前版本: $current"
    info "最新版本: $latest"
    
    if [[ "$current" == "$latest" ]]; then
        success "已是最新版本"
        return 0
    fi
    
    read -p "是否更新到 $latest？[y/N] " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        stop_yaolist
        
        if download_binary "$latest"; then
            start_yaolist
            success "更新完成！"
        fi
    fi
}

uninstall_yaolist() {
    echo ""
    warn "即将卸载 YaoList..."
    read -p "是否同时删除数据？[y/N] " -n 1 -r
    echo
    local remove_data=$REPLY
    
    read -p "确认卸载？[y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        info "已取消"
        return 0
    fi
    
    stop_yaolist
    
    systemctl disable ${SERVICE_NAME} 2>/dev/null || true
    rm -f /etc/systemd/system/${SERVICE_NAME}.service
    systemctl daemon-reload
    
    rm -f "${INSTALL_DIR}/${BINARY_NAME}"
    
    if [[ $remove_data =~ ^[Yy]$ ]]; then
        rm -rf "$INSTALL_DIR"
        success "已完全卸载（包含数据）"
    else
        success "已卸载（保留数据）"
    fi
}

show_status() {
    echo ""
    echo -e "${CYAN}==================== YaoList 状态 ====================${NC}"
    
    if systemctl is-active --quiet ${SERVICE_NAME}; then
        echo -e "服务状态: ${GREEN}运行中${NC}"
    else
        echo -e "服务状态: ${RED}已停止${NC}"
    fi
    
    echo -e "当前版本: $(get_current_version)"
    echo -e "安装目录: ${INSTALL_DIR}"
    echo -e "配置文件: ${CONFIG_FILE}"
    echo -e "日志文件: ${LOG_FILE}"
    
    if [[ -f "$CONFIG_FILE" ]]; then
        local port=$(grep -o '"port":[[:space:]]*[0-9]*' "$CONFIG_FILE" | grep -o '[0-9]*')
        echo -e "监听端口: ${port:-8180}"
    fi
    
    echo -e "${CYAN}======================================================${NC}"
}

password_manage() {
    echo ""
    echo "密码管理功能："
    echo "1、重置管理员密码"
    echo "2、返回"
    echo ""
    read -p "请选择 [1-2]: " choice
    
    case $choice in
        1)
            read -p "请输入新密码: " -s new_pass
            echo
            if [[ -z "$new_pass" ]]; then
                error "密码不能为空"
                return 1
            fi
            # TODO: 实现密码重置逻辑
            warn "密码重置功能需要后端支持 CLI 参数"
            ;;
        2) return 0 ;;
        *) error "无效选择" ;;
    esac
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
        error "重启失败，请查看日志"
    fi
}

view_logs() {
    echo ""
    info "实时日志 (Ctrl+C 退出)..."
    echo ""
    tail -f "$LOG_FILE"
}

backup_config() {
    echo ""
    local backup_file="/tmp/yaolist_backup_$(date +%Y%m%d_%H%M%S).tar.gz"
    
    info "备份配置到: $backup_file"
    
    tar -czvf "$backup_file" -C "$INSTALL_DIR" config.json data/ 2>/dev/null || true
    
    success "备份完成: $backup_file"
}

restore_config() {
    echo ""
    read -p "请输入备份文件路径: " backup_file
    
    if [[ ! -f "$backup_file" ]]; then
        error "文件不存在: $backup_file"
        return 1
    fi
    
    stop_yaolist
    
    info "恢复配置..."
    tar -xzvf "$backup_file" -C "$INSTALL_DIR"
    
    start_yaolist
    success "恢复完成"
}

docker_manage() {
    echo ""
    echo "Docker 管理："
    echo "1、使用 Docker 安装"
    echo "2、使用 Docker Compose 安装"
    echo "3、返回"
    echo ""
    read -p "请选择 [1-3]: " choice
    
    case $choice in
        1)
            info "Docker 安装命令："
            echo ""
            echo "docker run -d \\"
            echo "  --name yaolist \\"
            echo "  -p 8180:8180 \\"
            echo "  -v /opt/yaolist/data:/app/data \\"
            echo "  chuyao233/yaolist:latest"
            echo ""
            ;;
        2)
            info "Docker Compose 配置："
            echo ""
            cat << 'COMPOSE'
version: '3'
services:
  yaolist:
    image: chuyao233/yaolist:latest
    container_name: yaolist
    ports:
      - "8180:8180"
    volumes:
      - ./data:/app/data
    restart: unless-stopped
COMPOSE
            echo ""
            ;;
        3) return 0 ;;
        *) error "无效选择" ;;
    esac
}

setup_cron_update() {
    echo ""
    echo "定时更新设置："
    echo "1、启用每日自动更新（凌晨3点）"
    echo "2、禁用自动更新"
    echo "3、返回"
    echo ""
    read -p "请选择 [1-3]: " choice
    
    case $choice in
        1)
            local script_path="/usr/local/bin/yaolist-update.sh"
            cat > "$script_path" << 'SCRIPT'
#!/bin/bash
cd /opt/yaolist
latest=$(curl -fsSL "https://api.github.com/repos/chuyao233/yaolist/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
current=$(/opt/yaolist/yaolist-backend --version 2>/dev/null | head -1 || echo "")
if [[ "$current" != "$latest" ]]; then
    systemctl stop yaolist
    curl -fsSL "https://github.com/chuyao233/yaolist/releases/download/${latest}/yaolist-backend-linux-amd64" -o /opt/yaolist/yaolist-backend
    chmod +x /opt/yaolist/yaolist-backend
    systemctl start yaolist
fi
SCRIPT
            chmod +x "$script_path"
            (crontab -l 2>/dev/null | grep -v yaolist-update; echo "0 3 * * * $script_path") | crontab -
            success "已启用每日自动更新"
            ;;
        2)
            crontab -l 2>/dev/null | grep -v yaolist-update | crontab -
            success "已禁用自动更新"
            ;;
        3) return 0 ;;
        *) error "无效选择" ;;
    esac
}

show_system_info() {
    echo ""
    echo -e "${CYAN}==================== 系统信息 ====================${NC}"
    echo -e "操作系统: $(cat /etc/os-release 2>/dev/null | grep PRETTY_NAME | cut -d'"' -f2 || uname -s)"
    echo -e "内核版本: $(uname -r)"
    echo -e "系统架构: $(uname -m)"
    echo -e "CPU 核心: $(nproc)"
    echo -e "内存使用: $(free -h | awk '/Mem:/ {print $3 "/" $2}')"
    echo -e "磁盘使用: $(df -h / | awk 'NR==2 {print $3 "/" $2 " (" $5 ")"}')"
    echo -e "${CYAN}===================================================${NC}"
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
    echo "  GitHub: https://github.com/chuyao233/yaolist"
    echo ""
    echo -e "${CYAN}=======================================================${NC}"
}

# ==================== 主菜单 ====================

show_menu() {
    clear
    echo ""
    echo -e "${CYAN}======================================================${NC}"
    echo -e "${CYAN}          欢迎使用 YaoList 管理脚本                  ${NC}"
    echo -e "${CYAN}======================================================${NC}"
    echo ""
    echo -e "${GREEN}基础功能：${NC}"
    echo "  1、安装 YaoList"
    echo "  2、更新 YaoList"
    echo "  3、卸载 YaoList"
    echo -e "${YELLOW}-------------------${NC}"
    echo -e "${GREEN}服务管理：${NC}"
    echo "  4、查看状态"
    echo "  5、密码管理"
    echo "  6、启动 YaoList"
    echo "  7、停止 YaoList"
    echo "  8、重启 YaoList"
    echo "  9、实时日志"
    echo -e "${YELLOW}-------------------${NC}"
    echo -e "${GREEN}配置管理：${NC}"
    echo "  10、备份配置"
    echo "  11、恢复配置"
    echo -e "${YELLOW}-------------------${NC}"
    echo -e "${GREEN}高级选项：${NC}"
    echo "  12、Docker 管理"
    echo "  13、定时更新"
    echo "  14、系统状态"
    echo "  15、关于"
    echo -e "${YELLOW}-------------------${NC}"
    echo "  0、退出脚本"
    echo ""
}

main() {
    check_root
    
    while true; do
        show_menu
        read -p "请输入选择 [0-15]: " choice
        
        case $choice in
            1) install_yaolist ;;
            2) update_yaolist ;;
            3) uninstall_yaolist ;;
            4) show_status ;;
            5) password_manage ;;
            6) start_yaolist ;;
            7) stop_yaolist ;;
            8) restart_yaolist ;;
            9) view_logs ;;
            10) backup_config ;;
            11) restore_config ;;
            12) docker_manage ;;
            13) setup_cron_update ;;
            14) show_system_info ;;
            15) show_about ;;
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
        read -p "按回车键继续..." -r
    done
}

# 运行主函数
main "$@"
