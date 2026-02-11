// ============================================================================
// LOGOS 逻辑层 (Logic Layer) - 无熵，被动接收优化，仅允许读取禁止修改
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer { Bedrock, Logic, Orchestrator, Pouch }

#[derive(Debug, Clone, PartialEq)]
pub enum RouteDecision {
    ToPouch(String),
    SystemCommand(SystemCmd),
    Reject(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SystemCmd {
    InstallPouch(String),
    UninstallPouch(String),
    SleepPouch(String),
    WakePouch(String),
    ConfigShow,
    ConfigSet(String, String),
    ListPouches,
    Status,
    Help,
    Teach(String, String),
    RunPipeline(Vec<String>, String),
    SelfTest,
    ImportPatterns(String),
    ExportPatterns,
    Rollback,
    Explain(String),
    Train,
    EvolutionStatus,
    Capabilities,
    ClearContext,
}

pub fn route(input: &str, installed: &[&str]) -> RouteDecision {
    let lower = input.to_lowercase();
    let trimmed = lower.trim();

    if trimmed.starts_with("安装尿袋") || trimmed.starts_with("install ") {
        let name = trimmed.replace("安装尿袋", "").replace("install ", "").trim().to_string();
        return RouteDecision::SystemCommand(SystemCmd::InstallPouch(name));
    }
    if trimmed.starts_with("卸载尿袋") || trimmed.starts_with("uninstall ") {
        let name = trimmed.replace("卸载尿袋", "").replace("uninstall ", "").trim().to_string();
        return RouteDecision::SystemCommand(SystemCmd::UninstallPouch(name));
    }
    if trimmed.starts_with("休眠") || trimmed.starts_with("sleep ") {
        let name = trimmed.replace("休眠", "").replace("sleep ", "").trim().to_string();
        return RouteDecision::SystemCommand(SystemCmd::SleepPouch(name));
    }
    if trimmed.starts_with("唤醒") || trimmed.starts_with("wake ") {
        let name = trimmed.replace("唤醒", "").replace("wake ", "").trim().to_string();
        return RouteDecision::SystemCommand(SystemCmd::WakePouch(name));
    }
    if trimmed == "配置" || trimmed == "config" || trimmed == "show config" {
        return RouteDecision::SystemCommand(SystemCmd::ConfigShow);
    }
    if trimmed.starts_with("配置设置") || trimmed.starts_with("config set ") {
        let content = trimmed.replace("配置设置", "").replace("config set ", "").trim().to_string();
        if let Some((key, value)) = content.split_once(' ') {
            return RouteDecision::SystemCommand(SystemCmd::ConfigSet(key.to_string(), value.to_string()));
        }
    }
    if trimmed == "尿袋列表" || trimmed == "list" || trimmed == "pouches" {
        return RouteDecision::SystemCommand(SystemCmd::ListPouches);
    }
    if trimmed == "状态" || trimmed == "status" {
        return RouteDecision::SystemCommand(SystemCmd::Status);
    }
    if trimmed == "自检" || trimmed == "selftest" {
        return RouteDecision::SystemCommand(SystemCmd::SelfTest);
    }
    if trimmed == "帮助" || trimmed == "help" || trimmed == "?" {
        return RouteDecision::SystemCommand(SystemCmd::Help);
    }
    if trimmed.contains("->") && (trimmed.starts_with("教你") || trimmed.starts_with("learn")) {
        let content = trimmed.replace("教你", "").replace("learn", "").trim().to_string();
        if let Some((trigger, response)) = content.split_once("->") {
            return RouteDecision::SystemCommand(SystemCmd::Teach(
                trigger.trim().to_string(),
                response.trim().to_string(),
            ));
        }
    }
    if trimmed.starts_with("流水线") || trimmed.starts_with("pipeline") {
        let content = trimmed.replace("流水线", "").replace("pipeline", "").trim().to_string();
        if let Some((stages_str, data)) = content.split_once(':') {
            let stages: Vec<String> = stages_str.split(',').map(|s| s.trim().to_string()).collect();
            if !stages.is_empty() {
                return RouteDecision::SystemCommand(SystemCmd::RunPipeline(stages, data.trim().to_string()));
            }
        }
    }
    if trimmed == "导出模式" || trimmed == "export patterns" {
        return RouteDecision::SystemCommand(SystemCmd::ExportPatterns);
    }
    if trimmed.starts_with("导入模式") || trimmed.starts_with("import patterns") {
        let path = trimmed.replace("导入模式", "").replace("import patterns", "").trim().to_string();
        return RouteDecision::SystemCommand(SystemCmd::ImportPatterns(path));
    }
    if trimmed == "回滚" || trimmed == "rollback" {
        return RouteDecision::SystemCommand(SystemCmd::Rollback);
    }
    if trimmed == "训练" || trimmed == "train" {
        return RouteDecision::SystemCommand(SystemCmd::Train);
    }
    if trimmed == "演化" || trimmed == "evolution" || trimmed == "演化状态" {
        return RouteDecision::SystemCommand(SystemCmd::EvolutionStatus);
    }
    if trimmed == "能力" || trimmed == "capabilities" || trimmed == "原子能力" {
        return RouteDecision::SystemCommand(SystemCmd::Capabilities);
    }
    if trimmed == "清空上下文" || trimmed == "clear context" || trimmed == "重置对话" {
        return RouteDecision::SystemCommand(SystemCmd::ClearContext);
    }
    if trimmed.starts_with("解释") || trimmed.starts_with("explain ") {
        let name = trimmed.replace("解释", "").replace("explain ", "").trim().to_string();
        return RouteDecision::SystemCommand(SystemCmd::Explain(name));
    }

    if let Some(target) = check_promoted_route(trimmed) {
        if installed.contains(&target.as_str()) {
            return RouteDecision::ToPouch(target);
        }
    }

    for pouch in installed {
        if trimmed.contains(&pouch.to_lowercase()) {
            return RouteDecision::ToPouch(pouch.to_string());
        }
    }

    RouteDecision::Reject("Unknown command")
}

pub fn decompose_intent(input: &str) -> Vec<crate::atom::AtomKind> {
    use crate::atom::AtomKind;
    let lower = input.to_lowercase();

    if (lower.contains("材料") || lower.contains("material"))
        && (lower.contains("打印") || lower.contains("print") || lower.contains("制造"))
    {
        return vec![AtomKind::Transform, AtomKind::Generate];
    }

    if (lower.contains("分析") || lower.contains("analyze") || lower.contains("检查"))
        && (lower.contains("分子") || lower.contains("化学") || lower.contains("元素"))
    {
        return vec![AtomKind::Transform];
    }

    if lower.contains("对比") || lower.contains("比较") || lower.contains("compare") {
        return vec![AtomKind::Match, AtomKind::Score];
    }

    if lower.contains("推理") || lower.contains("计算") || lower.contains("推导") {
        return vec![AtomKind::Score];
    }

    if lower.contains("创建") || lower.contains("生成") || lower.contains("创作") {
        return vec![AtomKind::Generate];
    }

    if lower.contains("搜索") || lower.contains("发现") || lower.contains("查找") {
        return vec![AtomKind::Match];
    }

    if lower.contains("消歧") || lower.contains("解释") || lower.contains("是什么") {
        return vec![AtomKind::Transform];
    }

    vec![]
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PromotedRule {
    pub input_pattern: String,
    pub target_pouch: String,
    pub verify_count: u32,
    pub promoted_at: u64,
}

static PROMOTED_RULES: std::sync::Mutex<Vec<PromotedRule>> = std::sync::Mutex::new(Vec::new());

pub fn accept_promoted_rule(rule: PromotedRule) -> Result<(), &'static str> {
    let mut rules = PROMOTED_RULES.lock().map_err(|_| "锁失败")?;
    for existing in rules.iter() {
        if existing.input_pattern == rule.input_pattern {
            if existing.target_pouch != rule.target_pouch {
                return Err("路由冲突：相同输入已存在不同路由");
            }
            return Ok(());
        }
    }
    if rules.len() >= 200 {
        return Err("规则容量已满");
    }
    rules.push(rule);
    Ok(())
}

pub fn check_promoted_route(input: &str) -> Option<String> {
    let rules = PROMOTED_RULES.lock().ok()?;
    let lower = input.to_lowercase();
    for rule in rules.iter() {
        if lower.contains(&rule.input_pattern) {
            return Some(rule.target_pouch.clone());
        }
    }
    None
}

pub fn promoted_rules_count() -> usize {
    PROMOTED_RULES.lock().map(|r| r.len()).unwrap_or(0)
}

pub fn save_promoted_rules() -> Result<Vec<u8>, String> {
    let rules = PROMOTED_RULES.lock().map_err(|_| "锁失败".to_string())?;
    serde_json::to_vec(&*rules).map_err(|e| format!("序列化失败: {}", e))
}

pub fn load_promoted_rules(data: &[u8]) -> Result<(), String> {
    let loaded: Vec<PromotedRule> = serde_json::from_slice(data).map_err(|e| format!("反序列化失败: {}", e))?;
    let mut rules = PROMOTED_RULES.lock().map_err(|_| "锁失败".to_string())?;
    *rules = loaded;
    Ok(())
}

pub fn adjacent(from: Layer, to: Layer) -> bool {
    matches!(
        (from, to),
        (Layer::Bedrock, Layer::Logic)
            | (Layer::Logic, Layer::Bedrock)
            | (Layer::Logic, Layer::Orchestrator)
            | (Layer::Orchestrator, Layer::Logic)
            | (Layer::Orchestrator, Layer::Pouch)
            | (Layer::Pouch, Layer::Orchestrator)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adjacent_valid() {
        assert!(adjacent(Layer::Orchestrator, Layer::Pouch));
        assert!(adjacent(Layer::Pouch, Layer::Orchestrator));
        assert!(adjacent(Layer::Orchestrator, Layer::Logic));
        assert!(adjacent(Layer::Logic, Layer::Orchestrator));
    }

    #[test]
    fn test_adjacent_invalid() {
        assert!(!adjacent(Layer::Bedrock, Layer::Pouch));
        assert!(!adjacent(Layer::Pouch, Layer::Bedrock));
        assert!(!adjacent(Layer::Bedrock, Layer::Orchestrator));
    }

    #[test]
    fn test_route_install() {
        let result = route("安装尿袋材料", &[]);
        assert_eq!(result, RouteDecision::SystemCommand(SystemCmd::InstallPouch("材料".into())));
    }

    #[test]
    fn test_route_help() {
        let result = route("帮助", &[]);
        assert_eq!(result, RouteDecision::SystemCommand(SystemCmd::Help));
    }

    #[test]
    fn test_route_teach() {
        let result = route("教你 天气 -> 晴天", &[]);
        assert_eq!(result, RouteDecision::SystemCommand(SystemCmd::Teach("天气".into(), "晴天".into())));
    }

    #[test]
    fn test_route_to_pouch() {
        let result = route("你好 language", &["language"]);
        assert_eq!(result, RouteDecision::ToPouch("language".into()));
    }

    #[test]
    fn test_decompose_material_print() {
        let kinds = decompose_intent("用材料打印一个零件");
        assert_eq!(kinds.len(), 2);
        assert_eq!(kinds[0], crate::atom::AtomKind::Transform);
        assert_eq!(kinds[1], crate::atom::AtomKind::Generate);
    }

    #[test]
    fn test_decompose_empty() {
        let kinds = decompose_intent("你好");
        assert!(kinds.is_empty());
    }

    #[test]
    fn test_route_capabilities() {
        let result = route("能力", &[]);
        assert_eq!(result, RouteDecision::SystemCommand(SystemCmd::Capabilities));
    }
}
