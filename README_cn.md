<div align="center">
  <h1>🗂️ YaoList</h1>
  <p><em>一个现代化、高性能的文件列表程序，使用 Rust + React 构建</em></p>

  <img src="https://img.shields.io/badge/rust-1.70+-orange.svg" alt="Rust" />
  <img src="https://img.shields.io/badge/react-18+-blue.svg" alt="React" />
  <img src="https://img.shields.io/badge/license-AGPL--3.0-green.svg" alt="License" />
</div>

---

- [English](./README.md) | 中文 | [日本語](./README_ja.md)

## ✨ 功能特性

### 📁 多存储支持

- [x] **本地存储** - 本地文件系统
- [x] **[OneDrive](https://www.microsoft.com/en-us/microsoft-365/onedrive/online-cloud-storage)** - 微软 OneDrive（个人版和商业版）
- [x] **[天翼云盘](https://cloud.189.cn)** - 中国电信云盘（个人版和家庭版）
- [x] **[123云盘](https://www.123pan.com)** - 123云盘（开放API）
- [x] **[夸克网盘](https://pan.quark.cn)** - 夸克网盘
- [x] **[蓝奏云](https://www.lanzou.com)** - 蓝奏云
- [x] **[FTP](https://en.wikipedia.org/wiki/File_Transfer_Protocol)** - FTP 协议
- [x] **[WebDAV](https://en.wikipedia.org/wiki/WebDAV)** - WebDAV 协议
- [x] **[SMB/CIFS](https://en.wikipedia.org/wiki/Server_Message_Block)** - Windows 网络共享（原生支持）
- [x] **[S3](https://aws.amazon.com/s3)** - Amazon S3 及兼容服务（MinIO、Cloudflare R2 等）

### 🎯 核心功能

- [x] **高性能** - Rust 后端搭配异步 I/O，可处理数千并发连接
- [x] **现代化界面** - 简洁的 React 前端配合 TailwindCSS，支持深色模式
- [x] **文件预览** - PDF、Markdown、代码、图片、视频、音频（支持字幕/歌词）
- [x] **Office 预览** - DOCX、PPTX、XLSX 文档预览
- [x] **压缩包支持** - 无需解压即可浏览 ZIP、7Z、TAR、GZ 压缩包
- [x] **全文搜索** - 内置搜索引擎，支持中文分词（结巴分词）
- [x] **WebDAV 服务器** - 通过 WebDAV 协议访问您的文件
- [x] **直链下载** - 生成永久直链下载地址
- [x] **文件分享** - 支持密码保护和过期时间的文件/文件夹分享

### 🔐 安全与管理

- [x] **用户系统** - 多用户支持，基于角色的权限管理
- [x] **双因素认证** - 基于 TOTP 的两步验证
- [x] **用户组管理** - 将用户组织到不同权限的用户组
- [x] **路径保护** - 为特定路径设置密码保护
- [x] **隐藏规则** - 基于模式隐藏文件/文件夹
- [x] **登录安全** - 验证码、速率限制、IP 封禁

### ⚡ 高级功能

- [x] **任务管理器** - 后台任务队列，用于复制/移动操作
- [x] **负载均衡** - 多节点负载均衡，支持 GeoIP 路由
- [x] **通知系统** - 邮件和短信通知
- [x] **备份/恢复** - 导出和导入配置
- [x] **缩略图生成** - 自动生成图片缩略图
- [x] **流媒体** - Range 请求支持视频流播放

## 🚀 快速开始

### 一键安装（推荐）

```bash
curl -fsSL https://raw.githubusercontent.com/chuyao233/yaolist/main/scripts/install.sh | sudo bash
```

### 下载二进制文件

```bash
# 下载最新版本
wget https://github.com/chuyao233/yaolist/releases/latest/download/yaolist-linux-amd64

# 添加执行权限
chmod +x yaolist-linux-amd64

# 运行
./yaolist-linux-amd64
```

### 从源码构建

```bash
# 克隆仓库
git clone https://github.com/chuyao233/yaolist.git
cd yaolist

# 构建（需要 Rust 1.70+）
cargo build --release

# 运行
./target/release/yaolist-backend
```

### Docker 部署

```bash
docker run -d \
  --name yaolist \
  -p 8180:8180 \
  -v /path/to/data:/app/data \
  chuyao233/yaolist:latest
```

## ⚙️ 配置文件

配置文件：`config.json`

```json
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
```

## 📖 文档

- [驱动开发指南](./drivers/DRIVER_DEVELOPMENT.md)
- [API 文档](./docs/API.md)（即将推出）

## 🛠️ 技术栈

### 后端
- **语言**: Rust
- **框架**: Axum
- **数据库**: SQLite (SQLx)
- **异步运行时**: Tokio

### 前端
- **框架**: React 18
- **UI 库**: TailwindCSS + shadcn/ui
- **状态管理**: React Query
- **图标**: Lucide React

## 📝 许可证

本项目是根据 [AGPL-3.0](https://www.gnu.org/licenses/agpl-3.0.txt) 许可证开源的软件。

## 🤝 贡献

欢迎贡献！请随时提交 Pull Request。

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 打开 Pull Request

## 📧 联系方式

- GitHub: [@chuyao233](https://github.com/chuyao233)

## 🙏 致谢

- 灵感来自 [AList](https://github.com/alist-org/alist) 和 [OpenList](https://github.com/OpenListTeam/OpenList)
- 感谢所有贡献者
