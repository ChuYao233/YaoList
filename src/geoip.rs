//! GeoIP工具模块 - 通用IP地理位置查询
//! 
//! 使用MaxMind GeoIP2数据库进行IP地理位置查询
//! 数据库文件应放在程序同级目录的 data/ 下：
//! - data/GeoLite2-Country.mmdb

use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use maxminddb::{Reader, geoip2};
use once_cell::sync::OnceCell;
use parking_lot::RwLock;

use crate::config;

/// GeoIP查询结果
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct GeoInfo {
    pub country_code: Option<String>,
    pub country_name: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub timezone: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub asn: Option<u32>,
    pub asn_org: Option<String>,
    pub is_datacenter: bool,
}

/// GeoIP管理器
pub struct GeoIpManager {
    country_reader: Option<Reader<Vec<u8>>>,
    city_reader: Option<Reader<Vec<u8>>>,
    asn_reader: Option<Reader<Vec<u8>>>,
}

impl GeoIpManager {
    pub fn new() -> Self {
        Self {
            country_reader: None,
            city_reader: None,
            asn_reader: None,
        }
    }

    pub fn load_from_dir<P: AsRef<Path>>(&mut self, dir: P) -> anyhow::Result<()> {
        let dir = dir.as_ref();
        
        for (name, field) in [
            ("GeoLite2-Country.mmdb", &mut self.country_reader),
            ("GeoLite2-City.mmdb", &mut self.city_reader),
            ("GeoLite2-ASN.mmdb", &mut self.asn_reader),
        ] {
            let path = dir.join(name);
            if path.exists() {
                match Reader::open_readfile(&path) {
                    Ok(reader) => {
                        tracing::info!("Loading GeoIP database: {:?}", path);
                        *field = Some(reader);
                    }
                    Err(e) => tracing::warn!("Failed to load {}: {}", name, e),
                }
            }
        }
        Ok(())
    }

    pub fn lookup(&self, ip: IpAddr) -> GeoInfo {
        let mut info = GeoInfo::default();
        
        if is_private_ip(&ip) {
            info.country_code = Some("LOCAL".to_string());
            info.country_name = Some("Local Network".to_string());
            return info;
        }
        
        if let Some(ref reader) = self.country_reader {
            if let Ok(country) = reader.lookup::<geoip2::Country>(ip) {
                if let Some(c) = country.country {
                    info.country_code = c.iso_code.map(|s| s.to_string());
                    if let Some(names) = c.names {
                        info.country_name = names.get("zh-CN")
                            .or_else(|| names.get("en"))
                            .map(|s| s.to_string());
                    }
                }
            }
        }
        
        if let Some(ref reader) = self.city_reader {
            if let Ok(city) = reader.lookup::<geoip2::City>(ip) {
                if let Some(c) = city.country {
                    info.country_code = c.iso_code.map(|s| s.to_string());
                    if let Some(names) = c.names {
                        info.country_name = names.get("zh-CN")
                            .or_else(|| names.get("en"))
                            .map(|s| s.to_string());
                    }
                }
                if let Some(c) = city.city {
                    if let Some(names) = c.names {
                        info.city = names.get("zh-CN")
                            .or_else(|| names.get("en"))
                            .map(|s| s.to_string());
                    }
                }
                if let Some(subdivisions) = city.subdivisions {
                    if let Some(first) = subdivisions.first() {
                        if let Some(names) = &first.names {
                            info.region = names.get("zh-CN")
                                .or_else(|| names.get("en"))
                                .map(|s| s.to_string());
                        }
                    }
                }
                if let Some(location) = city.location {
                    info.latitude = location.latitude;
                    info.longitude = location.longitude;
                    info.timezone = location.time_zone.map(|s| s.to_string());
                }
            }
        }
        
        if let Some(ref reader) = self.asn_reader {
            if let Ok(asn) = reader.lookup::<geoip2::Asn>(ip) {
                info.asn = asn.autonomous_system_number;
                info.asn_org = asn.autonomous_system_organization.map(|s| s.to_string());
                if let Some(ref org) = info.asn_org {
                    let org_lower = org.to_lowercase();
                    info.is_datacenter = ["cloud", "hosting", "server", "datacenter", "amazon", "google", "microsoft", "alibaba", "tencent"]
                        .iter().any(|k| org_lower.contains(k));
                }
            }
        }
        
        info
    }

    pub fn get_country_code(&self, ip: IpAddr) -> Option<String> {
        self.lookup(ip).country_code
    }

    pub fn is_china(&self, ip: IpAddr) -> bool {
        self.get_country_code(ip).map(|c| c.eq_ignore_ascii_case("CN")).unwrap_or(false)
    }

    pub fn is_loaded(&self) -> bool {
        self.country_reader.is_some() || self.city_reader.is_some()
    }
}

impl Default for GeoIpManager {
    fn default() -> Self { Self::new() }
}

pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            ipv4.is_private() || ipv4.is_loopback() || ipv4.is_link_local()
                || (ipv4.octets()[0] == 100 && ipv4.octets()[1] >= 64 && ipv4.octets()[1] <= 127)
        }
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback() || ipv6.is_unspecified()
                || (ipv6.segments()[0] & 0xfe00) == 0xfc00
                || (ipv6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

pub fn extract_client_ip(headers: &axum::http::HeaderMap, connect_ip: Option<IpAddr>) -> Option<IpAddr> {
    for header in ["CF-Connecting-IP", "X-Real-IP"] {
        if let Some(val) = headers.get(header) {
            if let Ok(ip_str) = val.to_str() {
                if let Ok(ip) = ip_str.parse::<IpAddr>() {
                    return Some(ip);
                }
            }
        }
    }
    
    if let Some(forwarded) = headers.get("X-Forwarded-For") {
        if let Ok(ip_str) = forwarded.to_str() {
            if let Some(first_ip) = ip_str.split(',').next() {
                if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                    return Some(ip);
                }
            }
        }
    }
    
    connect_ip
}

pub fn hash_ip(ip: &IpAddr) -> u64 {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    ip.hash(&mut hasher);
    hasher.finish()
}

static GEOIP_MANAGER: OnceCell<Arc<RwLock<GeoIpManager>>> = OnceCell::new();

pub fn get_geoip_manager() -> Arc<RwLock<GeoIpManager>> {
    GEOIP_MANAGER.get_or_init(|| {
        let mut manager = GeoIpManager::new();
        let geoip_dir = config::config().get_geoip_dir();
        let _ = manager.load_from_dir(&geoip_dir);
        Arc::new(RwLock::new(manager))
    }).clone()
}

pub fn lookup_country(ip: IpAddr) -> Option<String> {
    get_geoip_manager().read().get_country_code(ip)
}

pub fn lookup_ip(ip: IpAddr) -> GeoInfo {
    get_geoip_manager().read().lookup(ip)
}

pub fn is_china_ip(ip: IpAddr) -> bool {
    get_geoip_manager().read().is_china(ip)
}
