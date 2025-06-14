use super::{QuarkDriver, QuarkConfig};
use crate::drivers::Driver;

pub fn register_quark() -> Box<dyn Driver> {
    Box::new(QuarkDriver::new(QuarkConfig {
        cookie: String::new(),
        root_folder_id: "0".to_string(),
        order_by: "none".to_string(),
        order_direction: "asc".to_string(),
    }))
}

pub fn register_uc() -> Box<dyn Driver> {
    let mut driver = QuarkDriver::new(QuarkConfig {
        cookie: String::new(),
        root_folder_id: "0".to_string(),
        order_by: "none".to_string(),
        order_direction: "asc".to_string(),
    });

    // 修改为 UC 网盘的配置
    driver.api_base = "https://pc-api.uc.cn/1/clouddrive".to_string();
    driver.referer = "https://drive.uc.cn".to_string();
    driver.ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) uc-cloud-drive/2.5.20 Chrome/100.0.4896.160 Electron/18.3.5.4-b478491100 Safari/537.36 Channel/pckk_other_ch".to_string();
    driver.pr = "UCBrowser".to_string();

    Box::new(driver)
} 