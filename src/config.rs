use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PouchConfig {
    pub name: String,
    pub enabled: bool,
    pub role: String,
    pub auto_load: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSleepConfig {
    pub enabled: bool,
    pub idle_threshold_secs: u64,
    pub check_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_pouches: usize,
    pub max_patterns: usize,
    pub memory_mb: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingScoreConfig {
    #[serde(default = "RoutingScoreConfig::default_baseline")]
    pub baseline_score: f64,
    #[serde(default = "RoutingScoreConfig::default_low_threshold")]
    pub low_score_threshold: f64,
    #[serde(default = "RoutingScoreConfig::default_promote_min_chain_score")]
    pub promote_min_chain_score: f64,
}

impl RoutingScoreConfig {
    fn default_baseline() -> f64 { 1.0 }
    fn default_low_threshold() -> f64 { 0.5 }
    fn default_promote_min_chain_score() -> f64 { 0.0 }
}

impl Default for RoutingScoreConfig {
    fn default() -> Self {
        Self {
            baseline_score: 1.0,
            low_score_threshold: 0.5,
            promote_min_chain_score: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    pub pouches: Vec<PouchConfig>,
    pub auto_sleep: AutoSleepConfig,
    pub resource_limits: ResourceLimits,
    #[serde(default)]
    pub routing_score: RoutingScoreConfig,
    pub version: String,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            pouches: vec![],
            auto_sleep: AutoSleepConfig {
                enabled: true,
                idle_threshold_secs: 300,
                check_interval_secs: 60,
            },
            resource_limits: ResourceLimits {
                max_pouches: 20,
                max_patterns: 10000,
                memory_mb: 512,
            },
            routing_score: RoutingScoreConfig::default(),
            version: "1.0".to_string(),
        }
    }
}

impl SystemConfig {
    pub fn load(path: &str) -> Result<Self, String> {
        if !Path::new(path).exists() {
            return Ok(Self::default());
        }
        match fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<Self>(&content) {
                Ok(config) => Ok(config),
                Err(e) => Err(format!("解析配置文件失败: {}", e)),
            },
            Err(e) => Err(format!("读取配置文件失败: {}", e)),
        }
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        let dir = Path::new(path).parent().unwrap_or_else(|| Path::new("."));
        if !dir.exists() {
            fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {}", e))?;
        }
        match serde_json::to_string_pretty(self) {
            Ok(content) => {
                fs::write(path, content).map_err(|e| format!("写入配置文件失败: {}", e))
            }
            Err(e) => Err(format!("序列化配置失败: {}", e)),
        }
    }
}
