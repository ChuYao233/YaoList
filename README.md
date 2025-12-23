<div align="center">
  <h1>🗂️ YaoList</h1>
  <p><em>A modern, high-performance file list program built with Rust + React</em></p>
  <p><em>一个现代化、高性能的文件列表程序，使用 Rust + React 构建</em></p>

  <img src="https://img.shields.io/badge/rust-1.70+-orange.svg" alt="Rust" />
  <img src="https://img.shields.io/badge/react-18+-blue.svg" alt="React" />
  <img src="https://img.shields.io/badge/license-AGPL--3.0-green.svg" alt="License" />
</div>

---

- English | [中文](./README_cn.md) | [日本語](./README_ja.md)

## ✨ Features

### 📁 Multiple Storage Support

- [x] **Local Storage** - Local file system
- [x] **[OneDrive](https://www.microsoft.com/en-us/microsoft-365/onedrive/online-cloud-storage)** - Microsoft OneDrive (Personal & Business)
- [x] **[189 Cloud](https://cloud.189.cn)** - China Telecom Cloud (Personal & Family)
- [x] **[123pan](https://www.123pan.com)** - 123 Cloud Drive (Open API)
- [x] **[Quark](https://pan.quark.cn)** - Quark Cloud Drive
- [x] **[Lanzou](https://www.lanzou.com)** - Lanzou Cloud
- [x] **[FTP](https://en.wikipedia.org/wiki/File_Transfer_Protocol)** - FTP Protocol
- [x] **[WebDAV](https://en.wikipedia.org/wiki/WebDAV)** - WebDAV Protocol
- [x] **[SMB/CIFS](https://en.wikipedia.org/wiki/Server_Message_Block)** - Windows Network Share (Native Support)
- [x] **[S3](https://aws.amazon.com/s3)** - Amazon S3 & Compatible Services (MinIO, Cloudflare R2, etc.)

### 🎯 Core Features

- [x] **High Performance & Low Memory** - Rust backend with async I/O, low memory footprint, handles thousands of concurrent connections
- [x] **Modern UI** - Clean React frontend with TailwindCSS, supports dark mode
- [x] **Custom Themes** - Customizable page backgrounds and glassmorphism styles
- [x] **File Preview** - PDF, Markdown, code, images, video, audio with subtitle/lyrics support
- [x] **Image Preview** - Supports HEIC and almost all RAW formats
- [x] **Encrypted Audio** - Supports NCM and other encrypted audio formats (manual enable required)
- [x] **Office Preview** - DOCX, PPTX, XLSX local parsing, no public domain required, no Microsoft/Google online services
- [x] **Archive Support** - Browse ZIP, 7Z, TAR, GZ archives without extraction
- [x] **Full-text Search** - Built-in search engine with Chinese word segmentation (Jieba), lightweight index database
- [x] **WebDAV Server** - Access your files via WebDAV protocol
- [x] **Direct Links** - Generate permanent direct download links with access count limits
- [x] **Sharing** - Share files/folders with password protection, expiration and access count limits

### 🔐 Security & Management

- [x] **User System** - Multi-user support with group-based permissions
- [x] **Self-Registration** - Users can self-register via phone/email
- [x] **Two-Factor Auth** - TOTP-based 2FA support
- [x] **Group Management** - Organize users into groups with different permissions
- [x] **Path Protection** - Password protect specific paths
- [x] **Hide Rules** - Hide files/folders based on patterns
- [x] **Login Security** - Captcha on login retry, rate limiting, IP blocking
- [x] **Usage Statistics** - Track each user's traffic and access count

### ⚡ Advanced Features

- [x] **Task Manager** - Clean background task queue for copy/move operations
- [x] **Load Balancing** - Multi-node load balancing with GeoIP routing
- [x] **Notification** - Email & SMS notifications
- [x] **Backup/Restore** - Export and import configuration
- [x] **Thumbnail Generation** - Auto-generate image thumbnails
- [x] **Streaming** - Range request support for video streaming
- [ ] **Scheduled Tasks** - Coming soon
- [ ] **File Collection** - File collection form feature, coming soon

## 🚀 Quick Start

### One-Click Installation (Recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/chuyao233/yaolist/main/scripts/install.sh | sudo bash
```

### Binary Release

```bash
# Download the latest release
wget https://github.com/chuyao233/yaolist/releases/latest/download/yaolist-linux-amd64

# Make it executable
chmod +x yaolist-linux-amd64

# Run
./yaolist-linux-amd64
```

### Build from Source

```bash
# Clone the repository
git clone https://github.com/chuyao233/yaolist.git
cd yaolist

# Build (requires Rust 1.70+)
cargo build --release

# Run
./target/release/yaolist-backend
```

### Docker

```bash
docker run -d \
  --name yaolist \
  -p 8180:8180 \
  -v /path/to/data:/app/data \
  chuyao233/yaolist:latest
```

## ⚙️ Configuration

Configuration file: `config.json`

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

## 📖 Documentation

- [Driver Development Guide](./drivers/DRIVER_DEVELOPMENT.md)
- [API Documentation](./docs/API.md) (Coming soon)

## 🛠️ Tech Stack

### Backend
- **Language**: Rust
- **Framework**: Axum
- **Database**: SQLite (SQLx)
- **Async Runtime**: Tokio

### Frontend
- **Framework**: React 18
- **UI Library**: TailwindCSS + shadcn/ui
- **State Management**: React Query
- **Icons**: Lucide React

## 📝 License

This project is open-source software licensed under the [AGPL-3.0](https://www.gnu.org/licenses/agpl-3.0.txt) license.

## 📚 Documentation

> ⚠️ **Documentation is still under construction.** If you're interested in helping, contributions are very welcome!

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

**We especially need help with:**
- 📖 Writing documentation
- 🌐 Translating to other languages
- 🐛 Bug reports and fixes

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit your changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

## 📧 Contact

- GitHub: [@chuyao233](https://github.com/chuyao233)

## 🙏 Acknowledgments

- Some code logic in this project is referenced from [OpenList](https://github.com/OpenListTeam/OpenList)
