<div align="center">
  <h1>🗂️ YaoList</h1>
  <p><em>Rust + React で構築されたモダンで高性能なファイルリストプログラム</em></p>

  <img src="https://img.shields.io/badge/rust-1.70+-orange.svg" alt="Rust" />
  <img src="https://img.shields.io/badge/react-18+-blue.svg" alt="React" />
  <img src="https://img.shields.io/badge/license-AGPL--3.0-green.svg" alt="License" />
</div>

---

- [English](./README.md) | [中文](./README_cn.md) | 日本語

## ✨ 機能

### 📁 マルチストレージ対応

- [x] **ローカルストレージ** - ローカルファイルシステム
- [x] **[OneDrive](https://www.microsoft.com/en-us/microsoft-365/onedrive/online-cloud-storage)** - Microsoft OneDrive（個人用・ビジネス用）
- [x] **[Aliyundrive](https://www.alipan.com)** - アリババクラウドドライブ
- [x] **[189 Cloud](https://cloud.189.cn)** - 中国電信クラウド（個人用・家族用）
- [x] **[123pan](https://www.123pan.com)** - 123クラウドドライブ（Open API）
- [x] **[Quark](https://pan.quark.cn)** - Quarkクラウドドライブ
- [x] **[Lanzou](https://www.lanzou.com)** - 蓝奏クラウド
- [x] **[FTP](https://en.wikipedia.org/wiki/File_Transfer_Protocol)** - FTPプロトコル
- [x] **[WebDAV](https://en.wikipedia.org/wiki/WebDAV)** - WebDAVプロトコル
- [x] **[SMB/CIFS](https://en.wikipedia.org/wiki/Server_Message_Block)** - Windows ネットワーク共有（ネイティブサポート）
- [x] **[S3](https://aws.amazon.com/s3)** - Amazon S3 および互換サービス（MinIO、Cloudflare R2など）
- [x] **[PikPak](https://mypikpak.com)** - PikPakクラウドドライブ
- [x] **[Yun139](https://yun.139.com)** - 中国移動クラウド（個人用・家族用）
- [ ] **[115 Cloud](https://115.com)** - 115クラウドドライブ（開発中）

### 🎯 コア機能

- [x] **高性能・低メモリ** - 非同期I/OによるRustバックエンド、低メモリ消費、数千の同時接続を処理
- [x] **モダンUI** - TailwindCSSを使用したクリーンなReactフロントエンド、ダークモード対応
- [x] **カスタムテーマ** - ページ背景とすりガラス効果のカスタマイズ
- [x] **ファイルプレビュー** - PDF、Markdown、コード、画像、動画、音声（字幕/歌詞対応）
- [x] **画像プレビュー** - HEICおよびほぼすべてのRAW形式に対応
- [x] **暗号化音声** - NCMなどの暗号化音声形式に対応（手動で有効化が必要）
- [x] **Officeプレビュー** - DOCX、PPTX、XLSXローカル解析、公開ドメイン不要、Microsoft/Googleオンラインサービス不要
- [x] **アーカイブ対応** - ZIP、7Z、TAR、GZアーカイブを解凍せずに閲覧
- [x] **全文検索** - 中国語分かち書き（Jieba）対応の内蔵検索エンジン、軽量インデックスDB
- [x] **WebDAVサーバー** - WebDAVプロトコルでファイルにアクセス
- [x] **直接リンク** - アクセス回数制限付きの永久直接ダウンロードリンクを生成
- [x] **共有** - パスワード保護、有効期限、アクセス回数制限付きのファイル/フォルダ共有

### 🔐 セキュリティと管理

- [x] **ユーザーシステム** - グループベースの権限を持つマルチユーザー対応
- [x] **セルフ登録** - 電話/メールによるユーザー自己登録
- [x] **二要素認証** - TOTPベースの2FA対応
- [x] **グループ管理** - 異なる権限を持つグループにユーザーを整理
- [x] **パス保護** - 特定のパスにパスワード保護を設定
- [x] **非表示ルール** - パターンに基づいてファイル/フォルダを非表示
- [x] **ログインセキュリティ** - ログイン再試行時のCAPTCHA、レート制限、IPブロック
- [x] **使用統計** - 各ユーザーのトラフィックとアクセス回数を追跡

### ⚡ 高度な機能

- [x] **タスクマネージャー** - コピー/移動操作用のシンプルなバックグラウンドタスクキュー
- [x] **ロードバランシング** - GeoIPルーティング付きマルチノードロードバランシング
- [x] **通知** - メールおよびSMS通知
- [x] **バックアップ/復元** - 設定のエクスポートとインポート
- [x] **ストリーミング** - 動画ストリーミング用のRangeリクエスト対応
- [ ] **スケジュールタスク** - 開発予定
- [ ] **ファイル収集** - ファイル収集フォーム機能、開発予定

## 🚀 クイックスタート

### ワンクリックインストール（推奨）

```bash
curl -fsSL https://raw.githubusercontent.com/chuyao233/yaolist/main/scripts/install.sh | sudo bash
```

### バイナリリリース

```bash
# 最新版をダウンロード
wget https://github.com/chuyao233/yaolist/releases/latest/download/yaolist-linux-amd64

# 実行権限を付与
chmod +x yaolist-linux-amd64

# 実行
./yaolist-linux-amd64
```

### ソースからビルド

```bash
# リポジトリをクローン
git clone https://github.com/chuyao233/yaolist.git
cd yaolist

# ビルド（Rust 1.70+が必要）
cargo build --release

# 実行
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

## ⚙️ 設定

設定ファイル：`config.json`

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

## 📖 ドキュメント

- [ドライバー開発ガイド](./drivers/DRIVER_DEVELOPMENT.md)
- [APIドキュメント](./docs/API.md)（近日公開）

## 🛠️ 技術スタック

### バックエンド
- **言語**: Rust
- **フレームワーク**: Axum
- **データベース**: SQLite (SQLx)
- **非同期ランタイム**: Tokio

### フロントエンド
- **フレームワーク**: React 18
- **UIライブラリ**: TailwindCSS + shadcn/ui
- **状態管理**: React Query
- **アイコン**: Lucide React

## 📝 ライセンス

このプロジェクトは [AGPL-3.0](https://www.gnu.org/licenses/agpl-3.0.txt) ライセンスの下でオープンソースソフトウェアとして公開されています。

## 📚 ドキュメント

> ⚠️ **ドキュメントはまだ作成中です。** ご協力いただける方を歓迎します！

## 🤝 貢献

貢献を歓迎します！お気軽にPull Requestを提出してください。

**特に以下の分野でのご協力をお待ちしています：**
- 📖 ドキュメントの作成
- 🌐 他言語への翻訳
- 🐛 バグ報告と修正

1. リポジトリをFork
2. 機能ブランチを作成 (`git checkout -b feature/AmazingFeature`)
3. 変更をコミット (`git commit -m 'Add some AmazingFeature'`)
4. ブランチにプッシュ (`git push origin feature/AmazingFeature`)
5. Pull Requestを開く

## 📧 連絡先

- GitHub: [@chuyao233](https://github.com/chuyao233)

## 🙏 謝辞

- 本プロジェクトの一部のコードロジックは [OpenList](https://github.com/OpenListTeam/OpenList) を参考にしています
