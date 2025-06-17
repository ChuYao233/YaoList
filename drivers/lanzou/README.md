# 蓝奏云驱动

基于蓝奏云API的存储驱动，支持完整的文件操作功能。

## 功能特性

### ✅ 已实现功能
- **文件列表** - 列出文件夹中的文件和子文件夹
- **文件下载** - 获取下载链接并流式下载文件（支持Range请求）
- **文件上传** - 上传文件到指定文件夹
- **文件删除** - 删除文件或文件夹（自动判断类型）
- **文件重命名** - 重命名文件（仅支持文件，不支持文件夹）
- **创建文件夹** - 在指定目录创建新文件夹
- **移动文件** - 将文件移动到其他文件夹（仅支持文件）

### ❌ 不支持功能
- **复制文件** - 蓝奏云API不支持直接复制
- **文件夹重命名** - 蓝奏云API不支持文件夹重命名
- **文件夹移动** - 蓝奏云API不支持文件夹移动

## 认证方式

### 1. Cookie认证（推荐）
```json
{
  "type": "cookie",
  "cookie": "你的蓝奏云Cookie"
}
```

### 2. 账号密码认证
```json
{
  "type": "account", 
  "account": "你的蓝奏云账号",
  "password": "你的蓝奋云密码"
}
```

### 3. 分享链接访问（只读）
```json
{
  "type": "url",
  "share_password": "分享密码（如果有）"
}
```

## 配置参数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `type` | string | `"cookie"` | 认证类型：account/cookie/url |
| `account` | string | - | 蓝奏云账号（account认证时必填） |
| `password` | string | - | 蓝奏云密码（account认证时必填） |
| `cookie` | string | - | 蓝奏云Cookie（cookie认证时必填） |
| `root_folder_id` | string | `"-1"` | 根目录文件夹ID，-1表示根目录 |
| `share_password` | string | - | 分享链接的提取密码 |
| `baseUrl` | string | `"https://pc.woozooo.com"` | 文件操作的基础URL |
| `shareUrl` | string | `"https://pan.lanzoui.com"` | 分享页面的URL |
| `user_agent` | string | 默认UA | HTTP请求的User Agent |
| `repair_file_info` | boolean | `false` | 修复文件信息（获取准确的文件大小和时间） |

## API映射

| 操作 | 蓝奏云API Task | 说明 |
|------|----------------|------|
| 获取文件夹 | task=47 | 获取指定文件夹下的子文件夹 |
| 获取文件 | task=5 | 获取指定文件夹下的文件列表 |
| 上传文件 | task=1 | 通过html5up.php上传文件 |
| 删除文件 | task=6 | 删除指定文件 |
| 删除文件夹 | task=3 | 删除指定文件夹 |
| 重命名文件 | task=46 | 重命名指定文件 |
| 创建文件夹 | task=2 | 在指定位置创建文件夹 |
| 移动文件 | task=20 | 移动文件到其他文件夹 |
| 获取文件分享 | task=22 | 获取文件的分享信息 |

## 注意事项

1. **风控检测** - 蓝奏云有反爬虫措施，可能出现acw_sc__v2验证
2. **Cookie有效期** - Cookie大约15天有效期，过期需要重新获取
3. **文件限制** - 蓝奏云对文件类型和大小有限制
4. **路径处理** - 当前实现中路径简化为ID，实际使用时需要路径到ID的映射
5. **分页处理** - 文件列表会自动处理分页，获取所有文件

## 错误处理

- **zt=1/2** - 操作成功
- **zt=4** - 需要等待重试
- **zt=9** - 登录过期，需要重新认证
- **其他zt值** - 根据inf/info字段显示错误信息

## 示例配置

```json
{
  "type": "cookie",
  "cookie": "your_lanzou_cookie_here",
  "root_folder_id": "-1",
  "baseUrl": "https://pc.woozooo.com",
  "shareUrl": "https://pan.lanzoui.com",
  "repair_file_info": true
}
``` 