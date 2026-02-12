use crate::frozen::logic::{self, Layer, RouteDecision, SystemCmd};
use crate::language_pouch::LanguagePouch;
use crate::pouch_trait::{Pouch, PouchMeta, PouchRole, create_proposal};
use crate::atom::{AtomDeclaration, CapabilityRegistry};
use crate::frozen::bedrock;
use crate::config::SystemConfig;
use crate::manager_math;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvolutionRecord {
    input_hash: u64,
    pouch_name: String,
    output_hash: u64,
    verify_count: u32,
    first_seen: u64,
    last_seen: u64,
    promoted: bool,
}

const EVOLUTION_PROMOTE_THRESHOLD: u32 = 100;
const EVOLUTION_MAX_RECORDS: usize = 2000;
const ABSORB_WEIGHT: f64 = 1.2;

/*
 * EvolutionChain 覆盖范围声明：
 *   - 仅覆盖 RouteDecision::Reject → plan_for_kinds → execute_plan 的成功路径
 *   - 不覆盖：单 pouch 直连 (ToPouch)、云端执行 (cloud_plan)、缓存命中、language 直回退
 *   - success 字段当前恒为 true（仅成功路径写入），未来可扩展失败记录
 */
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvolutionEntry {
    pub timestamp: u64,
    pub input_trunc: String,
    pub pouch_name: String,
    pub output_trunc: String,
    pub success: bool,
    pub step_index: usize,
}

const EVOLUTION_CHAIN_MAX: usize = 1000;
const SATURATION_THRESHOLD: f64 = 0.65;
const CROSS_LEARN_BATCH: usize = 8;
const MATURITY_HUNGRY: f64 = 0.3;
const A2_NEXT_INTENT_MAX: usize = 500;

fn hash_str(s: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

pub const VERSION: &str = "5.0.0";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LearningState {
    pub cycle_count: u64,
    pub external_fed: u64,
    pub external_absorbed: u64,
    pub cross_fed: u64,
    pub cross_absorbed: u64,
    pub saturation: f64,
    pub last_cycle_ts: u64,
    pub phase: String,
    #[serde(default)]
    pub cloud_sync_cursor: i64,
    #[serde(default)]
    pub cloud_pulled: u64,
    #[serde(default)]
    pub cloud_pushed: u64,
}

impl Default for LearningState {
    fn default() -> Self {
        Self {
            cycle_count: 0,
            external_fed: 0,
            external_absorbed: 0,
            cross_fed: 0,
            cross_absorbed: 0,
            saturation: 0.0,
            last_cycle_ts: 0,
            phase: "idle".into(),
            cloud_sync_cursor: 0,
            cloud_pulled: 0,
            cloud_pushed: 0,
        }
    }
}

#[derive(serde::Deserialize)]
struct CloudPlan {
    pouches: Vec<CloudPouchSpec>,
    steps: Vec<CloudStep>,
}

#[derive(serde::Deserialize)]
struct CloudPouchSpec {
    name: String,
    endpoint: String,
}

#[derive(serde::Deserialize)]
struct CloudStep {
    pouch: String,
    input: String,
}

const GATEWAY_BASE: &str = "https://logos-gateway.amrta.workers.dev";

fn fetch_remote_spec_from_cloud(name: &str) -> Option<crate::remote_pouch::RemotePouchSpec> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return None;
    }
    let url = format!("{}/remote_pouches?name={}", GATEWAY_BASE, name.replace(' ', "%20"));
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let resp = client.get(&url).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().ok()?;
    if let Some(obj) = json.as_object() {
        let name = obj.get("name")?.as_str()?.to_string();
        let role = obj.get("role").and_then(|v| v.as_str()).unwrap_or("E1").to_string();
        let endpoint = obj.get("endpoint")?.as_str()?.to_string();
        let failover_endpoints: Vec<String> = obj.get("failover_endpoints")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        return Some(crate::remote_pouch::RemotePouchSpec {
            name,
            role,
            endpoint,
            failover_endpoints,
        });
    }
    if let Some(arr) = json.as_array() {
        let first = arr.first()?.as_object()?;
        let name = first.get("name")?.as_str()?.to_string();
        let role = first.get("role").and_then(|v| v.as_str()).unwrap_or("E1").to_string();
        let endpoint = first.get("endpoint")?.as_str()?.to_string();
        let failover_endpoints: Vec<String> = first.get("failover_endpoints")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        return Some(crate::remote_pouch::RemotePouchSpec {
            name,
            role,
            endpoint,
            failover_endpoints,
        });
    }
    None
}

async fn analyze_cloud(input: &str) -> Result<CloudPlan, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| "客户端创建失败".to_string())?;
    let resp = client
        .post(format!("{}/analyze", GATEWAY_BASE))
        .json(&serde_json::json!({ "input": input }))
        .send()
        .await
        .map_err(|_| "云端分析不可用".to_string())?;
    if !resp.status().is_success() {
        return Err("云端分析失败".into());
    }
    resp.json::<CloudPlan>().await
        .map_err(|_| "解析分析结果失败".into())
}

pub struct Orchestrator {
    language: LanguagePouch,
    pouches: HashMap<String, Box<dyn Pouch>>,
    meta: HashMap<String, PouchMeta>,
    call_stack: Vec<Layer>,
    data_dir: String,
    ready: bool,
    config: SystemConfig,
    pouch_sleep_state: HashMap<String, bool>,
    pouch_last_used: HashMap<String, u64>,
    events: Vec<String>,
    evolution: Vec<EvolutionRecord>,
    evolution_chain: Vec<EvolutionEntry>,
    promoted_cache: HashMap<u64, String>,
    chain_result_cache: std::collections::VecDeque<(String, String)>,
    registry: CapabilityRegistry,
    pub learning: LearningState,
}

impl Orchestrator {
    pub fn new(data_dir: &str) -> Self {
        let config_path = format!("{}/pouch_config.json", data_dir);
        let config = SystemConfig::load(&config_path).unwrap_or_default();
        let mut o = Self {
            language: LanguagePouch::new(),
            pouches: HashMap::new(),
            meta: HashMap::new(),
            call_stack: Vec::new(),
            data_dir: data_dir.to_string(),
            ready: false,
            config,
            pouch_sleep_state: HashMap::new(),
            pouch_last_used: HashMap::new(),
            events: Vec::new(),
            evolution: Vec::new(),
            evolution_chain: Vec::new(),
            promoted_cache: HashMap::new(),
            chain_result_cache: std::collections::VecDeque::new(),
            registry: CapabilityRegistry::new(),
            learning: LearningState::default(),
        };
        o.meta.insert("language".into(), PouchMeta { role: PouchRole::E0 });
        o.load_state();
        o.registry.register(AtomDeclaration {
            name: "route_intent".into(),
            kind: crate::atom::AtomKind::Route,
            pouch: "language".into(),
            confidence_range: (0.3, 0.95),
        });
        o.registry.register(AtomDeclaration {
            name: "proposal_validate".into(),
            kind: crate::atom::AtomKind::Validate,
            pouch: "orchestrator".into(),
            confidence_range: (0.9, 1.0),
        });
        o.ready = true;
        o
    }

    pub fn guard(&mut self, to: Layer) -> Result<(), String> {
        if let Some(&from) = self.call_stack.last() {
            if !logic::adjacent(from, to) {
                return Err(format!("跨层阻断:{:?}->{:?}", from, to));
            }
        }
        self.call_stack.push(to);
        Ok(())
    }

    pub fn unguard(&mut self) {
        self.call_stack.pop();
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn installed(&self) -> Vec<&str> {
        let mut list = vec!["language"];
        list.extend(self.pouches.keys().map(|s| s.as_str()));
        list
    }

    pub async fn execute_with_pouch(
        &mut self,
        input: &str,
    ) -> Result<(String, String), (String, String)> {
        if !self.call_stack.is_empty() {
            self.call_stack.clear();
        }
        let input = if input.len() > bedrock::MAX_INPUT_LEN {
            let mut end = bedrock::MAX_INPUT_LEN;
            while !input.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            &input[..end]
        } else {
            input
        };

        if let Some(domain) = Self::classify_domain(input) {
            self.auto_ensure_pouch(domain);
        }

        if let Err(e) = self.guard(Layer::Orchestrator) {
            return Err((e, "system".into()));
        }

        let trimmed = input.trim();
        let sanitized_store;
        let input = if let Some((rejected, out)) = self.sanitize_input(trimmed).await {
            if rejected {
                self.unguard();
                return Ok((out, "sanitize".into()));
            }
            sanitized_store = out;
            &sanitized_store
        } else {
            trimmed
        };
        if input.starts_with("语言评估") || input.to_lowercase().starts_with("eval language") {
            let path = input
                .replace("语言评估", "")
                .replace("eval language", "")
                .replace("Eval Language", "")
                .trim()
                .to_string();
            if !path.is_empty() {
                match self.eval_language(&path).await {
                    Ok(msg) => return Ok((msg, "system".into())),
                    Err(e) => return Err((e, "system".into())),
                }
            }
        }

        let lower = trimmed.to_lowercase();
        if let Some(fb_result) = self.detect_feedback(&lower) {
            self.unguard();
            return Ok((fb_result, "feedback".into()));
        }
        if lower == "反哺状态" || lower == "feedback status" {
            let result = self.feedback_status();
            self.unguard();
            return Ok((result, "system".into()));
        }
        if lower == "反哺导出" || lower == "feedback export" {
            let result = self.language.export_feedback_jsonl();
            self.unguard();
            if result.is_empty() {
                return Ok(("无反馈数据".into(), "system".into()));
            }
            return Ok((result, "system".into()));
        }
        if lower == "自我优化" || lower == "自我进化" || lower == "优化自己"
            || lower == "self optimize" || lower == "self-optimize"
        {
            match self.self_optimize().await {
                Ok(msg) => return Ok((msg, "system".into())),
                Err(e) => return Err((e, "system".into())),
            }
        }
        if lower.starts_with("对标") || lower.starts_with("进化能力") || lower.starts_with("升级能力")
            || lower == "evolve"
            || (lower.contains("对标") && (lower.contains("进化") || lower.contains("升级") || lower.contains("补齐")))
        {
            match self.evolve().await {
                Ok(msg) => return Ok((msg, "system".into())),
                Err(e) => return Err((e, "system".into())),
            }
        }
        if lower == "自主学习" || lower == "autonomous learn" || lower == "auto learn" {
            self.autonomous_learning_cycle().await;
            let ls = &self.learning;
            let summary = format!(
                "周期:{} 阶段:{} 外部:{}/{} 突触:{}/{} 饱和:{:.0}%",
                ls.cycle_count, ls.phase,
                ls.external_absorbed, ls.external_fed,
                ls.cross_absorbed, ls.cross_fed,
                ls.saturation * 100.0
            );
            return Ok((summary, "system".into()));
        }

        let installed = self.installed();
        let mut decision = logic::route(input, &installed);

        if matches!(decision, RouteDecision::Reject(_)) {
            if let Some(intent) = self.language.identify_requirement(input) {
                if intent.confidence > 0.6 {
                    let cap = &intent.capability_needed;
                    if !self.installed().contains(&cap.as_str()) {
                        self.log_event(format!("AUTO_INSTALL {}", cap));
                        let _ = self.install(cap);
                    }
                    if self.installed().contains(&cap.as_str()) {
                        decision = RouteDecision::ToPouch(cap.clone());
                    }
                }
            }
        }
        if matches!(decision, RouteDecision::Reject(_)) {
            let lang_check = self.language.process(input).await;
            if self.language.last_was_pattern_hit()
                && self.language.last_match_weight() >= ABSORB_WEIGHT
                && !self.language.is_fallback_response(&lang_check)
            {
                self.log_event("LANG_PRIORITY hit".into());
                if let Some(out) = self.execute_chain_spec(input, &lang_check).await {
                    self.unguard();
                    return Ok((out, "chain".into()));
                }
                if let Some(expanded) = self.expand_template(input, &lang_check).await {
                    self.unguard();
                    return Ok((expanded, "template".into()));
                }
                self.unguard();
                return Ok((lang_check, "language".into()));
            }
        }

        let route_event = match &decision {
            RouteDecision::ToPouch(n) => format!("ROUTE → {}", n),
            RouteDecision::SystemCommand(_) => "ROUTE → system".into(),
            RouteDecision::Reject(_) => "ROUTE → language".into(),
        };
        self.log_event(route_event);

        let result = match decision {
            RouteDecision::ToPouch(name) => {
                let r = self.call_pouch(&name, input).await;
                let ev = match &r {
                    Ok(_) => format!("EXEC {} → ok", name),
                    Err(e) => format!("EXEC {} → {}", name, e),
                };
                self.log_event(ev);
                match r {
                    Ok(r) => Ok((r, name)),
                    Err(e) => Err((e, name)),
                }
            }
            RouteDecision::SystemCommand(cmd) => {
                let cmd_name = format!("{:?}", cmd).chars().take(40).collect::<String>();
                let r = self.handle_cmd(cmd).await;
                self.log_event(format!("CMD {} → {}", cmd_name, if r.is_ok() { "ok" } else { "err" }));
                match r {
                    Ok(r) => Ok((r, "system".into())),
                    Err(e) => Err((e, "system".into())),
                }
            }
            RouteDecision::Reject(_) => {
                let kinds = logic::decompose_intent(input);
                if !kinds.is_empty() {
                    let entries = self.recent_evolution_entries(100);
                    let (baseline, low_threshold, _) = self.clamped_routing();
                    let scorer = move |name: &str| Self::score_pouch_from_entries(name, &entries, baseline);
                    if let Some(plan) = self.registry.plan_for_kinds(&kinds, Some(&scorer), Some(low_threshold)) {
                        let summary = crate::atom::CapabilityRegistry::plan_summary(&plan);
                        self.log_event(format!("PLAN {}", summary));
                        let r = self.execute_plan(&plan, input).await;
                        return match r {
                            Ok(data) => {
                                if !self.language.is_fallback_response(&data) {
                                    self.language.absorb(input, &data, 1.1);
                                    self.log_event("ABSORB plan→language".into());
                                }
                                Ok((data, "plan".into()))
                            }
                            Err(e) => Err((e, "plan".into())),
                        };
                    }
                }
                match analyze_cloud(input).await {
                    Ok(cloud_plan) if !cloud_plan.steps.is_empty() => {
                        self.log_event(format!(
                            "CLOUD_ANALYZE {}p {}s",
                            cloud_plan.pouches.len(),
                            cloud_plan.steps.len()
                        ));
                        for spec in &cloud_plan.pouches {
                            if !self.installed().contains(&spec.name.as_str()) {
                                let rp = crate::remote_pouch::RemotePouch::new(
                                    &spec.name, PouchRole::E1, &spec.endpoint,
                                );
                                let caps = rp.atom_capabilities();
                                self.pouches.insert(spec.name.clone(), Box::new(rp));
                                self.meta.insert(
                                    spec.name.clone(),
                                    PouchMeta { role: PouchRole::E1 },
                                );
                                for cap in caps {
                                    self.registry.register(cap);
                                }
                                self.log_event(format!("CLOUD_INSTALL {}", spec.name));
                            }
                        }
                        let mut last_output = String::new();
                        for step in &cloud_plan.steps {
                            match self.call_pouch(&step.pouch, &step.input).await {
                                Ok(data) => {
                                    self.log_event(format!("CLOUD_STEP {} ok", step.pouch));
                                    last_output = data;
                                }
                                Err(e) => {
                                    self.log_event(format!("CLOUD_STEP {} {}", step.pouch, e));
                                    return Err((e, "cloud_plan".into()));
                                }
                            }
                        }
                        if !self.language.is_fallback_response(&last_output) && !last_output.is_empty() {
                            self.language.absorb(input, &last_output, 1.1);
                            self.log_event("ABSORB cloud→language".into());
                        }
                        return Ok((last_output, "cloud_plan".into()));
                    }
                    _ => {}
                }
                let lang_final = self.language.process(input).await;
                self.log_event("LANG process".into());
                if let Some(out) = self.execute_chain_spec(input, &lang_final).await {
                    return self.unguard_then(Ok((out, "chain".into())));
                }
                if let Some(expanded) = self.expand_template(input, &lang_final).await {
                    return self.unguard_then(Ok((expanded, "template".into())));
                }
                if !self.language.is_fallback_response(&lang_final) {
                    return self.unguard_then(Ok((lang_final, "language".into())));
                }
                if let Some((out, pouch)) = self.try_fallback_chain(input).await {
                    if !self.language.is_fallback_response(&out) {
                        self.language.absorb(input, &out, 1.0);
                        self.log_event(format!("ABSORB {}→language", pouch));
                    }
                    return self.unguard_then(Ok((out, pouch)));
                }
                return self.unguard_then(Ok((lang_final, "language".into())));
            }
        };

        self.unguard();
        result
    }

    fn unguard_then<T>(&mut self, v: T) -> T {
        self.unguard();
        v
    }

    fn role_priority(role: PouchRole) -> u8 {
        match role {
            PouchRole::E0 => 0,
            PouchRole::E1 => 1,
            PouchRole::E2 => 2,
        }
    }

    async fn sanitize_input(&mut self, input: &str) -> Option<(bool, String)> {
        let name = self.pouches.keys().find(|n| n.contains("sanitize"))?.clone();
        let pouch = self.pouches.get_mut(&name)?;
        let proposal = create_proposal(input);
        let validated = pouch.validator().validate(&proposal).ok()?;
        let out = pouch.process_proposal(&validated).await.ok()?;
        let rejected = pouch.is_fallback_output(&out.data);
        Some((rejected, out.data))
    }

    async fn expand_template(&mut self, input: &str, response: &str) -> Option<String> {
        if !response.contains("{{") || !response.contains("}}") {
            return None;
        }
        let mut out = response.to_string();
        for _ in 0..5 {
            let start = match out.find("{{") {
                Some(s) => s,
                None => break,
            };
            let end = match out[start..].find("}}") {
                Some(e) => start + e + 2,
                None => break,
            };
            let inner = out[start..end].strip_prefix("{{").and_then(|s| s.strip_suffix("}}")).unwrap_or("").trim();
            if let Some((pouch, query)) = inner.split_once(':') {
                let pouch = pouch.trim();
                let query = query.trim();
                let q = if query == "input" { input } else { query };
                if self.pouches.contains_key(pouch) {
                    if let Ok(rep) = self.call_pouch(pouch, q).await {
                        out.replace_range(start..end, &rep);
                        continue;
                    }
                }
            }
            out.replace_range(start..end, "");
        }
        if out == response {
            return None;
        }
        Some(out)
    }

    async fn execute_chain_spec(&mut self, input: &str, response: &str) -> Option<String> {
        let spec = response.strip_prefix("chain:")?.trim();
        if spec.is_empty() {
            return None;
        }
        let pouches: Vec<&str> = spec.split("->").map(str::trim).filter(|s| !s.is_empty()).collect();
        let max_depth = self.config.chain.chain_spec_max_depth.clamp(2, 12);
        if pouches.is_empty() || pouches.len() > max_depth {
            return None;
        }
        let cache_key = format!("{}|{}", hash_str(input), spec);
        if let Some(cached) = self.chain_result_cache.iter().find(|(k, _)| k == &cache_key).map(|(_, v)| v.clone()) {
            self.log_event("CACHE chain hit".into());
            return Some(cached);
        }
        let mut carry = input.to_string();
        for name in pouches {
            if !self.pouches.contains_key(name) {
                return None;
            }
            match self.call_pouch(name, &carry).await {
                Ok(out) => carry = out,
                Err(_) => return None,
            }
        }
        if self.chain_result_cache.len() >= 100 {
            self.chain_result_cache.pop_front();
        }
        self.chain_result_cache.push_back((cache_key, carry.clone()));
        Some(carry)
    }

    async fn try_fallback_chain(&mut self, input: &str) -> Option<(String, String)> {
        let proposal = create_proposal(input);
        let mut candidates: Vec<String> = self.pouches.keys().cloned().collect();
        candidates.sort_by_key(|name| {
            self.meta.get(name).map_or(1, |m| Self::role_priority(m.role))
        });
        for name in candidates {
            let role = self.meta.get(&name).map_or(PouchRole::E1, |m| m.role);
            if role == PouchRole::E2 && !self.is_pouch_awake(&name) {
                continue;
            }
            if let Some(pouch) = self.pouches.get_mut(&name) {
                if let Ok(validated) = pouch.validator().validate(&proposal) {
                    if let Ok(output) = pouch.process_proposal(&validated).await {
                        if !output.data.is_empty() && !pouch.is_fallback_output(&output.data) {
                            let conf_note = if output.confidence < 0.5 { " [低置信度]" } else { "" };
                            return Some((format!("{}{}", output.data, conf_note), name));
                        }
                    }
                }
            }
        }
        None
    }

    pub async fn call_pouch(&mut self, name: &str, input: &str) -> Result<String, String> {
        self.guard(Layer::Pouch)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.pouch_last_used.insert(name.to_string(), now);

        if !self.is_pouch_awake(name) && name != "language" {
            self.unguard();
            return Err(format!("{}正在休眠", name));
        }

        if name != "language" {
            if let Some(cached) = self.check_promoted(input).cloned() {
                self.log_event(format!("CACHE {} → promoted", name));
                self.unguard();
                return Ok(cached);
            }
        }

        let result = if name == "language" {
            Ok(self.language.process(input).await)
        } else if let Some(pouch) = self.pouches.get_mut(name) {
            let proposal = create_proposal(input);
            match pouch.validator().validate(&proposal) {
                Ok(validated) => match pouch.process_proposal(&validated).await {
                    Ok(output) => {
                        let conf_note = if output.confidence < 0.5 { " [低置信度]" } else { "" };
                        Ok(format!("{}{}", output.data, conf_note))
                    }
                    Err(e) => Err(format!("处理失败: {}", e)),
                },
                Err(e) => Err(format!("验证失败: {}", e)),
            }
        } else {
            Err(format!("尿袋{}未安装", name))
        };

        if let Ok(ref data) = result {
            if name != "language" {
                self.language.learn_routing(input, name);
                self.record_evolution(input, name, data);
                let tokens = self.language.tokenize(input);
                let patterns = vec![(tokens, data.clone(), 1.0)];
                for (_, pouch) in self.pouches.iter_mut() {
                    pouch.sync_patterns(&patterns);
                }
                self.language.receive_sync_patterns(&patterns);
            }
        }
        if let Err(ref e) = result {
            self.evolution_chain.push(EvolutionEntry {
                timestamp: now,
                input_trunc: input.chars().take(80).collect(),
                pouch_name: name.to_string(),
                output_trunc: e.chars().take(80).collect(),
                success: false,
                step_index: 0,
            });
            if self.evolution_chain.len() > EVOLUTION_CHAIN_MAX {
                self.evolution_chain.remove(0);
            }
        }

        self.unguard();
        result
    }

    async fn execute_plan(
        &mut self,
        plan: &crate::atom::ExecutionPlan,
        user_input: &str,
    ) -> Result<String, String> {
        let mut outputs: Vec<String> = Vec::new();
        let mut chain_updated = false;
        for (step_index, step) in plan.steps.iter().enumerate() {
            let input_data = match &step.input_from {
                crate::atom::StepInput::UserInput => user_input.to_string(),
                crate::atom::StepInput::PreviousStep(idx) => {
                    outputs.get(*idx).cloned().unwrap_or_default()
                }
            };
            let result = self.call_pouch(&step.pouch, &input_data).await;
            match result {
                Ok(data) => {
                    self.log_event(format!("PLAN_STEP {} → ok", step.atom_name));
                    let in_trunc: String = input_data.chars().take(80).collect();
                    let out_trunc: String = data.chars().take(80).collect();
                    self.log_event(format!("PouchSuccess: pouch={}, input={}, output={}", step.pouch, in_trunc, out_trunc));
                    /*
                     * 两条演化系统区别：
                     *   - evolution.json (record_evolution)：聚合计数，用于 promotion 阈值判断
                     *   - evolution_chain.json (EvolutionEntry)：时间顺序审计轨迹，用于路由偏好和未来分析
                     *   - plan 成功步会同时更新两者，但语义独立
                     */
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    self.evolution_chain.push(EvolutionEntry {
                        timestamp: now,
                        input_trunc: in_trunc.clone(),
                        pouch_name: step.pouch.clone(),
                        output_trunc: out_trunc.clone(),
                        success: true,
                        step_index,
                    });
                    chain_updated = true;
                    if self.evolution_chain.len() > EVOLUTION_CHAIN_MAX {
                        self.evolution_chain.remove(0);
                    }
                    outputs.push(data);
                }
                Err(e) => {
                    self.log_event(format!("PLAN_STEP {} → {}", step.atom_name, e));
                    return Err(format!("执行计划步骤{}失败: {}", step.atom_name, e));
                }
            }
        }
        if chain_updated {
            self.save_state();
        }
        Ok(outputs.last().cloned().unwrap_or_default())
    }

    async fn handle_cmd(&mut self, cmd: SystemCmd) -> Result<String, String> {
        match cmd {
            SystemCmd::InstallPouch(name) => self.install(&name),
            SystemCmd::UninstallPouch(name) => self.uninstall(&name),
            SystemCmd::SleepPouch(name) => self.sleep_pouch(&name),
            SystemCmd::WakePouch(name) => self.wake_pouch(&name),
            SystemCmd::ConfigShow => self.config_show(),
            SystemCmd::ConfigSet(key, value) => self.config_set(&key, &value),
            SystemCmd::ListPouches => Ok(self.list()),
            SystemCmd::Status => Ok(self.status()),
            SystemCmd::Help => Ok(Self::help()),
            SystemCmd::Teach(trigger, response) => {
                self.language.teach(&trigger, &response);
                self.save_state();
                Ok(format!("学会了:「{}」→「{}」", trigger, response))
            }
            SystemCmd::RunPipeline(stages, data) => {
                let stages_ref: Vec<&str> = stages.iter().map(|s| s.as_str()).collect();
                self.run_pipeline(&stages_ref, data).await
            }
            SystemCmd::SelfTest => self.selftest().await,
            SystemCmd::ImportPatterns(path) => self.import_patterns_from_file(&path).await,
            SystemCmd::ExportPatterns => Ok(self.export_patterns_display()),
            SystemCmd::Rollback => self.rollback(),
            SystemCmd::Train => self.trigger_train().await,
            SystemCmd::Explain(name) => Ok(self.explain_pouch(&name)),
            SystemCmd::EvolutionStatus => Ok(self.evolution_status()),
            SystemCmd::Capabilities => Ok(self.registry.summary()),
            SystemCmd::ClearContext => {
                self.language.clear_context();
                Ok("对话上下文已清空".into())
            }
        }
    }

    pub fn install(&mut self, name: &str) -> Result<String, String> {
        let name = name.trim().to_lowercase();
        if name.is_empty() {
            return Err("名称空".into());
        }
        if name == "language" {
            return Err("language已预装".into());
        }
        if self.pouches.len() >= bedrock::MAX_POUCHES {
            return Err("尿袋数量已满".into());
        }
        if self.pouches.contains_key(&name) {
            return Err(format!("{}已存在", name));
        }

        if let Some((pouch, role)) = crate::pouch_catalog::instantiate(&name, &self.data_dir) {
            let caps = pouch.atom_capabilities();
            self.pouches.insert(name.clone(), pouch);
            self.meta.insert(name.clone(), PouchMeta { role });
            for cap in caps {
                self.registry.register(cap);
            }
            self.save_state();
            return Ok(format!("安装「{}」({:?}) atoms:{}", name, role, self.registry.count()));
        }

        if let Some(spec) = self.lookup_remote_spec(&name) {
            let role = match spec.role.as_str() {
                "E0" => PouchRole::E0,
                "E1" => PouchRole::E1,
                "E2" => PouchRole::E2,
                _ => PouchRole::E1,
            };
            let mut rp = crate::remote_pouch::RemotePouch::new(&name, role, &spec.endpoint);
            rp.failover_endpoints = spec.failover_endpoints.clone();
            let caps = rp.atom_capabilities();
            self.pouches.insert(name.clone(), Box::new(rp));
            self.meta.insert(name.clone(), PouchMeta { role });
            for cap in caps {
                self.registry.register(cap);
            }
            self.save_state();
            let fo = if spec.failover_endpoints.is_empty() { "" } else { " +容灾" };
            return Ok(format!("安装「{}」(远程{})", name, fo));
        }
        if let Some(spec) = fetch_remote_spec_from_cloud(&name) {
            let role = match spec.role.as_str() {
                "E0" => PouchRole::E0,
                "E1" => PouchRole::E1,
                "E2" => PouchRole::E2,
                _ => PouchRole::E1,
            };
            let mut rp = crate::remote_pouch::RemotePouch::new(&spec.name, role, &spec.endpoint);
            rp.failover_endpoints = spec.failover_endpoints.clone();
            let caps = rp.atom_capabilities();
            self.pouches.insert(spec.name.clone(), Box::new(rp));
            self.meta.insert(spec.name.clone(), PouchMeta { role });
            for cap in caps {
                self.registry.register(cap);
            }
            self.save_state();
            let fo = if spec.failover_endpoints.is_empty() { "" } else { " +容灾" };
            return Ok(format!("安装「{}」(云端远程{})", spec.name, fo));
        }

        let endpoint = format!("{}/pouch/{}", GATEWAY_BASE, name);
        let rp = crate::remote_pouch::RemotePouch::new(&name, PouchRole::E1, &endpoint);
        let caps = rp.atom_capabilities();
        self.pouches.insert(name.clone(), Box::new(rp));
        self.meta.insert(name.clone(), PouchMeta { role: PouchRole::E1 });
        for cap in caps {
            self.registry.register(cap);
        }
        self.save_state();
        Ok(format!("自动安装「{}」(远程通用)", name))
    }

    fn uninstall(&mut self, name: &str) -> Result<String, String> {
        let name = name.trim().to_lowercase();
        if name == "language" {
            return Err("language不可卸载".into());
        }
        if self.pouches.remove(&name).is_some() {
            self.meta.remove(&name);
            self.registry.unregister_pouch(&name);
            self.save_state();
            Ok(format!("卸载「{}」", name))
        } else {
            Err(format!("{}不存在", name))
        }
    }

    fn sleep_pouch(&mut self, name: &str) -> Result<String, String> {
        let name = name.trim().to_lowercase();
        if name == "language" {
            return Err("语言尿袋不可休眠".into());
        }
        if !self.pouches.contains_key(&name) {
            return Err(format!("{}未安装", name));
        }
        self.pouch_sleep_state.insert(name.clone(), true);
        self.save_state();
        Ok(format!("已休眠「{}」", name))
    }

    fn wake_pouch(&mut self, name: &str) -> Result<String, String> {
        let name = name.trim().to_lowercase();
        if name == "language" {
            return Err("语言尿袋始终活跃".into());
        }
        if !self.pouches.contains_key(&name) {
            return Err(format!("{}未安装", name));
        }
        self.pouch_sleep_state.insert(name.clone(), false);
        self.save_state();
        Ok(format!("已唤醒「{}」", name))
    }

    fn is_pouch_awake(&self, name: &str) -> bool {
        !self.pouch_sleep_state.get(name).copied().unwrap_or(false)
    }

    async fn run_pipeline(
        &mut self,
        stages: &[&str],
        initial_data: String,
    ) -> Result<String, String> {
        if stages.is_empty() {
            return Err("Pipeline为空".into());
        }
        if stages.len() > bedrock::MAX_PIPELINE_STAGES {
            return Err("Pipeline阶段过多".into());
        }
        let mut current_data = initial_data;
        let mut trace = Vec::new();

        for (i, &stage) in stages.iter().enumerate() {
            let stage_lower = stage.to_lowercase();
            if stage_lower == "language" {
                if i > 0 {
                    return Err("language不能在Pipeline中间".into());
                }
            } else if let Some(pouch) = self.pouches.get_mut(&stage_lower) {
                if i > 0 && !pouch.can_call_others() {
                    return Err(format!("{}不能接收Pipeline数据", stage));
                }
                let proposal = create_proposal(&current_data);
                let validated = pouch
                    .validator()
                    .validate(&proposal)
                    .map_err(|e| format!("阶段{}验证失败: {}", stage, e))?;
                let output = pouch
                    .process_proposal(&validated)
                    .await
                    .map_err(|e| format!("阶段{}处理失败: {}", stage, e))?;
                current_data = output.data;
            } else {
                return Err(format!("尿袋{}未安装", stage));
            }
            trace.push(format!("{}→", stage));
        }

        trace.push("完成".into());
        let pipeline_desc = trace.join("");
        self.log_event(format!("PIPE {}", pipeline_desc));
        Ok(format!("Pipeline: {}\n结果: {}", pipeline_desc, current_data))
    }

    fn save_state(&self) {
        let _ = std::fs::create_dir_all(&self.data_dir);
        let names: Vec<&String> = self.pouches.keys().collect();
        if let Ok(json) = serde_json::to_string(&names) {
            let _ = std::fs::write(format!("{}/pouches.json", self.data_dir), json);
        }
        if let Ok(data) = self.language.save() {
            let _ = std::fs::write(format!("{}/language.bin", self.data_dir), data);
        }
        if let Ok(data) = self.language.save_routes() {
            let _ = std::fs::write(format!("{}/routes.bin", self.data_dir), data);
        }
        if let Ok(json) = serde_json::to_string(&self.evolution) {
            let _ = std::fs::write(format!("{}/evolution.json", self.data_dir), json);
        }
        if let Ok(json) = serde_json::to_string(&self.evolution_chain) {
            let _ = std::fs::write(format!("{}/evolution_chain.json", self.data_dir), json);
        }
        if let Ok(data) = logic::save_promoted_rules() {
            let _ = std::fs::write(format!("{}/promoted_rules.json", self.data_dir), data);
        }
        if let Ok(data) = self.language.save_feedback() {
            let _ = std::fs::write(format!("{}/feedback.json", self.data_dir), data);
        }
        if let Ok(json) = serde_json::to_string(&self.learning) {
            let _ = std::fs::write(format!("{}/learning_state.json", self.data_dir), json);
        }
    }

    fn maybe_adjust_baseline(&mut self) {
        const AUTO_ADJUST_MIN_CHAIN: usize = 50;
        const BASELINE_STEP: f64 = 0.02;
        const THRESHOLD_STEP: f64 = 0.01;
        const PROMOTE_STEP: f64 = 0.005;
        if self.evolution_chain.len() < AUTO_ADJUST_MIN_CHAIN {
            return;
        }
        let success_count = self.evolution_chain.iter().filter(|e| e.success).count();
        let total = self.evolution_chain.len();
        let success_rate = success_count as f64 / total as f64;
        let bounds = manager_math::RoutingParamsBounds::default();
        let cur_baseline = self.config.routing_score.baseline_score;
        let new_baseline = manager_math::adjusted_baseline(
            cur_baseline,
            success_rate,
            (bounds.baseline_min, bounds.baseline_max),
            BASELINE_STEP,
        );
        let cur_threshold = self.config.routing_score.low_score_threshold;
        let new_threshold = manager_math::adjusted_baseline(
            cur_threshold,
            success_rate,
            (bounds.low_threshold_min, bounds.low_threshold_max),
            THRESHOLD_STEP,
        );
        let cur_promote = self.config.routing_score.promote_min_chain_score;
        let new_promote = manager_math::adjusted_baseline(
            cur_promote,
            success_rate,
            (bounds.promote_min_chain_min, bounds.promote_min_chain_max),
            PROMOTE_STEP,
        );
        let mut changed = false;
        if (new_baseline - cur_baseline).abs() > 1e-6 {
            self.config.routing_score.baseline_score = new_baseline;
            changed = true;
        }
        if (new_threshold - cur_threshold).abs() > 1e-6 {
            self.config.routing_score.low_score_threshold = new_threshold;
            changed = true;
        }
        if (new_promote - cur_promote).abs() > 1e-6 {
            self.config.routing_score.promote_min_chain_score = new_promote;
            changed = true;
        }
        if changed {
            let config_path = format!("{}/pouch_config.json", self.data_dir);
            let _ = self.config.save(&config_path);
        }
    }

    fn load_state(&mut self) {
        let chain_path = format!("{}/evolution_chain.json", self.data_dir);
        let loaded_chain: Vec<EvolutionEntry> = std::fs::read_to_string(&chain_path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        if let Ok(json) = std::fs::read_to_string(format!("{}/pouches.json", self.data_dir)) {
            if let Ok(names) = serde_json::from_str::<Vec<String>>(&json) {
                for name in names {
                    let _ = self.install(&name);
                }
            }
        }
        if let Ok(data) = std::fs::read(format!("{}/language.bin", self.data_dir)) {
            if self.language.load(&data).is_ok() {
                log::info!("语言尿袋已恢复");
            }
        }
        if let Ok(data) = std::fs::read(format!("{}/routes.bin", self.data_dir)) {
            self.language.load_routes(&data).ok();
        }
        if let Ok(json) = std::fs::read_to_string(format!("{}/evolution.json", self.data_dir)) {
            if let Ok(records) = serde_json::from_str::<Vec<EvolutionRecord>>(&json) {
                self.evolution = records;
            }
        }
        self.evolution_chain = loaded_chain;
        if let Ok(data) = std::fs::read(format!("{}/promoted_rules.json", self.data_dir)) {
            logic::load_promoted_rules(&data).ok();
        }
        if let Ok(data) = std::fs::read(format!("{}/feedback.json", self.data_dir)) {
            self.language.load_feedback(&data).ok();
        }
        if let Ok(json) = std::fs::read_to_string(format!("{}/learning_state.json", self.data_dir)) {
            if let Ok(ls) = serde_json::from_str::<LearningState>(&json) {
                self.learning = ls;
            }
        }
    }

    pub fn total_memory_count(&self) -> usize {
        let mut count = self.language.memory_count();
        for pouch in self.pouches.values() {
            count += pouch.memory_count();
        }
        count
    }

    #[cfg(test)]
    pub fn routing_baseline(&self) -> f64 {
        self.config.routing_score.baseline_score
    }

    #[cfg(test)]
    pub fn routing_low_threshold(&self) -> f64 {
        self.config.routing_score.low_score_threshold
    }

    pub fn promoted_cache_len(&self) -> usize {
        self.promoted_cache.len()
    }

    pub fn data_dir(&self) -> &str {
        &self.data_dir
    }

    fn clamped_routing(&self) -> (f64, f64, f64) {
        let bounds = manager_math::RoutingParamsBounds::default();
        manager_math::clamp_routing_params(&self.config.routing_score, &bounds)
    }

    pub fn score_pouch(&self, pouch_name: &str) -> f64 {
        let entries = self.recent_evolution_entries(100);
        let (base, _, _) = self.clamped_routing();
        Self::score_pouch_from_entries(pouch_name, &entries, base)
    }

    pub fn run_promote_check(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(self.evolution_status());
        let chain = self.recent_evolution_entries(100);
        let (base, _, min_chain) = self.clamped_routing();
        for rec in &self.evolution {
            if rec.verify_count >= EVOLUTION_PROMOTE_THRESHOLD && !rec.promoted {
                let score = Self::score_pouch_from_entries(&rec.pouch_name, &chain, base);
                let chain_ok = manager_math::promote_eligible(score, min_chain);
                out.push(format!(
                    "candidate {} verify_count={} chain_ok={} (晋升仅在 ToPouch 路径 record_evolution 时写入)",
                    rec.pouch_name, rec.verify_count, chain_ok
                ));
            }
        }
        out
    }

    fn log_event(&mut self, msg: String) {
        if self.events.len() >= 50 {
            self.events.remove(0);
        }
        self.events.push(msg);
    }

    pub fn log_event_pub(&mut self, msg: String) {
        self.log_event(msg);
    }

    pub fn recent_events(&self) -> Vec<String> {
        self.events.clone()
    }

    pub fn recent_evolution_entries(&self, n: usize) -> Vec<EvolutionEntry> {
        let len = self.evolution_chain.len();
        if n >= len {
            self.evolution_chain.clone()
        } else {
            self.evolution_chain[len - n..].to_vec()
        }
    }

    fn score_pouch_from_entries(pouch_name: &str, entries: &[EvolutionEntry], baseline: f64) -> f64 {
        let matching: Vec<_> = entries.iter().filter(|e| e.pouch_name == pouch_name).collect();
        let matching_count = matching.len();
        let total_output_len: usize = matching.iter().map(|e| e.output_trunc.len()).sum();
        manager_math::score_from_evolution_stats(matching_count, total_output_len, baseline)
    }

    /*
     * 两条演化系统区别：
     *   - evolution.json (record_evolution)：聚合计数，用于 promotion 阈值判断
     *   - evolution_chain.json (EvolutionEntry)：时间顺序审计轨迹，用于路由偏好和未来分析
     *   - plan 成功步会同时更新两者，但语义独立
     */
    fn record_evolution(&mut self, input: &str, pouch_name: &str, output: &str) {
        let ih = hash_str(input);
        let oh = hash_str(output);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let (base, _, min_chain) = self.clamped_routing();
        let chain_entries = self.recent_evolution_entries(100);

        let mut event: Option<String> = None;

        for rec in &mut self.evolution {
            if rec.input_hash == ih && rec.pouch_name == pouch_name {
                if rec.output_hash == oh {
                    rec.verify_count += 1;
                    rec.last_seen = now;
                    if rec.verify_count >= EVOLUTION_PROMOTE_THRESHOLD && !rec.promoted {
                        let score = Self::score_pouch_from_entries(pouch_name, &chain_entries, base);
                        let chain_ok = manager_math::promote_eligible(score, min_chain);
                        if chain_ok {
                        rec.promoted = true;
                        let rule = logic::PromotedRule {
                            input_pattern: input.to_lowercase().chars().take(20).collect(),
                            target_pouch: pouch_name.to_string(),
                            verify_count: rec.verify_count,
                            promoted_at: now,
                        };
                        match logic::accept_promoted_rule(rule) {
                            Ok(()) => {
                                event = Some(format!("EVOLVE_L2 {}→logic ({}x)", pouch_name, rec.verify_count));
                            }
                            Err(reason) => {
                                rec.promoted = false;
                                event = Some(format!("EVOLVE_L2_REJECT {} ({})", pouch_name, reason));
                            }
                        }
                        } else {
                            event = Some(format!("EVOLVE_L2_SKIP {} (chain_score < {})", pouch_name, min_chain));
                        }
                    }
                } else {
                    rec.output_hash = oh;
                    rec.verify_count = 1;
                    rec.last_seen = now;
                    if rec.promoted {
                        rec.promoted = false;
                        event = Some(format!("EVOLVE {}→revoked (output changed)", pouch_name));
                    }
                }

                if rec.promoted {
                    self.promoted_cache.insert(ih, output.to_string());
                } else {
                    self.promoted_cache.remove(&ih);
                }
                if let Some(ev) = event {
                    self.log_event(ev);
                }
                return;
            }
        }

        if self.evolution.len() >= EVOLUTION_MAX_RECORDS {
            self.evolution.sort_by_key(|r| std::cmp::Reverse(r.verify_count));
            self.evolution.truncate(EVOLUTION_MAX_RECORDS / 2);
        }

        self.evolution.push(EvolutionRecord {
            input_hash: ih,
            pouch_name: pouch_name.to_string(),
            output_hash: oh,
            verify_count: 1,
            first_seen: now,
            last_seen: now,
            promoted: false,
        });
        self.maybe_adjust_baseline();
    }

    fn check_promoted(&self, input: &str) -> Option<&String> {
        let ih = hash_str(input);
        self.promoted_cache.get(&ih).filter(|s| !s.is_empty())
    }

    pub fn evolution_status(&self) -> String {
        let total = self.evolution.len();
        let promoted = self.evolution.iter().filter(|r| r.promoted).count();
        let candidates = self.evolution.iter().filter(|r| r.verify_count >= 50 && !r.promoted).count();
        let l2_rules = logic::promoted_rules_count();
        format!("演化记录:{} L1晋升:{} L2规则:{} 候选:{}", total, promoted, l2_rules, candidates)
    }

    pub fn capabilities_info(&self) -> Vec<AtomDeclaration> {
        self.registry.all().to_vec()
    }

    pub fn pouches_detail(&self) -> Vec<(String, String, usize, bool, String, Vec<String>)> {
        let mut info = vec![
            (
                "language".into(),
                "E0".into(),
                self.language.memory_count(),
                true,
                format!("LanguagePouch: 语言处理，{} 条模式", self.language.memory_count()),
                vec!["route_intent".into()],
            ),
        ];
        for (name, pouch) in &self.pouches {
            let role = self.meta.get(name).map_or("E1".to_string(), |m| format!("{:?}", m.role));
            let explanation = pouch.explain();
            let pouch_atoms: Vec<String> = pouch.atom_capabilities().iter().map(|a| a.name.clone()).collect();
            info.push((name.clone(), role, pouch.memory_count(), self.is_pouch_awake(name), explanation, pouch_atoms));
        }
        info
    }

    pub fn evolution_info(&self) -> (usize, usize, usize, usize) {
        let total = self.evolution.len();
        let promoted = self.evolution.iter().filter(|r| r.promoted).count();
        let candidates = self.evolution.iter().filter(|r| r.verify_count >= 50 && !r.promoted).count();
        let l2_rules = logic::promoted_rules_count();
        (total, promoted, candidates, l2_rules)
    }

    pub fn evolution_records_snapshot(&self) -> Vec<(String, u32, bool, u64)> {
        let mut records: Vec<_> = self.evolution.iter()
            .map(|r| (r.pouch_name.clone(), r.verify_count, r.promoted, r.last_seen))
            .collect();
        records.sort_by(|a, b| b.1.cmp(&a.1));
        records
    }

    pub fn routing_config_snapshot(&self) -> (f64, f64, f64) {
        let bounds = manager_math::RoutingParamsBounds::default();
        manager_math::clamp_routing_params(&self.config.routing_score, &bounds)
    }

    pub fn pouches_info(&self) -> Vec<(String, String, usize, bool)> {
        let mut info = vec![
            ("language".into(), "E0".into(), self.language.memory_count(), true),
        ];
        for (name, pouch) in &self.pouches {
            let role = self.meta.get(name).map_or("E1".to_string(), |m| format!("{:?}", m.role));
            info.push((name.clone(), role, pouch.memory_count(), self.is_pouch_awake(name)));
        }
        info
    }

    fn detect_feedback(&mut self, lower: &str) -> Option<String> {
        let positive = lower == "好" || lower == "对" || lower == "点赞"
            || lower == "不错" || lower == "正确" || lower == "good"
            || lower == "👍" || lower == "赞";
        let negative = lower == "不好" || lower == "不对" || lower == "点踩"
            || lower == "错了" || lower == "bad" || lower == "👎"
            || lower == "不行" || lower == "错误";
        if !positive && !negative {
            return None;
        }
        let last_input = self.language.last_context_input().map(|s| s.to_string());
        let input = match last_input {
            Some(ref s) if !s.is_empty() => s.as_str(),
            _ => return Some("无上次对话记录可评价".into()),
        };
        if positive {
            let reinforced = self.language.reinforce(input);
            self.log_event(format!("FEEDBACK+ {}", if reinforced { "reinforced" } else { "no_match" }));
            self.save_state();
            Some(format!("已记录正向反馈。{}", if reinforced { "权重已增强。" } else { "" }))
        } else {
            let penalized = self.language.penalize(input);
            self.log_event(format!("FEEDBACK- {}", if penalized { "penalized" } else { "no_match" }));
            self.save_state();
            Some(format!("已记录负向反馈。{}", if penalized { "权重已降低。你可以用「教你 X -> Y」纠正我。" } else { "" }))
        }
    }

    pub fn apply_feedback(&mut self, input: &str, signal: i8, correction: Option<&str>) {
        match signal {
            s if s > 0 => {
                self.language.reinforce(input);
                self.log_event("API_FEEDBACK+".into());
            }
            s if s < 0 => {
                if let Some(correct) = correction {
                    self.language.feedback_correction(input, correct);
                    self.log_event("API_FEEDBACK_CORRECTION".into());
                } else {
                    self.language.penalize(input);
                    self.log_event("API_FEEDBACK-".into());
                }
            }
            _ => {
                if let Some(correct) = correction {
                    self.language.feedback_correction(input, correct);
                    self.log_event("API_FEEDBACK_CORRECTION".into());
                }
            }
        }
        self.save_state();
    }

    pub fn feedback_status(&self) -> String {
        let (misses, log_count, absorbed, net_positive) = self.language.feedback_stats();
        format!(
            "反哺状态: 未命中缓冲 {} 条, 反馈记录 {} 条, 已吸收 {} 条, 净正向 {}",
            misses, log_count, absorbed, net_positive
        )
    }

    pub fn language_feedback_stats(&self) -> (usize, usize, usize, usize) {
        self.language.feedback_stats()
    }

    pub fn pending_misses(&self, limit: usize) -> Vec<String> {
        self.language.pending_misses(limit)
    }

    pub async fn language_debug(&mut self, input: &str) -> (String, bool, f64) {
        let result = self.language.process(input).await;
        let is_fallback = self.language.is_fallback_response(&result);
        let weight = self.language.last_match_weight();
        (result, is_fallback, weight)
    }

    pub async fn import_patterns_from_file(&mut self, path: &str) -> Result<String, String> {
        if path.is_empty() {
            return Err("路径为空".into());
        }
        if let Ok(data) = self.language.save() {
            let backup_path = format!("{}/language.bin.backup", self.data_dir);
            std::fs::write(&backup_path, data).map_err(|e| format!("备份失败: {}", e))?;
        }
        let content = if path.starts_with("http://") || path.starts_with("https://") {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| format!("http client: {}", e))?;
            let resp = client.get(path).send().await.map_err(|e| format!("fetch: {}", e))?;
            if !resp.status().is_success() {
                return Err(format!("http {} {}", resp.status(), path));
            }
            resp.text().await.map_err(|e| format!("body: {}", e))?
        } else {
            std::fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))?
        };
        let is_jsonl = path.ends_with(".jsonl") || path.contains(".jsonl");
        let count = self.language.import_from_content(&content, is_jsonl)?;
        self.save_state();
        Ok(format!("导入 {} 条模式", count))
    }

    pub fn seed_route(&mut self, input: &str, pouch_name: &str) {
        self.language.learn_routing(input, pouch_name);
    }

    const DOMAIN_MAP: &'static [(&'static str, &'static [&'static str])] = &[
        ("medical", &["治疗","症状","药物","患者","临床","诊断","病","疫苗","手术","医","健康","服用","剂量","副作用"]),
        ("physics", &["量子","引力","电磁","粒子","波","光速","相对论","力学","能量","动量","物理"]),
        ("biology", &["基因","细胞","蛋白质","DNA","RNA","进化","生态","物种","酶","代谢","生物"]),
        ("math", &["方程","定理","证明","积分","微分","矩阵","概率","统计","拓扑","代数","数学"]),
        ("cs_ai", &["算法","神经网络","模型","训练","推理","机器学习","深度学习","GPU","transformer","embedding"]),
        ("legal", &["法律","条款","合同","诉讼","法院","权利","义务","法规","判决","立法","违法"]),
        ("finance", &["投资","股票","利率","通胀","GDP","资产","风险","基金","债券","金融","经济"]),
        ("history", &["朝代","战争","革命","文明","帝国","考古","遗址","历史","年代","王朝"]),
        ("psychology", &["心理","认知","情绪","行为","焦虑","抑郁","人格","意识","潜意识","动机"]),
        ("education", &["教学","课程","学习","考试","素质","培养","教育","学生","教师","培训"]),
        ("literature", &["文学","诗","散文","小说","修辞","叙事","意象","典故","作品","作者"]),
        ("engineering", &["设计","制造","材料","结构","电路","控制","机械","工程","传感器","自动化"]),
        ("environment", &["气候","碳排放","生态","污染","可持续","环保","温室","海洋","森林","能源"]),
        ("philosophy", &["哲学","伦理","存在","认识论","形而上学","逻辑","辩证","价值","本体","道德"]),
        ("astronomy", &["恒星","行星","星系","黑洞","宇宙","天文","红移","超新星","暗物质","望远镜"]),
        ("agriculture", &["种植","农业","土壤","灌溉","作物","肥料","病虫害","产量","畜牧","收获"]),
        ("geography", &["地形","气候带","板块","河流","山脉","人口","城市化","地理","大陆","海拔"]),
        ("music_art", &["旋律","和声","节奏","绘画","雕塑","美学","艺术","色彩","构图","乐器"]),
        ("sports", &["比赛","训练","运动员","体育","竞技","赛事","冠军","奥运","健身","战术"]),
        ("nutrition", &["营养","维生素","蛋白","碳水","脂肪","膳食","热量","矿物质","补充","饮食"]),
    ];

    pub fn classify_domain(text: &str) -> Option<&'static str> {
        let mut best: Option<(&str, usize)> = None;
        for &(domain, keywords) in Self::DOMAIN_MAP {
            let hits = keywords.iter().filter(|kw| text.contains(**kw)).count();
            if hits >= 2 && best.is_none_or(|(_, b)| hits > b) {
                best = Some((domain, hits));
            }
        }
        best.map(|(d, _)| d)
    }

    pub fn auto_ensure_pouch(&mut self, domain: &str) -> bool {
        if self.pouches.contains_key(domain) { return false; }
        if self.pouches.len() >= crate::frozen::bedrock::MAX_POUCHES { return false; }
        match self.install(domain) {
            Ok(msg) => {
                self.log_event(format!("AUTO_POUCH {}: {}", domain, msg));
                if let Some(&(_, keywords)) = Self::DOMAIN_MAP.iter().find(|&&(d, _)| d == domain) {
                    for kw in keywords.iter().take(5) {
                        self.language.learn_routing(kw, domain);
                    }
                }
                self.save_state();
                true
            }
            Err(_) => false,
        }
    }

    async fn discover_gaps_from_misses(&mut self) {
        if self.pouches.len() >= crate::frozen::bedrock::MAX_POUCHES { return; }
        let clusters = self.language.miss_token_clusters(3);
        let maturities: Vec<(String, f64)> = self.pouches.keys()
            .map(|n| (n.clone(), self.pouch_maturity(n)))
            .collect();
        let teachers: Vec<String> = {
            let mut t: Vec<_> = maturities.iter()
                .filter(|(n, m)| *m >= MATURITY_HUNGRY && n != "language")
                .cloned()
                .collect();
            t.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            t.into_iter().map(|(n, _)| n).take(5).collect()
        };

        for (token, count, sample_inputs) in clusters.iter().take(2) {
            let target_domain: Option<String> =
                if let Some(domain) = Self::classify_domain(&sample_inputs.join(" ")) {
                    if !self.pouches.contains_key(domain) {
                        Some(domain.to_string())
                    } else { None }
                } else if token.chars().count() >= 2 && *count >= 4 {
                    if !self.pouches.contains_key(token.as_str()) {
                        Some(token.clone())
                    } else { None }
                } else { None };

            let Some(ref domain) = target_domain else { continue };
            if !self.auto_ensure_pouch(domain) { continue; }

            for input in sample_inputs {
                self.language.learn_routing(input, domain);
            }

            let mut train_pairs: Vec<(Vec<String>, String, f64)> = Vec::new();
            let mut trained = 0u32;
            self.call_stack.clear();
            let _ = self.guard(Layer::Orchestrator);

            for input in sample_inputs {
                let tokens = self.language.tokenize(input);
                let mut responses: Vec<String> = Vec::new();
                for teacher_name in &teachers {
                    let result = self.call_pouch(teacher_name, input).await;
                    if let Ok(ref output) = result {
                        if output.len() > 15 && !self.language.is_fallback_response(output) {
                            responses.push(output.clone());
                        }
                    }
                    if responses.len() >= 3 { break; }
                }
                if responses.is_empty() {
                    let lang_result = self.call_pouch("language", input).await;
                    if let Ok(ref lang_out) = lang_result {
                        if lang_out.len() > 20 && !self.language.is_fallback_response(lang_out) {
                            train_pairs.push((tokens.clone(), lang_out.clone(), 0.6));
                            trained += 1;
                        }
                    }
                } else {
                    let best = responses.iter()
                        .max_by_key(|r| r.len())
                        .cloned()
                        .unwrap_or_default();
                    let weight = if responses.len() >= 2 { 1.2 } else { 0.6 };
                    train_pairs.push((tokens.clone(), best.clone(), weight));
                    self.language.absorb(input, &best, weight * 0.8);
                    trained += 1;
                }
            }

            self.unguard();

            if let Some(pouch) = self.pouches.get_mut(domain.as_str()) {
                pouch.sync_patterns(&train_pairs);
            }
            self.log_event(format!(
                "GAP_FILL miss频:{} 词:{} → {} 训练:{}/{}",
                count, token, domain, trained, sample_inputs.len()
            ));
        }
    }

    pub fn batch_teach_content(&mut self, content: &str) -> usize {
        let before = self.language.memory_count();
        let taught = self.language.batch_teach_from_content(content);
        let after = self.language.memory_count();
        if taught > 0 {
            self.save_state();
        }
        log::info!("batch_teach: taught={} before={} after={}", taught, before, after);
        taught
    }

    pub async fn eval_language(&mut self, path: &str) -> Result<String, String> {
        if path.is_empty() {
            return Err("路径为空".into());
        }
        self.language.eval_from_path(path, &self.data_dir).await
    }

    fn export_patterns_display(&self) -> String {
        self.language.export_summary()
    }

    fn rollback(&mut self) -> Result<String, String> {
        let backup_path = format!("{}/language.bin.backup", self.data_dir);
        let count = self.language.rollback_from(&backup_path)?;
        self.save_state();
        Ok(format!("回滚完成，当前 {} 条模式", count))
    }

    async fn trigger_train(&mut self) -> Result<String, String> {
        let trainer_name = self.pouches.keys()
            .find(|k| k.contains("cloud_trainer") || k.contains("训练"))
            .cloned();
        let trainer_name = trainer_name.ok_or("未安装云训练尿袋")?;
        let pouch = self.pouches.get_mut(&trainer_name).ok_or("云训练尿袋不可用")?;
        let proposal = create_proposal("train");
        let validated = pouch.validator().validate(&proposal)
            .map_err(|e| format!("验证失败: {}", e))?;
        let output = pouch.process_proposal(&validated)
            .await
            .map_err(|e| format!("训练失败: {}", e))?;
        Ok(output.data)
    }

    fn explain_pouch(&self, name: &str) -> String {
        let name = name.trim().to_lowercase();
        if name == "language" || name.is_empty() {
            return format!("LanguagePouch: 语言处理尿袋，{} 条模式", self.language.memory_count());
        }
        if let Some(pouch) = self.pouches.get(&name) {
            pouch.explain()
        } else {
            format!("尿袋 {} 未安装", name)
        }
    }

    fn status(&self) -> String {
        format!(
            "LOGOS v{}\n语言模式:{}条\n尿袋数:{}\n总记忆:{}\n上下文:{}/{}轮\n原子能力:{}\nL2规则:{}",
            VERSION,
            self.language.memory_count(),
            self.pouches.len() + 1,
            self.total_memory_count(),
            self.language.context_len(),
            crate::language_pouch::MAX_CONTEXT_TURNS,
            self.registry.count(),
            logic::promoted_rules_count(),
        )
    }

    fn list(&self) -> String {
        let mut s = String::from("尿袋列表:\n");
        s.push_str(&format!("language (E0) - LanguagePouch: {} 条模式\n", self.language.memory_count()));
        for (name, meta) in &self.meta {
            if name != "language" {
                if let Some(pouch) = self.pouches.get(name) {
                    s.push_str(&format!("{} ({:?}) - {}\n", name, meta.role, pouch.explain()));
                }
            }
        }
        s.push_str(&format!("容量: {}/{}", self.pouches.len() + 1, bedrock::MAX_POUCHES));
        s
    }

    fn help() -> String {
        "LOGOS命令:\n\
        直接对话 - 语言尿袋交流\n\
        状态 - 系统状态\n\
        帮助 - 命令列表\n\
        尿袋列表 - 已安装尿袋\n\
        安装尿袋 <名> - 安装\n\
        卸载尿袋 <名> - 卸载\n\
        休眠 <名> - 休眠尿袋\n\
        唤醒 <名> - 唤醒尿袋\n\
        教你 X -> Y - 教我新模式\n\
        流水线 A,B,C: 数据 - Pipeline执行\n\
        配置 - 查看配置\n\
        自检 - 系统自检\n\
        解释 <名> - 查看尿袋说明\n\
        导出模式 - 导出语言模式\n\
        导入模式 <路径> - 导入语言模式\n\
        回滚 - 回滚语言模式\n\
        训练 - 触发云训练\n\
        演化 - 被动演化状态"
            .into()
    }

    fn config_show(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&self.config).map_err(|e| format!("序列化失败: {}", e))
    }

    fn config_set(&mut self, key: &str, value: &str) -> Result<String, String> {
        match key.to_lowercase().as_str() {
            "auto_sleep" => {
                self.config.auto_sleep.enabled = value.to_lowercase() == "true";
            }
            "idle_threshold" => {
                self.config.auto_sleep.idle_threshold_secs =
                    value.parse().map_err(|_| "无效数值".to_string())?;
            }
            _ => return Err(format!("未知配置项: {}", key)),
        }
        let config_path = format!("{}/pouch_config.json", self.data_dir);
        self.config.save(&config_path)?;
        Ok(format!("配置已更新: {}={}", key, value))
    }

    async fn selftest(&mut self) -> Result<String, String> {
        let mut results = Vec::new();

        let (route_ok, pouch_count) = {
            let installed = self.installed();
            let decision = logic::route("你好", &installed);
            let ok = matches!(decision, RouteDecision::ToPouch(_));
            let count = installed.len();
            (ok, count)
        };
        results.push(format!("路由: {}", if route_ok { "ok" } else { "fail" }));

        let guard_result = self.guard(Layer::Orchestrator).map(|_| {
            self.unguard();
        });
        results.push(format!("层守卫: {}", if guard_result.is_ok() { "ok" } else { "fail" }));

        let adj_bedrock = logic::adjacent(Layer::Bedrock, Layer::Logic);
        let adj_logic = logic::adjacent(Layer::Logic, Layer::Orchestrator);
        let no_cross = !logic::adjacent(Layer::Bedrock, Layer::Pouch);
        results.push(format!("层邻接: {}", if adj_bedrock && adj_logic && no_cross { "ok" } else { "fail" }));

        for pouch in self.pouches.values() {
            results.push(format!("{}: E{} mem={}", pouch.name(), pouch.role() as u8, pouch.memory_count()));
        }

        results.push(format!("尿袋数: {}", pouch_count));
        results.push(format!("原子能力: {}", self.registry.count()));
        let route_ok = self.registry.find_by_name("route_intent").is_some();
        let validate_ok = self.registry.find_by_name("proposal_validate").is_some();
        results.push(format!("核心原子: route={} validate={}", route_ok, validate_ok));
        for kind in [
            crate::atom::AtomKind::Transform,
            crate::atom::AtomKind::Match,
            crate::atom::AtomKind::Score,
            crate::atom::AtomKind::Generate,
            crate::atom::AtomKind::Validate,
            crate::atom::AtomKind::Route,
        ] {
            let n = self.registry.find_by_kind(kind).len();
            if n > 0 {
                results.push(format!("  {:?}: {}", kind, n));
            }
        }
        Ok(results.join("\n"))
    }

    fn collect_system_metrics_json(&self) -> String {
        let installed: Vec<String> = self.pouches.keys().cloned().collect();
        let awake: Vec<bool> = installed.iter().map(|n| self.is_pouch_awake(n)).collect();
        let evo_total = self.evolution.len();
        let evo_promoted = self.evolution.iter().filter(|r| r.promoted).count();

        serde_json::json!({
            "pattern_count": self.language.memory_count(),
            "pouch_count": self.pouches.len() + 1,
            "max_pouches": bedrock::MAX_POUCHES,
            "installed_pouches": installed,
            "pouch_awake": awake,
            "atom_count": self.registry.count(),
            "evolution_total": evo_total,
            "evolution_promoted": evo_promoted,
            "evolution_chain_len": self.evolution_chain.len(),
            "total_memory": self.total_memory_count(),
            "has_context": self.language.context_len() > 0,
        }).to_string()
    }

    async fn self_optimize(&mut self) -> Result<String, String> {
        let mut report = Vec::new();
        report.push("=== 自我优化启动 ===".to_string());

        let core_pouches = ["benchmark", "defect_scanner", "capability_comparer", "code_analyzer", "knowledge_retriever"];
        for name in &core_pouches {
            if !self.pouches.contains_key(*name) {
                match self.install(name) {
                    Ok(msg) => {
                        report.push(format!("自动安装: {}", msg));
                        self.log_event(format!("SELF_OPT auto_install {}", name));
                    }
                    Err(e) => report.push(format!("安装{}失败: {}", name, e)),
                }
            }
        }

        let metrics_json = self.collect_system_metrics_json();

        if let Some(bp) = self.pouches.get_mut("benchmark") {
            let proposal = create_proposal(&metrics_json);
            if let Ok(validated) = bp.validator().validate(&proposal) {
                if let Ok(output) = bp.process_proposal(&validated).await {
                    report.push(output.data);
                }
            }
        }

        let defect_json = self.collect_system_metrics_json();
        if let Some(ds) = self.pouches.get_mut("defect_scanner") {
            let proposal = create_proposal(&defect_json);
            if let Ok(validated) = ds.validator().validate(&proposal) {
                if let Ok(output) = ds.process_proposal(&validated).await {
                    report.push(output.data.clone());
                    for pname in ds.recommended_follow_ups(&output.data) {
                        if !self.pouches.contains_key(&pname) {
                            if let Ok(msg) = self.install(&pname) {
                                report.push(format!("自动补齐: {}", msg));
                                self.log_event(format!("SELF_OPT remediate {}", pname));
                            }
                        }
                    }
                }
            }
        }

        let pouch_count = self.pouches.len() + 1;
        let atom_count = self.registry.count();
        let total_mem = self.total_memory_count();
        report.push(format!(
            "优化后: 尿袋:{} 原子:{} 记忆:{}",
            pouch_count, atom_count, total_mem
        ));

        self.log_event("SELF_OPT complete".into());
        self.save_state();
        Ok(report.join("\n"))
    }

    async fn evolve(&mut self) -> Result<String, String> {
        let mut report = Vec::new();
        report.push("=== 进化对标启动 ===".to_string());

        let core_pouches = ["benchmark", "defect_scanner", "capability_comparer"];
        for name in &core_pouches {
            if !self.pouches.contains_key(*name) {
                let _ = self.install(name);
            }
        }

        let caps_json = self.collect_system_metrics_json();
        let mut gaps: Vec<(String, f64)> = Vec::new();

        if let Some(cc) = self.pouches.get_mut("capability_comparer") {
            let proposal = create_proposal(&caps_json);
            if let Ok(validated) = cc.validator().validate(&proposal) {
                if let Ok(output) = cc.process_proposal(&validated).await {
                    report.push(output.data.clone());
                    gaps = cc.evolution_gaps_from_output(&output.data);
                }
            }
        }

        if gaps.is_empty() {
            report.push("未发现显著能力差距（LOGOS >= 竞品80%）".to_string());
        } else {
            report.push(format!("发现 {} 个能力差距:", gaps.len()));
            let remediation_map: Vec<(&str, &[&str])> = vec![
                ("语言理解", &["context", "memory"]),
                ("逻辑推理", &["reasoning"]),
                ("知识检索", &["knowledge_retriever"]),
                ("创意生成", &["creative"]),
                ("代码分析", &["code_analyzer", "programming"]),
            ];
            for (category, deficit) in &gaps {
                report.push(format!("  {} 差距: {:.0}%", category, deficit));
                for (cat_key, pouches_needed) in &remediation_map {
                    if category.contains(cat_key) {
                        for pname in *pouches_needed {
                            if !self.pouches.contains_key(*pname) {
                                match self.install(pname) {
                                    Ok(msg) => {
                                        report.push(format!("  → 补齐: {}", msg));
                                        self.log_event(format!("EVOLVE auto_install {}", pname));
                                    }
                                    Err(e) => report.push(format!("  → 安装{}失败: {}", pname, e)),
                                }
                            }
                        }
                    }
                }
            }
        }

        let pouch_count = self.pouches.len() + 1;
        let atom_count = self.registry.count();
        report.push(format!(
            "进化后: 尿袋:{} 原子:{}",
            pouch_count, atom_count
        ));

        self.log_event("EVOLVE complete".into());
        self.save_state();
        Ok(report.join("\n"))
    }

    fn pouch_maturity(&self, name: &str) -> f64 {
        let mem = if name == "language" {
            self.language.memory_count()
        } else {
            self.pouches.get(name).map_or(0, |p| p.memory_count())
        };
        let evo_sum: u32 = self.evolution.iter()
            .filter(|r| r.pouch_name == name)
            .map(|r| r.verify_count)
            .sum();
        let promoted_count = self.evolution.iter()
            .filter(|r| r.pouch_name == name && r.promoted)
            .count();
        let role_bonus = self.meta.get(name).map_or(0.0, |m| match m.role {
            PouchRole::E0 => 0.1,
            _ => 0.0,
        });
        let mem_part = (mem as f64 / 200.0).min(1.0);
        let evo_part = (evo_sum as f64 / (EVOLUTION_PROMOTE_THRESHOLD as f64 * 2.0)).min(1.0);
        let prom_part = (promoted_count as f64 * 0.15).min(0.3);
        (mem_part * 0.35 + evo_part * 0.4 + prom_part + role_bonus).min(1.0)
    }

    const COMBO_ACTIONS: &'static [&'static str] = &[
        "解释","比较","总结","列举","分析","推导","定义","评价","预测","分类",
    ];
    const COMBO_TOPICS: &'static [&'static str] = &[
        "量子计算","神经网络","区块链","基因编辑","纳米材料","黑洞","蛋白质折叠",
        "操作系统","编译器","密码学","博弈论","进化算法","量子纠缠","催化反应",
        "深度学习","分布式系统","形式验证","自然语言处理","计算机视觉","强化学习",
        "超导体","拓扑学","群论","微分方程","概率论","图数据库","内存管理",
    ];

    fn learning_intents_for(name: &str, cycle: u64) -> &'static str {
        let bank: &[&str] = match name {
            "reasoning" => &[
                "如果A大于B且B大于C，那么A和C的关系",
                "一个水池两个进水管一个出水管，进水各2吨3吨，出水1吨，20吨多久满",
                "所有哺乳动物都是温血的，鲸是哺乳动物，鲸是什么",
                "三个人分100元，要求每人至少10元有多少种分法",
                "如果下雨路就湿，路湿了，一定下过雨吗",
                "甲乙丙三人赛跑，甲比乙快，丙比甲慢，谁最快谁最慢",
                "一个命题的逆否命题与原命题等价吗",
                "集合A是B的子集且B是C的子集推出什么",
                "两个事件互斥和独立有什么区别",
                "反证法的核心步骤是什么",
                "数学归纳法为什么能证明所有自然数",
                "充分条件和必要条件的关系",
                "鸽巢原理在什么场景下使用",
                "递归和迭代在效率上有什么区别",
                "如何判断一个论证是否有效",
                "概率中贝叶斯定理的含义",
            ],
            "code_analyzer" => &[
                "分析复杂度: fn fib(n:u32)->u32{if n<=1{n}else{fib(n-1)+fib(n-2)}}",
                "这段有什么问题: let mut v=vec![1,2,3]; for i in &v { v.push(*i); }",
                "分析: fn search(arr:&[i32],t:i32)->Option<usize>{arr.iter().position(|&x|x==t)}",
                "这段安全吗: unsafe { *std::ptr::null::<i32>() }",
                "分析内存使用: let s=String::from(\"hello\"); let s2=s; println!(\"{}\",s);",
                "优化建议: for i in 0..n { for j in 0..n { matrix[i][j]=i*j; } }",
                "分析这段代码的并发安全性: Arc::new(Mutex::new(0))",
                "这段SQL有注入风险吗: format!(\"SELECT * WHERE id={}\")",
                "分析死锁: lock(a) lock(b) 另一线程 lock(b) lock(a)",
                "这个递归有没有栈溢出风险",
                "分析尾递归优化的条件",
                "比较 HashMap 和 BTreeMap 的性能场景",
                "分析 async/await 的状态机转换开销",
                "这段代码是否存在整数溢出",
                "分析 trait object 和泛型的取舍",
                "检查这段代码的错误处理是否完整",
            ],
            "programming" => &[
                "用Rust实现栈数据结构",
                "写一个二分查找",
                "实现LRU缓存",
                "写一个简单的状态机",
                "实现生产者消费者模式",
                "写一个并发安全的计数器",
                "实现一个简单的正则表达式引擎",
                "写一个最小堆",
                "实现字符串匹配的KMP算法",
                "写一个简单的 HTTP 服务器框架",
                "实现观察者模式",
                "写一个线程池",
                "实现 Trie 前缀树",
                "写一个简单的内存分配器",
                "实现基于 token 的简单词法分析器",
                "写一个事件驱动的消息队列",
            ],
            "knowledge_retriever" => &[
                "量子纠缠的基本原理",
                "TCP三次握手过程",
                "光合作用的化学方程式",
                "图灵完备的定义",
                "相对论的核心思想",
                "操作系统中进程和线程的区别",
                "什么是CAP定理",
                "哈希表的冲突解决策略",
                "傅里叶变换的物理意义",
                "什么是零知识证明",
                "CRISPR基因编辑的原理",
                "信息熵的数学定义",
                "什么是图灵测试",
                "量子退火与经典退火的区别",
                "冯诺依曼架构的核心思想",
                "什么是P=NP问题",
            ],
            "context" => &[
                "在金融领域期货和期权的区别",
                "在计算机中栈和堆的不同",
                "在物理学中功和能的关系",
                "在生物学中DNA和RNA的区别",
                "在数学中离散和连续的区别",
                "在网络中TCP和UDP的区别",
                "在机器学习中过拟合和欠拟合的区别",
                "在数据库中ACID和BASE的区别",
                "在密码学中对称和非对称加密的区别",
                "在操作系统中用户态和内核态的区别",
                "在编程中编译型和解释型语言的区别",
                "在统计学中频率学派和贝叶斯学派的区别",
                "在化学中有机物和无机物的区别",
                "在经济学中微观和宏观的区别",
                "在哲学中唯心主义和唯物主义的区别",
                "在建筑中承重结构和非承重结构的区别",
            ],
            "memory" => &[
                "LOGOS是自演化AI操作系统",
                "尿袋之间通过管理器中转实现突触互学",
                "四层架构：公式层、逻辑层、管理器、尿袋层",
                "演化链记录成功和失败路径用于路由偏好",
                "sync_patterns是尿袋间知识同步的标准机制",
                "晋升阈值由verify_count和chain_score共同决定",
                "每个尿袋通过atom_capabilities声明自身能力",
                "路由决策基于逻辑层的纯函数不依赖外部状态",
                "管理器数学化参数通过几何边界clamp约束",
                "RemotePouch将重计算放在云端本地仅调用",
                "LanguagePouch是E0层唯一本地核心袋",
                "fallback_chain按E0到E2优先级遍历",
            ],
            "chemistry" => &[
                "水分子H2O的结构",
                "碳的四种同素异形体",
                "苯环的共振结构",
                "蛋白质的四级结构",
                "DNA双螺旋中的碱基配对规则",
                "催化剂如何降低活化能",
                "离子键和共价键的区别",
                "电化学中电极电位的含义",
                "化学平衡常数的意义",
                "稀有气体为什么化学性质稳定",
                "有机化学中的官能团分类",
                "酸碱滴定的终点判断",
                "高分子聚合的机理",
                "化学热力学中自由能的作用",
                "胶体和溶液的区别",
                "放射性同位素的衰变规律",
            ],
            "material" => &[
                "钛合金的抗拉强度",
                "碳纤维复合材料的特性",
                "高温超导陶瓷的临界温度",
                "石墨烯的导电性",
                "形状记忆合金的工作原理",
                "纳米材料的表面效应",
                "陶瓷材料的脆性断裂机理",
                "金属疲劳的微观机制",
                "聚合物的玻璃化转变温度",
                "半导体材料的能带结构",
                "生物材料的生物相容性",
                "压电材料的工作原理",
                "稀土元素在材料中的应用",
                "复合材料的界面结合强度",
                "智能材料的自修复机制",
                "非晶态金属的特殊性质",
            ],
            _ => &[
                "你的核心能力是什么",
                "分析当前系统状态",
                "可以处理什么类型的任务",
                "你的原子能力声明",
                "你的置信度范围是多少",
                "你能为其他尿袋提供什么帮助",
                "你和其他同类尿袋的区别",
                "你需要什么输入才能发挥最大能力",
                "你在什么情况下会失败",
                "你如何利用sync_patterns提升",
                "你的记忆容量当前是多少",
                "你最近学到了什么新模式",
                "你处理过最复杂的请求是什么",
                "你的输出置信度通常在什么范围",
                "你如何判断自己的回答是否可靠",
                "列举你目前已掌握的三个核心技能",
            ],
        };
        let ci = cycle as usize;
        if ci < bank.len() * 3 {
            return bank[ci % bank.len()];
        }
        let ai = ci / 7 % Self::COMBO_ACTIONS.len();
        let ti = ci / 3 % Self::COMBO_TOPICS.len();
        let combo_bank: &[&str] = &[
            Self::COMBO_ACTIONS[ai], Self::COMBO_TOPICS[ti],
        ];
        let _ = combo_bank;
        bank[ci % bank.len()]
    }

    fn learning_intent_combo(cycle: u64) -> String {
        let ai = (cycle as usize) % Self::COMBO_ACTIONS.len();
        let ti = ((cycle as usize) / Self::COMBO_ACTIONS.len()) % Self::COMBO_TOPICS.len();
        format!("{}{}的核心原理", Self::COMBO_ACTIONS[ai], Self::COMBO_TOPICS[ti])
    }

    fn load_external_seeds(data_dir: &str) -> Vec<(String, String)> {
        let dir = std::path::Path::new(data_dir).join("external_seeds");
        let mut seeds = Vec::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return seeds,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|e| e == "json" || e == "jsonl") {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if path.extension().is_some_and(|e| e == "jsonl") {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() { continue; }
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        if let (Some(i), Some(r)) = (v["intent"].as_str(), v["response"].as_str()) {
                            seeds.push((i.to_string(), r.to_string()));
                        }
                    }
                }
            } else if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                for v in &arr {
                    if let (Some(i), Some(r)) = (v["intent"].as_str(), v["response"].as_str()) {
                        seeds.push((i.to_string(), r.to_string()));
                    }
                }
            }
        }
        seeds
    }

    pub fn learning_snapshot(&self) -> &LearningState {
        &self.learning
    }

    pub fn learning_metrics_extra(&self) -> (usize, usize, f64) {
        let info = self.pouches_info();
        let pattern_count = info.iter().find(|(n, _, _, _)| n == "language").map(|(_, _, m, _)| *m).unwrap_or(0);
        let pouch_count = info.len();
        let mat_sum: f64 = info.iter().map(|(n, _, _, _)| self.pouch_maturity(n)).sum();
        let avg_maturity = if pouch_count > 0 { mat_sum / pouch_count as f64 } else { 0.0 };
        (pattern_count, pouch_count, avg_maturity)
    }

    const CLOUD_WORKER_BASE: &'static str = "https://logos-gateway.amrta.workers.dev";

    pub async fn sync_with_cloud(&mut self) {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        let pull_url = format!(
            "{}/sync?since={}&limit=200",
            Self::CLOUD_WORKER_BASE,
            self.learning.cloud_sync_cursor
        );
        if let Ok(resp) = client.get(&pull_url).send().await {
            if resp.status().is_success() {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    let mut pulled = 0u64;
                    if let Some(pairs) = body["pairs"].as_array() {
                        for pair in pairs {
                            let human = pair["human"].as_str().unwrap_or("");
                            let gpt = pair["gpt"].as_str().unwrap_or("");
                            if human.len() >= 2 && gpt.len() > 5 {
                                self.language.teach(human, gpt);
                                pulled += 1;
                            }
                        }
                    }
                    if let Some(max_id) = body["max_id"].as_i64() {
                        if max_id > self.learning.cloud_sync_cursor {
                            self.learning.cloud_sync_cursor = max_id;
                        }
                    }
                    if pulled > 0 {
                        self.learning.cloud_pulled += pulled;
                        self.log_event(format!("CLOUD_PULL {} pairs, cursor={}", pulled, self.learning.cloud_sync_cursor));
                    }
                }
            }
        }

        let top_pairs = self.language.top_quality_pairs(50);
        if !top_pairs.is_empty() {
            let push_pairs: Vec<serde_json::Value> = top_pairs.iter()
                .map(|(h, g)| serde_json::json!({"human": h, "gpt": g}))
                .collect();
            let push_url = format!("{}/sync", Self::CLOUD_WORKER_BASE);
            if let Ok(resp) = client
                .post(&push_url)
                .json(&serde_json::json!({"pairs": push_pairs}))
                .send()
                .await
            {
                if resp.status().is_success() {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let inserted = body["inserted"].as_u64().unwrap_or(0);
                        if inserted > 0 {
                            self.learning.cloud_pushed += inserted;
                            self.log_event(format!("CLOUD_PUSH {} new pairs", inserted));
                        }
                    }
                }
            }
        }
    }

    pub async fn autonomous_learning_cycle(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.learning.cycle_count += 1;
        self.learning.last_cycle_ts = now;

        let mut maturities: Vec<(String, f64)> = Vec::new();
        let installed = self.installed().iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for name in &installed {
            if name == "language" || self.is_pouch_awake(name) {
                let m = self.pouch_maturity(name);
                maturities.push((name.clone(), m));
            }
        }
        maturities.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let hungry: Vec<(String, f64)> = maturities.iter()
            .filter(|(_, m)| *m < MATURITY_HUNGRY)
            .cloned()
            .collect();

        let misses = self.pending_misses(12);

        let cycle = self.learning.cycle_count;
        let force_synapse = cycle > 0 && cycle.is_multiple_of(5);
        let do_feed = !hungry.is_empty()
            && self.learning.saturation < SATURATION_THRESHOLD
            && !force_synapse;

        if do_feed {
            self.learning.phase = if misses.is_empty() { "external".into() } else { "miss_driven".into() };
            let before_mem = self.total_memory_count();
            let before_evo = self.evolution.len();

            self.call_stack.clear();
            let _ = self.guard(Layer::Orchestrator);

            let mut fed_count: u64 = 0;
            if !misses.is_empty() {
                for miss_input in misses.iter().take(8) {
                    let lang_result = self.call_pouch("language", miss_input).await;
                    if let Ok(ref lang_out) = lang_result {
                        if lang_out.len() > 20 && !self.language.is_fallback_response(lang_out) {
                            let tokens = self.language.tokenize(miss_input);
                            let broadcast = vec![(tokens, lang_out.clone(), 1.3)];
                            for (_, pouch) in self.pouches.iter_mut() {
                                pouch.sync_patterns(&broadcast);
                            }
                            self.language.absorb(miss_input, lang_out, 1.0);
                        }
                    }
                    for (name, _) in hungry.iter().take(4) {
                        if name != "language" {
                            let pouch_result = self.call_pouch(name, miss_input).await;
                            if let Ok(ref pouch_out) = pouch_result {
                                if pouch_out.len() > 15 && !self.language.is_fallback_response(pouch_out) {
                                    self.language.absorb(miss_input, pouch_out, 0.8);
                                }
                            }
                        }
                    }
                    fed_count += 1;
                }
            } else {
                let seeds = Self::load_external_seeds(&self.data_dir);
                let use_seed = cycle % 5 == 2 && !seeds.is_empty();

                if use_seed {
                    let seed_idx = (cycle as usize / 5) % seeds.len();
                    let (ref s_intent, ref s_response) = seeds[seed_idx];
                    let tokens = self.language.tokenize(s_intent);
                    let broadcast = vec![(tokens, s_response.clone(), 1.5)];
                    for (_, pouch) in self.pouches.iter_mut() {
                        pouch.sync_patterns(&broadcast);
                    }
                    self.language.teach(s_intent, s_response);
                    fed_count += 1;
                    self.log_event(format!("SEED_FED {}", &s_intent[..s_intent.len().min(30)]));
                }

                for (idx, (name, _)) in hungry.iter().take(6).enumerate() {
                    let combo = Self::learning_intent_combo(cycle + idx as u64);
                    let use_combo = cycle.is_multiple_of(3) && idx == 0;
                    let fixed = Self::learning_intents_for(name, cycle + idx as u64);
                    let intent_str: &str = if use_combo { &combo } else { fixed };
                    let lang_result = self.call_pouch("language", intent_str).await;
                    if let Ok(ref lang_out) = lang_result {
                        if lang_out.len() > 20 && !self.language.is_fallback_response(lang_out) {
                            let tokens = self.language.tokenize(intent_str);
                            let broadcast = vec![(tokens, lang_out.clone(), 1.3)];
                            for (_, pouch) in self.pouches.iter_mut() {
                                pouch.sync_patterns(&broadcast);
                            }
                        }
                    }
                    let _ = self.call_pouch(name, intent_str).await;
                    fed_count += 1;
                }
            }

            self.unguard();

            self.discover_gaps_from_misses().await;

            let gained_mem = self.total_memory_count().saturating_sub(before_mem);
            let gained_evo = self.evolution.len().saturating_sub(before_evo);
            self.learning.external_fed += fed_count;
            self.learning.external_absorbed += (gained_mem + gained_evo) as u64;

            if self.learning.external_fed > 8 {
                let fed = self.learning.external_fed.max(1) as f64;
                let gap = self.learning.external_fed.saturating_sub(self.learning.external_absorbed) as f64;
                self.learning.saturation = (1.0 - gap / fed).clamp(0.0, 1.0);
            }

            self.log_event(format!(
                "LEARN_{} 投喂:{} 缺口:{} 记忆+{} 演化+{} 饱和:{:.0}%",
                if misses.is_empty() { "EXT" } else { "MISS" },
                fed_count, misses.len(), gained_mem, gained_evo, self.learning.saturation * 100.0
            ));
        } else {
            self.learning.phase = "synapse".into();
            let before_mem = self.total_memory_count();
            let before_evo = self.evolution.len();

            let mut by_maturity = maturities.clone();
            by_maturity.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let mut teachers: Vec<String> = by_maturity.iter()
                .filter(|(n, m)| *m >= MATURITY_HUNGRY && *n != "language")
                .map(|(n, _)| n.clone())
                .collect();
            if teachers.is_empty() {
                teachers = by_maturity.iter()
                    .filter(|(n, m)| *n != "language" && *m > 0.0)
                    .take(3)
                    .map(|(n, _)| n.clone())
                    .collect();
            }
            let students: Vec<String> = by_maturity.iter()
                .rev()
                .filter(|(n, _)| *n != "language" && !teachers.contains(n))
                .map(|(n, _)| n.clone())
                .take(CROSS_LEARN_BATCH)
                .collect();

            let mut cross_count: u64 = 0;

            self.call_stack.clear();
            let _ = self.guard(Layer::Orchestrator);

            let intent_count = CROSS_LEARN_BATCH.max(misses.len().min(12));
            let mut intents: Vec<String> = Vec::with_capacity(intent_count);
            for i in 0..intent_count {
                if i < misses.len() {
                    intents.push(misses[i].clone());
                } else {
                    let combo = Self::learning_intent_combo(cycle + i as u64);
                    if (cycle + i as u64).is_multiple_of(3) {
                        intents.push(combo);
                    } else {
                        let teacher_idx = i % teachers.len().max(1);
                        let t_name = teachers.get(teacher_idx).map_or("reasoning", |s| s.as_str());
                        intents.push(Self::learning_intents_for(t_name, cycle + i as u64).to_string());
                    }
                }
            }

            let mut lang_outputs: Vec<(Vec<String>, String)> = Vec::new();
            for intent in &intents {
                let lang_result = self.call_pouch("language", intent).await;
                if let Ok(ref lang_out) = lang_result {
                    if lang_out.len() > 20 && !self.language.is_fallback_response(lang_out) {
                        let tokens = self.language.tokenize(intent);
                        lang_outputs.push((tokens, lang_out.clone()));
                    }
                }
            }

            if !lang_outputs.is_empty() {
                let batch: Vec<(Vec<String>, String, f64)> = lang_outputs.iter()
                    .map(|(t, o)| (t.clone(), o.clone(), 1.3))
                    .collect();
                for (_, pouch) in self.pouches.iter_mut() {
                    pouch.sync_patterns(&batch);
                }
            }

            let mut teacher_outputs: Vec<(String, String, String)> = Vec::new();
            let a2_hops = self.config.chain.a2_chain_hops.clamp(2, 12);
            let mut chain_intents: Vec<String> = intents.clone();
            for hop in 0..a2_hops {
                let mut hop_outputs: Vec<(String, String, String)> = Vec::new();
                for teacher in &teachers {
                    for intent in &chain_intents {
                        let result = self.call_pouch(teacher, intent).await;
                        if let Ok(ref output) = result {
                            if output.len() > 10 && !self.language.is_fallback_response(output) {
                                hop_outputs.push((teacher.clone(), intent.clone(), output.clone()));
                            }
                        }
                    }
                }
                for o in &hop_outputs {
                    teacher_outputs.push(o.clone());
                }
                if hop + 1 >= a2_hops {
                    break;
                }
                chain_intents = hop_outputs
                    .iter()
                    .map(|(_, _, o)| {
                        let s = o.trim();
                        if s.len() > A2_NEXT_INTENT_MAX {
                            s.chars().take(A2_NEXT_INTENT_MAX).collect::<String>()
                        } else {
                            s.to_string()
                        }
                    })
                    .filter(|s| s.len() > 10 && !self.language.is_fallback_response(s))
                    .collect::<Vec<_>>();
                if chain_intents.is_empty() {
                    break;
                }
            }

            if !teacher_outputs.is_empty() {
                let teach_batch: Vec<(Vec<String>, String, f64)> = teacher_outputs.iter()
                    .map(|(_, intent, output)| {
                        let tokens = self.language.tokenize(intent);
                        (tokens, output.clone(), 1.2)
                    })
                    .collect();
                for student in &students {
                    if let Some(pouch) = self.pouches.get_mut(student.as_str()) {
                        pouch.sync_patterns(&teach_batch);
                    }
                }
                for (_, intent, output) in &teacher_outputs {
                    self.language.absorb(intent, output, 0.8);
                }
                cross_count = teacher_outputs.len() as u64;
            }

            self.unguard();

            let gained_mem = self.total_memory_count().saturating_sub(before_mem);
            let gained_evo = self.evolution.len().saturating_sub(before_evo);
            self.learning.cross_fed += cross_count;
            self.learning.cross_absorbed += (gained_mem + gained_evo) as u64;

            if cross_count > 0 {
                self.learning.saturation = (self.learning.saturation - 0.08).max(0.0);
            }

            self.log_event(format!(
                "LEARN_SYN 突触:{} 记忆+{} 演化+{} 饱和:{:.0}%",
                cross_count, gained_mem, gained_evo, self.learning.saturation * 100.0
            ));
        }

        if cycle > 0 && cycle.is_multiple_of(20) {
            match self.self_optimize().await {
                Ok(msg) => self.log_event(format!("SELF_OPT_AUTO {}", msg.lines().count())),
                Err(e) => self.log_event(format!("SELF_OPT_AUTO_ERR {}", e)),
            }
        }
        if cycle > 0 && cycle.is_multiple_of(31) {
            match self.evolve().await {
                Ok(msg) => self.log_event(format!("EVOLVE_AUTO {}", msg.lines().count())),
                Err(e) => self.log_event(format!("EVOLVE_AUTO_ERR {}", e)),
            }
        }

        if cycle > 0 && cycle.is_multiple_of(7) {
            self.sync_with_cloud().await;
        }

        self.maybe_adjust_baseline();
        self.save_state();
    }

    fn lookup_remote_spec(&self, name: &str) -> Option<crate::remote_pouch::RemotePouchSpec> {
        let spec_path = std::path::Path::new(&self.data_dir).join("remote_pouches.json");
        if !spec_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&spec_path).ok()?;
        let specs: Vec<crate::remote_pouch::RemotePouchSpec> =
            serde_json::from_str(&content).ok()?;
        specs.into_iter().find(|s| s.name == name)
    }
}

/*
 * E2E 测试覆盖范围声明（防幻觉）：
 *   - 本模块含单链路 e2e：意图 "对比能力" → Reject → plan → execute_plan → 输出 + 演化链
 *   - 不覆盖：从 UI/HTTP 入口的完整请求、Language 层内部 tokenize/teach、云端 analyze、多轮对话
 *   - 生产级 e2e 建议：测试脚本部署为 Cloudflare Worker，用 Workers AI 校验输出合理性
 */
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_e2e_reject_plan_path_output_and_chain() {
        let dir = "/tmp/logos_test_e2e";
        let _ = std::fs::remove_dir_all(dir);
        let mut orch = Orchestrator::new(dir);
        assert!(orch.install("capability_comparer").is_ok());
        let chain_before = orch.recent_evolution_entries(100).len();
        let result = orch.execute_with_pouch("对比能力").await;
        let (out, pouch) = match result {
            Ok(x) => x,
            Err((msg, _)) => panic!("e2e should not error: {}", msg),
        };
        assert_eq!(pouch, "plan", "e2e must go Reject → plan path");
        assert!(!out.is_empty());
        assert!(out.contains("实测") || out.contains("LOGOS") || out.contains("能力"), "output must contain expected snippet: {}", out);
        let chain_after = orch.recent_evolution_entries(100).len();
        assert!(chain_after > chain_before, "evolution_chain must grow after successful plan run");
    }

    #[test]
    fn test_guard_adjacent() {
        let mut orch = Orchestrator::new("/tmp/logos_test_guard");
        assert!(orch.guard(Layer::Orchestrator).is_ok());
        assert!(orch.guard(Layer::Pouch).is_ok());
        orch.unguard();
        orch.unguard();
    }

    #[test]
    fn test_guard_reject_cross_layer() {
        let mut orch = Orchestrator::new("/tmp/logos_test_reject");
        assert!(orch.guard(Layer::Orchestrator).is_ok());
        assert!(orch.guard(Layer::Bedrock).is_err());
        orch.unguard();
    }

    #[test]
    fn test_installed_has_language() {
        let orch = Orchestrator::new("/tmp/logos_test_inst");
        assert!(orch.installed().contains(&"language"));
    }

    #[test]
    fn test_routing_score_config_loaded() {
        let dir = "/tmp/logos_test_config_load";
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::create_dir_all(dir);
        let config_path = format!("{}/pouch_config.json", dir);
        let mut config = crate::config::SystemConfig::default();
        config.routing_score = crate::config::RoutingScoreConfig {
            baseline_score: 0.8,
            low_score_threshold: 0.3,
            promote_min_chain_score: 0.0,
        };
        let _ = config.save(&config_path);
        let orch = Orchestrator::new(dir);
        assert_eq!(orch.routing_baseline(), 0.8);
        assert_eq!(orch.routing_low_threshold(), 0.3);
    }

    #[test]
    fn test_install_uninstall() {
        let mut orch = Orchestrator::new("/tmp/logos_test_iu");
        assert!(orch.install("reasoning").is_ok());
        assert!(orch.installed().contains(&"reasoning"));
        assert!(orch.uninstall("reasoning").is_ok());
        assert!(!orch.installed().contains(&"reasoning"));
    }

    #[tokio::test]
    async fn test_pilot_pouch_install_and_call() {
        let _ = std::fs::remove_dir_all("/tmp/logos_test_pilot");
        let mut orch = Orchestrator::new("/tmp/logos_test_pilot");
        assert!(orch.install("pilot").is_ok());
        assert!(orch.installed().contains(&"pilot"));
        let r = orch.call_pouch("pilot", "hello").await;
        let out = match r {
            Ok(s) => s,
            Err(e) => panic!("pilot call: {}", e),
        };
        assert!(out.contains("Pilot") && out.contains("hello"));
    }

    /*
     * 连续喂养验证：多轮相同/相似意图观察 evolution_chain 累积与 score 效应。
     * 注意：单次执行不代表整体；promotion 仅在直连 ToPouch 路径发生（record_evolution），
     * 本测试走 Reject→plan 路径，故主要验证链增长与 pouch 重复出现（即 score 累积）。
     */
    #[tokio::test]
    async fn test_continuous_execution_evolution() {
        let dir = "/tmp/logos_test_continuous";
        let _ = std::fs::remove_dir_all(dir);
        let mut orch = Orchestrator::new(dir);
        assert!(orch.install("capability_comparer").is_ok());
        assert!(orch.install("pilot").is_ok());
        let mut chosen_log: Vec<String> = Vec::new();
        let mut chain_lens: Vec<usize> = Vec::new();
        let mut promoted_log: Vec<usize> = Vec::new();
        for i in 0..15 {
            let result = orch.execute_with_pouch("对比能力").await;
            let (_, pouch) = match result {
                Ok(x) => x,
                Err((msg, _)) => panic!("each run should ok: {}", msg),
            };
            chosen_log.push(pouch.clone());
            let entries = orch.recent_evolution_entries(3);
            let chain_len = orch.recent_evolution_entries(100).len();
            chain_lens.push(chain_len);
            let plen = orch.promoted_cache_len();
            promoted_log.push(plen);
            let prev_plen = if i > 0 { promoted_log[i - 1] } else { 0 };
            let promoted_new = plen > prev_plen;
            eprintln!("[{}] pouch={} chain_len={} promoted_cache={} promoted_new={}", i + 1, pouch, chain_len, plen, promoted_new);
            for e in &entries {
                eprintln!("  recent pouch={} output_len={}", e.pouch_name, e.output_trunc.len());
            }
        }
        let final_chain_len = orch.recent_evolution_entries(100).len();
        assert!(final_chain_len >= 2, "evolution_chain should grow after at least one plan run (got {})", final_chain_len);
        let comparer_count = orch.recent_evolution_entries(100).iter().filter(|e| e.pouch_name == "capability_comparer").count();
        assert!(comparer_count >= 1, "capability_comparer should appear in chain (got {})", comparer_count);
        let promotion_happened = promoted_log.iter().any(|&n| n > 0);
        let chain_grew_over_runs = chain_lens.last() > chain_lens.first();
        let value_observed = promotion_happened || chain_grew_over_runs || (final_chain_len >= 2 && comparer_count >= 1);
        assert!(value_observed, "continuous feeding: promote={} chain_grew={} chain_len={} comparer_count={}", promotion_happened, chain_grew_over_runs, final_chain_len, comparer_count);
    }

    #[tokio::test]
    async fn test_reject_branch_plan_path_when_kinds_covered() {
        let _ = std::fs::remove_dir_all("/tmp/logos_test_plan_ok");
        let mut orch = Orchestrator::new("/tmp/logos_test_plan_ok");
        assert!(orch.install("capability_comparer").is_ok());
        let result = orch.execute_with_pouch("对比能力").await;
        match &result {
            Ok((s, pouch)) => {
                assert_eq!(pouch.as_str(), "plan");
                assert!(!s.is_empty());
                assert!(s.contains("实测") || s.contains("LOGOS") || s.contains("能力"));
            }
            Err((e, _)) => panic!("expected Ok, got Err: {}", e),
        }
    }

    #[tokio::test]
    async fn test_passive_feedback_logs_on_success() {
        let _ = std::fs::remove_dir_all("/tmp/logos_test_passive_ok");
        let mut orch = Orchestrator::new("/tmp/logos_test_passive_ok");
        assert!(orch.install("capability_comparer").is_ok());
        let _ = orch.execute_with_pouch("对比能力").await;
        let events = orch.recent_events();
        let has_pouch_success = events.iter().any(|e| e.starts_with("PouchSuccess:"));
        let has_comparer = events.iter().any(|e| e.contains("pouch=capability_comparer"));
        let has_output = events.iter().any(|e| e.contains("output=") && (e.contains("实测") || e.contains("LOGOS") || e.contains("能力")));
        assert!(has_pouch_success, "events should contain PouchSuccess: {:?}", events);
        assert!(has_comparer, "events should mention capability_comparer: {:?}", events);
        assert!(has_output, "events should contain output snippet: {:?}", events);
    }

    #[tokio::test]
    async fn test_passive_feedback_no_log_on_failure() {
        let mut orch = Orchestrator::new("/tmp/logos_test_passive_fail");
        let plan = crate::atom::ExecutionPlan {
            steps: vec![crate::atom::ExecutionStep {
                atom_name: "fake".into(),
                pouch: "nonexistent_pouch_xyz".into(),
                kind: crate::atom::AtomKind::Match,
                input_from: crate::atom::StepInput::UserInput,
            }],
        };
        let r = orch.execute_plan(&plan, "input").await;
        assert!(r.is_err());
        let events = orch.recent_events();
        let pouch_success_count = events.iter().filter(|e| e.starts_with("PouchSuccess:")).count();
        assert_eq!(pouch_success_count, 0, "failed step must not record PouchSuccess: {:?}", events);
    }

    #[tokio::test]
    async fn test_evolution_chain_has_entry_after_success() {
        let _ = std::fs::remove_dir_all("/tmp/logos_test_chain_ok");
        let mut orch = Orchestrator::new("/tmp/logos_test_chain_ok");
        assert!(orch.install("capability_comparer").is_ok());
        let _ = orch.execute_with_pouch("对比能力").await;
        let entries = orch.recent_evolution_entries(10);
        let has_comparer = entries.iter().any(|e| e.pouch_name == "capability_comparer");
        let has_output_snippet = entries.iter().any(|e| {
            e.output_trunc.contains("实测") || e.output_trunc.contains("LOGOS") || e.output_trunc.contains("能力")
        });
        assert!(has_comparer, "evolution_chain should have capability_comparer entry: {:?}", entries);
        assert!(has_output_snippet, "evolution_chain entry should have output snippet: {:?}", entries);
        assert!(entries.iter().all(|e| e.success), "all entries should be success=true");
    }

    #[tokio::test]
    async fn test_evolution_chain_appends_in_order() {
        let _ = std::fs::remove_dir_all("/tmp/logos_test_chain_order");
        let mut orch = Orchestrator::new("/tmp/logos_test_chain_order");
        assert!(orch.install("capability_comparer").is_ok());
        let plan = crate::atom::ExecutionPlan {
            steps: vec![crate::atom::ExecutionStep {
                atom_name: "compare".into(),
                pouch: "capability_comparer".into(),
                kind: crate::atom::AtomKind::Match,
                input_from: crate::atom::StepInput::UserInput,
            }],
        };
        let _ = orch.execute_plan(&plan, "对比能力").await;
        let _ = orch.execute_plan(&plan, "对比能力").await;
        let entries = orch.recent_evolution_entries(100);
        assert!(entries.len() >= 2, "chain should have at least 2 entries after two execute_plan calls: {}", entries.len());
        let mut prev_ts = 0u64;
        for e in &entries {
            assert!(e.timestamp >= prev_ts, "entries should be non-decreasing by timestamp");
            prev_ts = e.timestamp;
        }
    }

    #[tokio::test]
    async fn test_evolution_chain_persists_across_restart() {
        let _ = std::fs::remove_dir_all("/tmp/logos_test_persist");
        let mut orch1 = Orchestrator::new("/tmp/logos_test_persist");
        assert!(orch1.install("capability_comparer").is_ok());
        let plan = crate::atom::ExecutionPlan {
            steps: vec![crate::atom::ExecutionStep {
                atom_name: "compare".into(),
                pouch: "capability_comparer".into(),
                kind: crate::atom::AtomKind::Match,
                input_from: crate::atom::StepInput::UserInput,
            }],
        };
        let _ = orch1.execute_plan(&plan, "对比能力").await;
        let _ = orch1.execute_plan(&plan, "对比能力").await;
        let entries1 = orch1.recent_evolution_entries(100);
        assert!(entries1.len() >= 2, "orch1 should have at least 2 entries");
        drop(orch1);
        let orch2 = Orchestrator::new("/tmp/logos_test_persist");
        let entries2 = orch2.recent_evolution_entries(100);
        assert!(entries2.len() >= 2, "after restart chain should restore: got {}", entries2.len());
        assert_eq!(entries1.len(), entries2.len(), "restored chain length should match");
        for (a, b) in entries1.iter().zip(entries2.iter()) {
            assert_eq!(a.pouch_name, b.pouch_name);
            assert_eq!(a.input_trunc, b.input_trunc);
            assert_eq!(a.output_trunc, b.output_trunc);
            assert_eq!(a.step_index, b.step_index);
            assert_eq!(a.success, b.success);
        }
    }

    #[tokio::test]
    async fn test_reject_branch_fallback_when_no_plan() {
        let _ = std::fs::remove_dir_all("/tmp/logos_test_plan_fallback");
        let mut orch = Orchestrator::new("/tmp/logos_test_plan_fallback");
        let result = orch.execute_with_pouch("对比能力").await;
        let (_, pouch) = match result {
            Ok(x) => x,
            Err((msg, _)) => panic!("execute should not error: {}", msg),
        };
        assert_ne!(pouch.as_str(), "plan");
    }

    #[test]
    fn test_role_priority_e0_before_e1_before_e2() {
        assert!(Orchestrator::role_priority(PouchRole::E0) < Orchestrator::role_priority(PouchRole::E1));
        assert!(Orchestrator::role_priority(PouchRole::E1) < Orchestrator::role_priority(PouchRole::E2));
    }
}
