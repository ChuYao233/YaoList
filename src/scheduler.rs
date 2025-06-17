use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_cron_scheduler::{Job, JobScheduler};
use std::collections::HashMap;
use uuid::Uuid;
use chrono::Utc;
use tracing::{info, error};
use std::str::FromStr;
use futures::future::BoxFuture;

use crate::drivers::Driver;

// 将简单调度参数转换为cron表达式
fn build_cron_expression(schedule_type: &str, schedule_time: &str, schedule_day: Option<i32>) -> Result<String> {
    // 解析时间 "HH:MM"
    let parts: Vec<&str> = schedule_time.split(':').collect();
    if parts.len() != 2 {
        return Err(anyhow!("时间格式错误，应为 HH:MM"));
    }
    
    let hour: u32 = parts[0].parse().map_err(|_| anyhow!("小时格式错误"))?;
    let minute: u32 = parts[1].parse().map_err(|_| anyhow!("分钟格式错误"))?;
    
    if hour > 23 || minute > 59 {
        return Err(anyhow!("时间范围错误"));
    }
    
    let cron = match schedule_type {
        "daily" => {
            // 每天指定时间执行：秒 分 时 * * *
            format!("0 {} {} * * *", minute, hour)
        },
        "weekly" => {
            // 每周指定星期几的指定时间执行：秒 分 时 * * 星期
            let day = schedule_day.ok_or_else(|| anyhow!("周调度需要指定星期几"))?;
            if day < 0 || day > 6 {
                return Err(anyhow!("星期几范围错误（0-6）"));
            }
            format!("0 {} {} * * {}", minute, hour, day)
        },
        "monthly" => {
            // 每月指定日期的指定时间执行：秒 分 时 日 * *
            let day = schedule_day.ok_or_else(|| anyhow!("月调度需要指定日期"))?;
            if day < 1 || day > 31 {
                return Err(anyhow!("月份日期范围错误（1-31）"));
            }
            format!("0 {} {} {} * *", minute, hour, day)
        },
        _ => {
            return Err(anyhow!("不支持的调度类型"));
        }
    };
    
    Ok(cron)
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub cron_expression: String, // 内部使用，由前端参数生成
    pub schedule_type: Option<String>, // 前端显示用
    pub schedule_time: Option<String>, // 前端显示用
    pub schedule_day: Option<i32>, // 前端显示用
    pub task_type: String, // "copy", "move", "sync"
    pub source_path: String,
    pub destination_path: String,
    pub enabled: bool,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_run: Option<String>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub run_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub name: String,
    pub description: Option<String>,
    pub schedule_type: String, // "daily", "weekly", "monthly", "custom"
    pub schedule_time: String, // "HH:MM" format for time
    pub schedule_day: Option<i32>, // day of week (0-6) for weekly, day of month (1-31) for monthly
    pub task_type: String,
    pub source_path: String,
    pub destination_path: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTaskRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub schedule_type: Option<String>,
    pub schedule_time: Option<String>,
    pub schedule_day: Option<i32>,
    pub task_type: Option<String>,
    pub source_path: Option<String>,
    pub destination_path: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TaskExecution {
    pub id: String,
    pub task_id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String, // "running", "success", "failed"
    pub files_processed: i64,
    pub bytes_transferred: i64,
    pub error_message: Option<String>,
}

pub struct TaskScheduler {
    scheduler: JobScheduler,
    db_pool: SqlitePool,
    tasks: Arc<RwLock<HashMap<String, ScheduledTask>>>,
}

impl TaskScheduler {
    pub async fn new(db_pool: SqlitePool) -> Result<Self> {
        let scheduler = JobScheduler::new().await?;
        
        Ok(Self {
            scheduler,
            db_pool,
            tasks: Arc::new(RwLock::new(HashMap::new())),
        })
    }
    
    pub async fn start(&self) -> Result<()> {
        self.scheduler.start().await?;
        info!("📅 定时任务调度器已启动");
        
        // 加载数据库中的所有任务
        self.load_tasks_from_db().await?;
        
        Ok(())
    }
    
    pub async fn stop(&mut self) -> Result<()> {
        self.scheduler.shutdown().await?;
        info!("📅 定时任务调度器已停止");
        Ok(())
    }
    
    async fn load_tasks_from_db(&self) -> Result<()> {
        let tasks: Vec<ScheduledTask> = sqlx::query_as(
            "SELECT * FROM scheduled_tasks WHERE enabled = true"
        )
        .fetch_all(&self.db_pool)
        .await?;
        
        let mut loaded_count = 0;
        for task in tasks {
            if let Err(e) = self.schedule_task(&task).await {
                error!("加载定时任务失败 {}: {}", task.name, e);
            } else {
                loaded_count += 1;
            }
        }
        
        info!("📅 已加载 {} 个定时任务", loaded_count);
        Ok(())
    }
    
    async fn schedule_task(&self, task: &ScheduledTask) -> Result<()> {
        let task_id = task.id.clone();
        let task_clone = task.clone();
        let db_pool = self.db_pool.clone();
        
        let job = Job::new_async(task.cron_expression.as_str(), move |_uuid, _scheduler| {
            let task = task_clone.clone();
            let pool = db_pool.clone();
            
            Box::pin(async move {
                info!("🔄 执行定时任务: {}", task.name);
                
                // 创建任务执行记录
                let execution_id = Uuid::new_v4().to_string();
                let execution = TaskExecution {
                    id: execution_id.clone(),
                    task_id: task.id.clone(),
                    started_at: Utc::now().to_rfc3339(),
                    finished_at: None,
                    status: "running".to_string(),
                    files_processed: 0,
                    bytes_transferred: 0,
                    error_message: None,
                };
                
                // 保存执行记录
                if let Err(e) = save_task_execution(&pool, &execution).await {
                    error!("保存任务执行记录失败: {}", e);
                    return;
                }
                
                // 执行任务
                let result = execute_copy_task(&task).await;
                
                // 更新执行记录
                let mut final_execution = execution;
                final_execution.finished_at = Some(Utc::now().to_rfc3339());
                
                match result {
                    Ok((files_count, bytes_count)) => {
                        final_execution.status = "success".to_string();
                        final_execution.files_processed = files_count;
                        final_execution.bytes_transferred = bytes_count;
                        info!("✅ 定时任务 {} 执行成功，处理了 {} 个文件，传输了 {} 字节", 
                              task.name, files_count, bytes_count);
                    }
                    Err(e) => {
                        final_execution.status = "failed".to_string();
                        final_execution.error_message = Some(e.to_string());
                        error!("❌ 定时任务 {} 执行失败: {}", task.name, e);
                    }
                }
                
                // 更新任务状态
                if let Err(e) = update_task_last_run(&pool, &task.id, &final_execution).await {
                    error!("更新任务状态失败: {}", e);
                }
                
                // 保存最终执行记录
                if let Err(e) = update_task_execution(&pool, &final_execution).await {
                    error!("更新任务执行记录失败: {}", e);
                }
            })
        })?;
        
        self.scheduler.add(job).await?;
        
        // 缓存任务
        let mut tasks = self.tasks.write().await;
        tasks.insert(task_id, task.clone());
        
        Ok(())
    }
    
    pub async fn create_task(&self, request: CreateTaskRequest, created_by: String) -> Result<ScheduledTask> {
        // 生成cron表达式
        let cron_expression = build_cron_expression(
            &request.schedule_type,
            &request.schedule_time,
            request.schedule_day
        )?;
        
        info!("生成的cron表达式: {}", cron_expression);
        
        // 验证生成的cron表达式
        if let Err(e) = cron::Schedule::from_str(&cron_expression) {
            error!("cron表达式验证失败: {} -> {}", cron_expression, e);
            return Err(anyhow!("生成的cron表达式无效: {}", e));
        }
        
        let task = ScheduledTask {
            id: Uuid::new_v4().to_string(),
            name: request.name,
            description: request.description,
            cron_expression,
            schedule_type: Some(request.schedule_type),
            schedule_time: Some(request.schedule_time),
            schedule_day: request.schedule_day,
            task_type: request.task_type,
            source_path: request.source_path,
            destination_path: request.destination_path,
            enabled: request.enabled,
            created_by,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            last_run: None,
            last_status: None,
            last_error: None,
            run_count: 0,
        };
        
        // 保存到数据库
        sqlx::query(
            "INSERT INTO scheduled_tasks (id, name, description, cron_expression, schedule_type, schedule_time, schedule_day, task_type, source_path, destination_path, enabled, created_by, created_at, updated_at, run_count) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&task.id)
        .bind(&task.name)
        .bind(&task.description)
        .bind(&task.cron_expression)
        .bind(&task.schedule_type)
        .bind(&task.schedule_time)
        .bind(task.schedule_day)
        .bind(&task.task_type)
        .bind(&task.source_path)
        .bind(&task.destination_path)
        .bind(task.enabled)
        .bind(&task.created_by)
        .bind(&task.created_at)
        .bind(&task.updated_at)
        .bind(task.run_count)
        .execute(&self.db_pool)
        .await?;
        
        // 如果启用，则添加到调度器
        if task.enabled {
            self.schedule_task(&task).await?;
        }
        
        Ok(task)
    }
    
    pub async fn update_task(&self, task_id: String, request: UpdateTaskRequest) -> Result<ScheduledTask> {
        // 获取现有任务
        let existing_task: Option<ScheduledTask> = sqlx::query_as(
            "SELECT * FROM scheduled_tasks WHERE id = ?"
        )
        .bind(&task_id)
        .fetch_optional(&self.db_pool)
        .await?;
        
        let mut task = existing_task.ok_or_else(|| anyhow!("任务不存在"))?;
        
        // 更新字段
        if let Some(name) = request.name {
            task.name = name;
        }
        if let Some(description) = request.description {
            task.description = Some(description);
        }
        
        // 检查是否有调度相关的更新
        let schedule_updated = request.schedule_type.is_some() || 
                              request.schedule_time.is_some() || 
                              request.schedule_day.is_some();
        
        if schedule_updated {
            let schedule_type = request.schedule_type.unwrap_or(task.schedule_type.clone().unwrap_or("daily".to_string()));
            let schedule_time = request.schedule_time.unwrap_or(task.schedule_time.clone().unwrap_or("00:00".to_string()));
            let schedule_day = request.schedule_day.or(task.schedule_day);
            
            // 生成新的cron表达式
            let new_cron = build_cron_expression(&schedule_type, &schedule_time, schedule_day)?;
            
            // 验证cron表达式
            if let Err(_) = cron::Schedule::from_str(&new_cron) {
                return Err(anyhow!("生成的cron表达式无效"));
            }
            
            task.cron_expression = new_cron;
            task.schedule_type = Some(schedule_type);
            task.schedule_time = Some(schedule_time);
            task.schedule_day = schedule_day;
        }
        
        if let Some(task_type) = request.task_type {
            task.task_type = task_type;
        }
        if let Some(source_path) = request.source_path {
            task.source_path = source_path;
        }
        if let Some(destination_path) = request.destination_path {
            task.destination_path = destination_path;
        }
        if let Some(enabled) = request.enabled {
            task.enabled = enabled;
        }
        
        task.updated_at = Utc::now().to_rfc3339();
        
        // 更新数据库
        sqlx::query(
            "UPDATE scheduled_tasks SET name = ?, description = ?, cron_expression = ?, schedule_type = ?, schedule_time = ?, schedule_day = ?, task_type = ?, source_path = ?, destination_path = ?, enabled = ?, updated_at = ? WHERE id = ?"
        )
        .bind(&task.name)
        .bind(&task.description)
        .bind(&task.cron_expression)
        .bind(&task.schedule_type)
        .bind(&task.schedule_time)
        .bind(task.schedule_day)
        .bind(&task.task_type)
        .bind(&task.source_path)
        .bind(&task.destination_path)
        .bind(task.enabled)
        .bind(&task.updated_at)
        .bind(&task_id)
        .execute(&self.db_pool)
        .await?;
        
        // 重新调度任务
        self.reschedule_task(&task).await?;
        
        Ok(task)
    }
    
    pub async fn delete_task(&self, task_id: String) -> Result<()> {
        // 从调度器中移除
        self.unschedule_task(&task_id).await?;
        
        // 从数据库中删除
        sqlx::query("DELETE FROM scheduled_tasks WHERE id = ?")
            .bind(&task_id)
            .execute(&self.db_pool)
            .await?;
        
        // 删除执行记录
        sqlx::query("DELETE FROM task_executions WHERE task_id = ?")
            .bind(&task_id)
            .execute(&self.db_pool)
            .await?;
        
        Ok(())
    }
    
    async fn reschedule_task(&self, task: &ScheduledTask) -> Result<()> {
        // 先移除旧任务
        self.unschedule_task(&task.id).await?;
        
        // 如果启用，重新添加
        if task.enabled {
            self.schedule_task(task).await?;
        }
        
        Ok(())
    }
    
    async fn unschedule_task(&self, task_id: &str) -> Result<()> {
        // 从缓存中移除
        let mut tasks = self.tasks.write().await;
        tasks.remove(task_id);
        
        // 注意：tokio-cron-scheduler 没有直接的移除任务API
        // 在实际实现中可能需要重启调度器或使用其他方法
        
        Ok(())
    }
    
    pub async fn list_tasks(&self, created_by: Option<String>) -> Result<Vec<ScheduledTask>> {
        let tasks = if let Some(user) = created_by {
            sqlx::query_as(
                "SELECT * FROM scheduled_tasks WHERE created_by = ? ORDER BY created_at DESC"
            )
            .bind(user)
            .fetch_all(&self.db_pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT * FROM scheduled_tasks ORDER BY created_at DESC"
            )
            .fetch_all(&self.db_pool)
            .await?
        };
        
        Ok(tasks)
    }
    
    pub async fn get_task_executions(&self, task_id: String, limit: Option<i64>) -> Result<Vec<TaskExecution>> {
        let limit = limit.unwrap_or(20);
        
        let executions: Vec<TaskExecution> = sqlx::query_as(
            "SELECT * FROM task_executions WHERE task_id = ? ORDER BY started_at DESC LIMIT ?"
        )
        .bind(task_id)
        .bind(limit)
        .fetch_all(&self.db_pool)
        .await?;
        
        Ok(executions)
    }
    
    pub async fn run_task_now(&self, task_id: String) -> Result<()> {
        let task: Option<ScheduledTask> = sqlx::query_as(
            "SELECT * FROM scheduled_tasks WHERE id = ?"
        )
        .bind(&task_id)
        .fetch_optional(&self.db_pool)
        .await?;
        
        let task = task.ok_or_else(|| anyhow!("任务不存在"))?;
        
        // 异步执行任务
        let pool = self.db_pool.clone();
        tokio::spawn(async move {
            let execution_id = Uuid::new_v4().to_string();
            let execution = TaskExecution {
                id: execution_id.clone(),
                task_id: task.id.clone(),
                started_at: Utc::now().to_rfc3339(),
                finished_at: None,
                status: "running".to_string(),
                files_processed: 0,
                bytes_transferred: 0,
                error_message: None,
            };
            
            if let Err(e) = save_task_execution(&pool, &execution).await {
                error!("保存任务执行记录失败: {}", e);
                return;
            }
            
            let result = execute_copy_task(&task).await;
            
            let mut final_execution = execution;
            final_execution.finished_at = Some(Utc::now().to_rfc3339());
            
            match result {
                Ok((files_count, bytes_count)) => {
                    final_execution.status = "success".to_string();
                    final_execution.files_processed = files_count;
                    final_execution.bytes_transferred = bytes_count;
                    info!("✅ 手动执行任务 {} 成功", task.name);
                }
                Err(e) => {
                    final_execution.status = "failed".to_string();
                    final_execution.error_message = Some(e.to_string());
                    error!("❌ 手动执行任务 {} 失败: {}", task.name, e);
                }
            }
            
            if let Err(e) = update_task_last_run(&pool, &task.id, &final_execution).await {
                error!("更新任务状态失败: {}", e);
            }
            
            if let Err(e) = update_task_execution(&pool, &final_execution).await {
                error!("更新任务执行记录失败: {}", e);
            }
        });
        
        Ok(())
    }
}

// 辅助函数
async fn execute_copy_task(task: &ScheduledTask) -> Result<(i64, i64)> {
    use crate::{find_storage_for_path, create_driver_from_storage};
    
    // 获取源和目标存储
    let src_storage = find_storage_for_path(&task.source_path).await
        .ok_or_else(|| anyhow!("未找到源存储"))?;
    let dst_storage = find_storage_for_path(&task.destination_path).await
        .ok_or_else(|| anyhow!("未找到目标存储"))?;
    
    let src_driver = create_driver_from_storage(&src_storage)
        .ok_or_else(|| anyhow!("无法创建源存储驱动"))?;
    let dst_driver = create_driver_from_storage(&dst_storage)
        .ok_or_else(|| anyhow!("无法创建目标存储驱动"))?;
    
    // 获取相对路径
    let src_rel = get_relative_path(&task.source_path, &src_storage.mount_path);
    let dst_rel = get_relative_path(&task.destination_path, &dst_storage.mount_path);
    
    // 执行复制操作
    let (files_count, bytes_count) = copy_recursively_with_stats(
        &*src_driver, 
        &src_rel, 
        &*dst_driver, 
        &dst_rel
    ).await?;
    
    Ok((files_count, bytes_count))
}

// 带统计的递归同步函数（同步模式：目标目录将与源目录完全一致）
fn copy_recursively_with_stats<'a>(
    src_driver: &'a dyn Driver,
    src_path: &'a str,
    dst_driver: &'a dyn Driver,
    dst_path: &'a str,
) -> BoxFuture<'a, Result<(i64, i64)>> {
    Box::pin(async move {
    let mut files_count = 0i64;
    let mut bytes_count = 0i64;
    
    let src_info = src_driver.get_file_info(src_path).await?;
    
    if !src_info.is_dir {
        // 同步文件
        let mut should_copy = true;
        
        // 检查目标文件是否存在且大小相同
        if let Ok(dst_info) = dst_driver.get_file_info(dst_path).await {
            if !dst_info.is_dir && dst_info.size == src_info.size {
                // 文件存在且大小相同，跳过复制
                should_copy = false;
                println!("📋 跳过已存在的文件: {} (大小: {} 字节)", dst_path, src_info.size);
            }
        }
        
        if should_copy {
            // 复制文件，优先使用流式下载
            let mut buf = Vec::new();
            
            // 先尝试流式下载
            if let Ok(Some((mut stream, _))) = src_driver.stream_download(src_path).await {
                use futures::StreamExt;
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => buf.extend_from_slice(&bytes),
                        Err(e) => return Err(anyhow::anyhow!("流式下载失败: {}", e)),
                    }
                }
            } else {
                // 流式下载失败，使用标准下载
                use tokio::io::AsyncReadExt;
                let mut file = src_driver.download(src_path).await?;
                file.read_to_end(&mut buf).await?;
            }
            
            let filename = std::path::Path::new(dst_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("copy.dat");
            let parent_dir = std::path::Path::new(dst_path)
                .parent()
                .map(|p| p.to_string_lossy())
                .unwrap_or("/".into());
            let parent_dir = if parent_dir.is_empty() { "/" } else { parent_dir.as_ref() };
            
            dst_driver.upload_file(parent_dir, filename, &buf).await?;
            
            files_count += 1;
            bytes_count += buf.len() as i64;
            println!("✅ 已复制文件: {} ({} 字节)", dst_path, buf.len());
        }
    } else {
        // 同步目录
        
        // 确保目标目录存在
        dst_driver
            .create_folder(
                if dst_path == "/" {
                    "/"
                } else {
                    std::path::Path::new(dst_path)
                        .parent()
                        .and_then(|p| p.to_str())
                        .unwrap_or("/")
                },
                std::path::Path::new(dst_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(src_path),
            )
            .await
            .ok();
        
        // 获取源目录和目标目录的文件列表
        let src_children = src_driver.list(src_path).await?;
        let dst_children = dst_driver.list(dst_path).await.unwrap_or_else(|_| Vec::new());
        
        // 创建源文件映射
        let mut src_map = std::collections::HashMap::new();
        for child in &src_children {
            src_map.insert(child.name.clone(), child);
        }
        
        // 删除目标目录中源目录没有的文件
        for dst_child in &dst_children {
            if !src_map.contains_key(&dst_child.name) {
                let dst_child_path = format!("{}/{}", dst_path.trim_end_matches('/'), dst_child.name);
                match dst_driver.delete(&dst_child_path).await {
                    Ok(_) => println!("🗑️ 已删除多余文件: {}", dst_child_path),
                    Err(e) => println!("⚠️ 删除文件失败: {} - {}", dst_child_path, e),
                }
            }
        }
        
        // 同步源目录中的所有文件
        for src_child in src_children {
            let child_src = format!("{}/{}", src_path.trim_end_matches('/'), src_child.name);
            let child_dst = format!("{}/{}", dst_path.trim_end_matches('/'), src_child.name);
            let (child_files, child_bytes) = copy_recursively_with_stats(
                src_driver, 
                &child_src, 
                dst_driver, 
                &child_dst
            ).await?;
            files_count += child_files;
            bytes_count += child_bytes;
        }
    }
    
    Ok((files_count, bytes_count))
    })
}

async fn save_task_execution(pool: &SqlitePool, execution: &TaskExecution) -> Result<()> {
    sqlx::query(
        "INSERT INTO task_executions (id, task_id, started_at, status, files_processed, bytes_transferred) VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(&execution.id)
    .bind(&execution.task_id)
    .bind(&execution.started_at)
    .bind(&execution.status)
    .bind(execution.files_processed)
    .bind(execution.bytes_transferred)
    .execute(pool)
    .await?;
    
    Ok(())
}

async fn update_task_execution(pool: &SqlitePool, execution: &TaskExecution) -> Result<()> {
    sqlx::query(
        "UPDATE task_executions SET finished_at = ?, status = ?, files_processed = ?, bytes_transferred = ?, error_message = ? WHERE id = ?"
    )
    .bind(&execution.finished_at)
    .bind(&execution.status)
    .bind(execution.files_processed)
    .bind(execution.bytes_transferred)
    .bind(&execution.error_message)
    .bind(&execution.id)
    .execute(pool)
    .await?;
    
    Ok(())
}

async fn update_task_last_run(pool: &SqlitePool, task_id: &str, execution: &TaskExecution) -> Result<()> {
    sqlx::query(
        "UPDATE scheduled_tasks SET last_run = ?, last_status = ?, last_error = ?, run_count = run_count + 1 WHERE id = ?"
    )
    .bind(&execution.finished_at)
    .bind(&execution.status)
    .bind(&execution.error_message)
    .bind(task_id)
    .execute(pool)
    .await?;
    
    Ok(())
}

fn get_relative_path(path: &str, mount_path: &str) -> String {
    if mount_path == "/" {
        path.trim_start_matches('/').to_string()
    } else {
        path.strip_prefix(mount_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    }
} 