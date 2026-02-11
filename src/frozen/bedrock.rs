// ============================================================================
// LOGOS 公式层 (Formula Layer) - 完全不可修改
// ============================================================================
// 这一层定义了系统的基本常数和不可更改的规则。
// 任何对这一层的修改都是被禁止的，违反会导致编译错误。
// ============================================================================

pub const VERSION: &str = "5.0.0";
pub const RECURSIVE_MAX_DEPTH: usize = 16;
pub const MAX_POUCHES: usize = 32;
pub const MAX_PIPELINE_STAGES: usize = 8;
pub const SIMILARITY_THRESHOLD: f64 = 0.3;
pub const MAX_PATTERNS: usize = 8000;
pub const LEARNING_RATE: f64 = 0.15;
pub const MAX_INPUT_LEN: usize = 4096;
pub const LRU_EVICT_COUNT: usize = 320;

#[allow(dead_code)]
pub const PHYSICAL_ISOLATION_PRINCIPLE: &str = "无法自指 + 完全隔离 + 确定性逻辑";
#[allow(dead_code)]
pub const MIN_SECURITY_THRESHOLD: f64 = 0.99;
#[allow(dead_code)]
pub const MAX_ENTROPY_ALLOWED: f64 = 0.3;

#[allow(dead_code)]
pub fn get_formula_version() -> &'static str {
    VERSION
}

#[allow(dead_code)]
pub fn validate_constants() -> Result<(), String> {
    if RECURSIVE_MAX_DEPTH == 0 {
        return Err("RECURSIVE_MAX_DEPTH must be > 0".to_string());
    }
    if MAX_POUCHES == 0 {
        return Err("MAX_POUCHES must be > 0".to_string());
    }
    if LEARNING_RATE < 0.0 || LEARNING_RATE > 1.0 {
        return Err("LEARNING_RATE must be between 0 and 1".to_string());
    }
    if MIN_SECURITY_THRESHOLD < 0.0 || MIN_SECURITY_THRESHOLD > 1.0 {
        return Err("MIN_SECURITY_THRESHOLD must be between 0 and 1".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants_validity() {
        assert!(validate_constants().is_ok());
    }

    #[test]
    fn test_formula_constants() {
        assert!(RECURSIVE_MAX_DEPTH > 0);
        assert!(MAX_POUCHES > 0);
        assert!(MAX_PIPELINE_STAGES > 0);
        assert!(LEARNING_RATE > 0.0);
        assert!(LEARNING_RATE <= 1.0);
    }

    #[test]
    fn test_security_principle() {
        assert!(!PHYSICAL_ISOLATION_PRINCIPLE.is_empty());
        assert!(MIN_SECURITY_THRESHOLD > 0.9);
    }
}
