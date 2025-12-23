use chrono::Utc;

fn main() {
    // 设置构建时间
    let build_time = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    println!("cargo:rustc-env=BUILD_TIME={}", build_time);
    
    // 当build.rs改变时重新运行
    println!("cargo:rerun-if-changed=build.rs");
}
