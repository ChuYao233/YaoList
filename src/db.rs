use sqlx::SqlitePool;
use anyhow::Result;
use uuid::Uuid;
use chrono::Utc;
use rand::Rng;

/// Generate random password / 生成随机密码
fn generate_random_password(length: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789!@#$%^&*";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Run database migrations / 运行数据库迁移
pub async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            unique_id TEXT NOT NULL UNIQUE,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            email TEXT,
            phone TEXT,
            qq TEXT,
            root_path TEXT DEFAULT '/',
            is_admin INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 1,
            two_factor_enabled INTEGER NOT NULL DEFAULT 0,
            two_factor_secret TEXT,
            total_traffic INTEGER NOT NULL DEFAULT 0,
            total_requests INTEGER NOT NULL DEFAULT 0,
            last_login TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_groups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            description TEXT,
            is_admin INTEGER NOT NULL DEFAULT 0,
            allow_direct_link INTEGER NOT NULL DEFAULT 0,
            allow_share INTEGER NOT NULL DEFAULT 0,
            show_hidden_files INTEGER NOT NULL DEFAULT 0,
            no_password_access INTEGER NOT NULL DEFAULT 0,
            add_offline_download INTEGER NOT NULL DEFAULT 0,
            create_upload INTEGER NOT NULL DEFAULT 0,
            rename_files INTEGER NOT NULL DEFAULT 0,
            move_files INTEGER NOT NULL DEFAULT 0,
            copy_files INTEGER NOT NULL DEFAULT 0,
            delete_files INTEGER NOT NULL DEFAULT 0,
            read_files INTEGER NOT NULL DEFAULT 1,
            read_compressed INTEGER NOT NULL DEFAULT 0,
            extract_files INTEGER NOT NULL DEFAULT 0,
            webdav_enabled INTEGER NOT NULL DEFAULT 0,
            ftp_enabled INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS permissions (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            resource TEXT NOT NULL,
            action TEXT NOT NULL,
            description TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_group_members (
            user_id TEXT NOT NULL,
            group_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (user_id, group_id),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
            FOREIGN KEY (group_id) REFERENCES user_groups(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS group_permissions (
            group_id TEXT NOT NULL,
            permission_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (group_id, permission_id),
            FOREIGN KEY (group_id) REFERENCES user_groups(id) ON DELETE CASCADE,
            FOREIGN KEY (permission_id) REFERENCES permissions(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS mounts (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            driver TEXT NOT NULL,
            mount_path TEXT NOT NULL UNIQUE,
            config TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS drivers (
            name TEXT PRIMARY KEY,
            version TEXT NOT NULL,
            description TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 0,
            config TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS site_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS direct_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id TEXT,
            sign TEXT NOT NULL UNIQUE,
            path TEXT NOT NULL,
            filename TEXT NOT NULL,
            expires_at TEXT,
            max_access_count INTEGER,
            access_count INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS shares (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id TEXT,
            short_id TEXT NOT NULL UNIQUE,
            path TEXT NOT NULL,
            name TEXT NOT NULL,
            is_dir INTEGER NOT NULL DEFAULT 0,
            password TEXT,
            expires_at TEXT,
            max_access_count INTEGER,
            access_count INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            task_type TEXT NOT NULL,
            status TEXT NOT NULL,
            name TEXT NOT NULL,
            source_path TEXT NOT NULL,
            target_path TEXT,
            total_size INTEGER NOT NULL,
            processed_size INTEGER NOT NULL DEFAULT 0,
            total_files INTEGER NOT NULL DEFAULT 1,
            processed_files INTEGER NOT NULL DEFAULT 0,
            progress REAL NOT NULL DEFAULT 0.0,
            speed REAL NOT NULL DEFAULT 0.0,
            eta_seconds INTEGER,
            created_at TEXT NOT NULL,
            started_at TEXT,
            finished_at TEXT,
            error TEXT,
            user_id TEXT,
            current_file TEXT,
            files TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;
    
    // 添加新字段（如果不存在）
    let _ = sqlx::query("ALTER TABLE tasks ADD COLUMN current_file TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE tasks ADD COLUMN files TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE tasks ADD COLUMN items TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE tasks ADD COLUMN conflict_strategy TEXT").execute(pool).await;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS metas (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL,
            password TEXT,
            p_sub INTEGER NOT NULL DEFAULT 0,
            write INTEGER NOT NULL DEFAULT 0,
            w_sub INTEGER NOT NULL DEFAULT 0,
            hide TEXT,
            h_sub INTEGER NOT NULL DEFAULT 0,
            readme TEXT,
            r_sub INTEGER NOT NULL DEFAULT 0,
            header TEXT,
            header_sub INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS search_settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            enabled INTEGER NOT NULL DEFAULT 0,
            auto_update_index INTEGER NOT NULL DEFAULT 1,
            ignore_paths TEXT NOT NULL DEFAULT '',
            max_index_depth INTEGER NOT NULL DEFAULT 20,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS search_index (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            is_dir INTEGER NOT NULL DEFAULT 0,
            size INTEGER NOT NULL DEFAULT 0,
            modified_at TEXT,
            mount_path TEXT NOT NULL,
            indexed_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_search_index_name ON search_index(name)"
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_search_index_path ON search_index(path)"
    )
    .execute(pool)
    .await?;

    // 迁移：添加 extract_files 列（如果不存在）
    // SQLite 不支持 IF NOT EXISTS 语法，需要先检查列是否存在
    let has_extract_files: bool = sqlx::query_scalar::<_, i32>(
        "SELECT COUNT(*) FROM pragma_table_info('user_groups') WHERE name = 'extract_files'"
    )
    .fetch_one(pool)
    .await
    .map(|count| count > 0)
    .unwrap_or(false);
    
    if !has_extract_files {
        tracing::info!("Migration: Adding extract_files column to user_groups");
        sqlx::query("ALTER TABLE user_groups ADD COLUMN extract_files INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }

    // 迁移：添加 root_path 列到 user_groups 表（用户组根路径限制）
    let has_group_root_path: bool = sqlx::query_scalar::<_, i32>(
        "SELECT COUNT(*) FROM pragma_table_info('user_groups') WHERE name = 'root_path'"
    )
    .fetch_one(pool)
    .await
    .map(|count| count > 0)
    .unwrap_or(false);
    
    if !has_group_root_path {
        tracing::info!("Migration: Adding root_path column to user_groups");
        sqlx::query("ALTER TABLE user_groups ADD COLUMN root_path TEXT DEFAULT '/'")
            .execute(pool)
            .await?;
    }

    // 迁移：添加负载均衡配置字段到 drivers 表
    // load_balance_mode: none(默认)/round_robin(轮询)/ip_hash(IP分流)/weighted(按权重)
    // load_balance_weight: 权重值(1-100)
    // load_balance_group: 负载均衡组名（同组的驱动参与负载均衡）
    let has_lb_mode: bool = sqlx::query_scalar::<_, i32>(
        "SELECT COUNT(*) FROM pragma_table_info('drivers') WHERE name = 'load_balance_mode'"
    )
    .fetch_one(pool)
    .await
    .map(|count| count > 0)
    .unwrap_or(false);
    
    if !has_lb_mode {
        tracing::info!("Migration: Adding load balance fields to drivers");
        sqlx::query("ALTER TABLE drivers ADD COLUMN load_balance_mode TEXT DEFAULT 'none'")
            .execute(pool)
            .await?;
        sqlx::query("ALTER TABLE drivers ADD COLUMN load_balance_weight INTEGER DEFAULT 1")
            .execute(pool)
            .await?;
        sqlx::query("ALTER TABLE drivers ADD COLUMN load_balance_group TEXT")
            .execute(pool)
            .await?;
    }

    // 创建通知配置表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS notification_config (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL,
            name TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 0,
            config TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 创建验证码表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS verification_codes (
            id TEXT PRIMARY KEY,
            user_id TEXT,
            target TEXT NOT NULL,
            code TEXT NOT NULL,
            type TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            used INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 创建负载均衡组表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS load_balance_groups (
            name TEXT PRIMARY KEY,
            config TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    tracing::info!("Database migration completed");
    
    initialize_default_data(pool).await?;
    
    Ok(())
}

/// Initialize default data / 初始化默认数据
async fn initialize_default_data(pool: &SqlitePool) -> Result<()> {
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    
    if user_count == 0 {
        tracing::info!("First startup, initializing default data...");
        
        let admin_id = Uuid::new_v4().to_string();
        let admin_unique_id = "1".to_string();
        let now = Utc::now().to_rfc3339();
        
        let admin_password = generate_random_password(16);
        let password_hash = bcrypt::hash(&admin_password, bcrypt::DEFAULT_COST)?;
        
        sqlx::query(
            "INSERT INTO users (id, unique_id, username, password_hash, email, is_admin, enabled, created_at, updated_at) 
             VALUES (?, ?, ?, ?, ?, 1, 1, ?, ?)"
        )
        .bind(&admin_id)
        .bind(&admin_unique_id)
        .bind("admin")
        .bind(&password_hash)
        .bind("admin@ylist.org")
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
        
        // 创建管理员组，让数据库自动生成id，拥有所有权限
        let admin_group_result = sqlx::query(
            "INSERT INTO user_groups (
                name, description, is_admin, 
                allow_direct_link, allow_share, show_hidden_files, no_password_access,
                add_offline_download, create_upload, rename_files, move_files,
                copy_files, delete_files, read_files, read_compressed, extract_files,
                webdav_enabled, ftp_enabled, created_at, updated_at
            ) VALUES (?, ?, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, ?, ?)"
        )
        .bind("管理员组")
        .bind("系统管理员组，拥有所有权限")
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
        
        let admin_group_id = admin_group_result.last_insert_rowid();
        
        sqlx::query(
            "INSERT INTO user_group_members (user_id, group_id, created_at) 
             VALUES (?, ?, ?)"
        )
        .bind(&admin_id)
        .bind(admin_group_id.to_string())
        .bind(&now)
        .execute(pool)
        .await?;
        
        let guest_id = Uuid::new_v4().to_string();
        let guest_unique_id = "2".to_string();
        let guest_password_hash = bcrypt::hash("", bcrypt::DEFAULT_COST)?;
        
        sqlx::query(
            "INSERT INTO users (id, unique_id, username, password_hash, is_admin, enabled, created_at, updated_at) 
             VALUES (?, ?, ?, ?, 0, 0, ?, ?)"
        )
        .bind(&guest_id)
        .bind(&guest_unique_id)
        .bind("guest")
        .bind(&guest_password_hash)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
        
        // 创建游客组，让数据库自动生成id，默认有 read_files 权限
        let guest_group_result = sqlx::query(
            "INSERT INTO user_groups (name, description, read_files, created_at, updated_at) 
             VALUES (?, ?, 1, ?, ?)"
        )
        .bind("游客组")
        .bind("游客用户组")
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
        
        let guest_group_id = guest_group_result.last_insert_rowid();
        
        sqlx::query(
            "INSERT INTO user_group_members (user_id, group_id, created_at) 
             VALUES (?, ?, ?)"
        )
        .bind(&guest_id)
        .bind(guest_group_id.to_string())
        .bind(&now)
        .execute(pool)
        .await?;
        
        let permissions = vec![
            ("mount.read", "mount", "read", "查看挂载点"),
            ("mount.write", "mount", "write", "创建和修改挂载点"),
            ("mount.delete", "mount", "delete", "删除挂载点"),
            ("driver.read", "driver", "read", "查看驱动"),
            ("driver.manage", "driver", "manage", "管理驱动"),
            ("file.read", "file", "read", "读取文件"),
            ("file.write", "file", "write", "上传和修改文件"),
            ("file.delete", "file", "delete", "删除文件"),
            ("user.read", "user", "read", "查看用户"),
            ("user.manage", "user", "manage", "管理用户"),
            ("group.read", "group", "read", "查看用户组"),
            ("group.manage", "group", "manage", "管理用户组"),
        ];
        
        // 初始化站点设置
        let site_settings = vec![
            ("site_title", "YaoList"),
            ("site_description", "简洁优雅的文件列表程序"),
        ];
        
        for (key, value) in site_settings {
            sqlx::query(
                "INSERT OR IGNORE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
            )
            .bind(key)
            .bind(value)
            .bind(&now)
            .execute(pool)
            .await?;
        }
        
        for (name, resource, action, desc) in permissions {
            let perm_id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO permissions (id, name, resource, action, description) 
                 VALUES (?, ?, ?, ?, ?)"
            )
            .bind(&perm_id)
            .bind(name)
            .bind(resource)
            .bind(action)
            .bind(desc)
            .execute(pool)
            .await?;
            
            sqlx::query(
                "INSERT INTO group_permissions (group_id, permission_id, created_at) 
                 VALUES (?, ?, ?)"
            )
            .bind(admin_group_id)
            .bind(&perm_id)
            .bind(&now)
            .execute(pool)
            .await?;
        }
        
        tracing::info!("============================================================");
        tracing::info!("Default admin account created:");
        tracing::info!("  Email: admin@ylist.org");
        tracing::info!("  Username: admin");
        tracing::info!("  Password: {}", admin_password);
        tracing::info!("  Group: Administrator");
        tracing::info!("WARNING: Please save the password and change it after login!");
        tracing::info!("============================================================");
    }
    
    // 数据库迁移：添加 two_factor_secret 字段（如果不存在）
    let columns: Vec<(i32, String, String, i32, Option<String>, i32)> = sqlx::query_as("PRAGMA table_info(users)")
        .fetch_all(pool)
        .await?;
    
    let has_two_factor_secret = columns.iter().any(|(_, name, _, _, _, _)| name == "two_factor_secret");
    if !has_two_factor_secret {
        sqlx::query("ALTER TABLE users ADD COLUMN two_factor_secret TEXT")
            .execute(pool)
            .await?;
        tracing::info!("Migration: Added two_factor_secret column");
    }
    
    Ok(())
}
