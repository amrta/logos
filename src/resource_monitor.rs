use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const CPU_LIMIT: f32 = 40.0;
pub const CPU_CRITICAL: f32 = 70.0;
pub const MEM_LIMIT_MB: usize = 200;
const THROTTLE_DURATION_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct Metrics {
    pub cpu: f32,
    pub mem_mb: usize,
    pub temp_est: f32,
}

pub enum Status {
    Ok(Metrics),
    Warn(Metrics, &'static str),
    Critical(Metrics, &'static str),
}


pub struct Monitor {
    last_warn_time: u64,
    last_critical_time: u64,
}

impl Monitor {
    pub fn new() -> Self {
        Self {
            last_warn_time: 0,
            last_critical_time: 0,
        }
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn check(&mut self) -> Status {
        let cpu = Self::cpu();
        let mem = Self::mem();
        let temp = 25.0 + cpu * 0.3;
        let m = Metrics {
            cpu,
            mem_mb: mem,
            temp_est: temp,
        };
        let now = Self::now_secs();

        if cpu > CPU_CRITICAL {
            if now - self.last_critical_time >= THROTTLE_DURATION_SECS {
                self.last_critical_time = now;
                return Status::Critical(m, "CPU过高");
            }
            return Status::Ok(m);
        }

        if cpu > CPU_LIMIT || mem > MEM_LIMIT_MB {
            if now - self.last_warn_time >= THROTTLE_DURATION_SECS {
                self.last_warn_time = now;
                return Status::Warn(m, "资源偏高");
            }
            return Status::Ok(m);
        }

        Status::Ok(m)
    }

    fn cpu() -> f32 {
        let output = Command::new("ps")
            .args(["-p", &std::process::id().to_string(), "-o", "%cpu="])
            .output();
        match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse()
                .unwrap_or(0.0),
            Err(_) => 0.0,
        }
    }

    fn mem() -> usize {
        let output = Command::new("ps")
            .args(["-p", &std::process::id().to_string(), "-o", "rss="])
            .output();
        match output {
            Ok(o) => {
                let kb: usize = String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse()
                    .unwrap_or(0);
                kb / 1024
            }
            Err(_) => 0,
        }
    }
}
