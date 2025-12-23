# YaoList Backend 架构文档

> 最后更新: 2024-12-22

## 目录结构概览

```
backend/
├── src/                    # 主程序源码
│   ├── api/               # HTTP API 层
│   ├── search/            # 搜索引擎核心
│   ├── server/            # WebDAV 服务
│   ├── storage/           # 存储驱动管理
│   ├── task/              # 任务管理系统
│   └── [单文件模块]
├── drivers/               # 存储驱动实现
└── data/                  # 运行时数据目录
```

---

## 📁 src/ - 主程序源码

### 入口文件

| 文件 | 功能 |
|------|------|
| `main.rs` | 程序入口，初始化服务器、路由、数据库连接 |
| `lib.rs` | 库入口，导出公共模块供外部使用 |

### 核心模块 (单文件)

| 文件 | 功能 |
|------|------|
| `auth.rs` | 会话管理、Session ID 生成、认证工具函数 |
| `config.rs` | 应用配置加载/保存 (config.json) |
| `db.rs` | 数据库初始化、迁移脚本、表结构定义 |
| `download.rs` | 下载限速、并发控制、代理下载 |
| `geoip.rs` | GeoIP 地理位置查询、数据库加载 |
| `load_balance.rs` | 负载均衡策略、驱动选择算法 |
| `models.rs` | 数据模型定义 (User, Mount, Meta 等) |
| `state.rs` | 应用状态管理 (AppState)、全局共享资源 |
| `utils.rs` | 路径处理、文件名冲突解决、隐藏文件检查 |

---

## 📁 src/api/ - HTTP API 层

### 模块概览

| 目录/文件 | 功能 |
|-----------|------|
| `mod.rs` | API 模块声明、公共响应结构 |
| `auth/` | 用户认证相关 API |
| `files/` | 文件操作相关 API |
| `extract/` | 在线解压缩 API |
| `shares/` | 文件分享 API |
| `settings/` | 系统设置 API |
| `search/` | 搜索功能 API |

### api/auth/ - 认证模块

| 文件 | 功能 |
|------|------|
| `mod.rs` | 模块声明和重导出 |
| `types.rs` | 请求/响应结构体定义 |
| `login.rs` | 登录、验证码、登出 |
| `register.rs` | 用户注册、用户名唯一性检查 |
| `password.rs` | 密码修改、密码重置 |
| `profile.rs` | 用户资料查看/更新 |
| `two_factor.rs` | 双因素认证 (2FA/TOTP) |

### api/files/ - 文件操作模块

| 文件 | 功能 |
|------|------|
| `mod.rs` | 模块声明、虚拟文件处理 |
| `common.rs` | 公共函数、权限检查、用户上下文 |
| `list.rs` | 文件/目录列表、排序、分页 |
| `operations.rs` | 创建目录、删除、重命名 |
| `upload.rs` | 文件上传、分片上传、秒传 |
| `download.rs` | 文件下载、直链生成、代理下载 |
| `copy_move.rs` | 文件/目录复制、移动 (支持跨驱动) |

### api/extract/ - 解压缩模块

| 文件 | 功能 |
|------|------|
| `mod.rs` | 模块声明 |
| `types.rs` | 请求/响应结构体、压缩格式枚举 |
| `utils.rs` | 格式判断、大小格式化、编码解码 |
| `extractors.rs` | ZIP/TAR/7Z 解压实现 |
| `handlers.rs` | API 处理函数、解压任务管理 |

### api/shares/ - 分享模块

| 文件 | 功能 |
|------|------|
| `mod.rs` | 模块声明 |
| `types.rs` | Share 结构体、请求类型 |
| `admin.rs` | 管理员API: 创建/编辑/删除分享 |
| `public.rs` | 公开API: 访问分享、验证密码、下载 |

### api/settings/ - 设置模块

| 文件 | 功能 |
|------|------|
| `mod.rs` | 模块声明 |
| `types.rs` | 设置请求结构体 |
| `site.rs` | 站点设置 (标题、公告、注册开关等) |
| `geoip.rs` | GeoIP 数据库管理、下载、配置 |

### api/search/ - 搜索 API 模块

| 文件 | 功能 |
|------|------|
| `mod.rs` | 模块声明 |
| `types.rs` | 搜索请求/响应结构体 |
| `admin.rs` | 索引管理 (构建/停止/状态) |
| `query.rs` | 搜索查询、结果过滤、分页 |

### api/ 其他单文件

| 文件 | 功能 |
|------|------|
| `archive.rs` | 压缩包内容预览 (不解压) |
| `backup.rs` | 系统备份/恢复 |
| `direct_links.rs` | 直链管理、签名验证 |
| `drivers.rs` | 存储驱动管理 API |
| `file_resolver.rs` | 路径解析、挂载点匹配、驱动选择 |
| `groups.rs` | 用户组管理 |
| `load_balance.rs` | 负载均衡配置 API |
| `meta.rs` | 元信息管理 (密码、隐藏规则等) |
| `mounts.rs` | 挂载点管理 |
| `notification.rs` | 消息通知 (WebSocket) |
| `server.rs` | 服务器状态、健康检查 |
| `tasks.rs` | 任务列表 API |
| `users.rs` | 用户管理 (管理员) |
| `webdav.rs` | WebDAV 请求转发 |

---

## 📁 src/search/ - 搜索引擎核心

| 文件 | 功能 |
|------|------|
| `mod.rs` | 模块声明、公共接口 |
| `db_index.rs` | SQLite 全文搜索索引 |
| `engine.rs` | 搜索引擎核心逻辑 |
| `file_index.rs` | 文件索引构建 |
| `schema.rs` | 索引结构定义 |
| `tokenizer.rs` | 中文/英文分词器 |

---

## 📁 src/server/ - WebDAV 服务

| 文件 | 功能 |
|------|------|
| `mod.rs` | 模块声明 |
| `config.rs` | WebDAV 配置、用户认证 |
| `webdav.rs` | WebDAV 协议实现 (dav-server) |

---

## 📁 src/storage/ - 存储驱动管理

| 文件 | 功能 |
|------|------|
| `mod.rs` | StorageDriver trait 定义、Entry 结构 |
| `manager.rs` | 驱动管理器、驱动注册/创建/获取 |
| `local_factory.rs` | 本地驱动工厂 |

---

## 📁 src/task/ - 任务管理系统

| 文件 | 功能 |
|------|------|
| `mod.rs` | 模块声明 |
| `types.rs` | TaskType, TaskStatus, TaskEvent 枚举 |
| `models.rs` | Task, TaskControl, TaskSummary 结构 |
| `manager.rs` | 任务管理器 (创建/暂停/取消/进度更新) |

---

## 📁 drivers/ - 存储驱动实现

| 目录 | 功能 |
|------|------|
| `mod.rs` | 驱动注册入口 |
| `local/` | 本地文件系统驱动 |
| `ftp/` | FTP/SFTP 驱动 |
| `onedrive/` | OneDrive 驱动 |
| `cloud189/` | 天翼云盘驱动 |
| `quark/` | 夸克网盘驱动 |

---

## 🔗 模块依赖关系

```
main.rs
    ├── api/          ← HTTP 请求处理
    │   ├── files/    ← 调用 storage/ 和 task/
    │   ├── search/   ← 调用 src/search/
    │   └── ...
    ├── storage/      ← 抽象存储接口
    │   └── drivers/  ← 具体驱动实现
    ├── task/         ← 异步任务管理
    ├── search/       ← 全文搜索引擎
    ├── server/       ← WebDAV 服务
    └── state.rs      ← 全局共享状态
```

---

## 📝 设计原则

1. **模块化**: 每个功能独立模块，职责单一
2. **分层架构**: API → 业务逻辑 → 存储驱动
3. **驱动抽象**: 所有存储通过 `StorageDriver` trait 统一接口
4. **异步优先**: 使用 Tokio 异步运行时
5. **类型安全**: 充分利用 Rust 类型系统

---

## 🚀 快速开始

```bash
# 开发运行
cargo run

# 生产构建
cargo build --release

# 运行测试
cargo test
```

配置文件: `config.json` (首次运行自动创建)
