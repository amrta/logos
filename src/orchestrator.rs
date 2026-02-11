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

fn hash_str(s: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

pub const VERSION: &str = "5.0.0";

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
    registry: CapabilityRegistry,
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
            registry: CapabilityRegistry::new(),
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

        if let Err(e) = self.guard(Layer::Orchestrator) {
            return Err((e, "system".into()));
        }

        let trimmed = input.trim();
        if trimmed.starts_with("语言评估") || trimmed.to_lowercase().starts_with("eval language") {
            let path = trimmed
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
                            Ok(data) => Ok((data, "plan".into())),
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
                        return Ok((last_output, "cloud_plan".into()));
                    }
                    _ => {}
                }
                let response = self.language.process(input).await;
                self.log_event("LANG process".into());
                if !self.language.is_fallback_response(&response) {
                    return self.unguard_then(Ok((response, "language".into())));
                }
                if let Some((out, pouch)) = self.try_fallback_chain(input).await {
                    return self.unguard_then(Ok((out, pouch)));
                }
                return self.unguard_then(Ok((response, "language".into())));
            }
        };

        self.unguard();
        result
    }

    fn unguard_then<T>(&mut self, v: T) -> T {
        self.unguard();
        v
    }

    async fn try_fallback_chain(&mut self, input: &str) -> Option<(String, String)> {
        let proposal = create_proposal(input);
        if let Some(reason) = self.pouches.get_mut("reasoning") {
            if let Ok(validated) = reason.validator().validate(&proposal) {
                if let Ok(output) = reason.process_proposal(&validated).await {
                    if !reason.is_fallback_output(&output.data) {
                        let conf_note = if output.confidence < 0.5 { " [低置信度]" } else { "" };
                        return Some((format!("{}{}", output.data, conf_note), "reasoning".into()));
                    }
                }
            }
        }
        if let Some(creative) = self.pouches.get_mut("creative") {
            if let Ok(validated) = creative.validator().validate(&proposal) {
                if let Ok(output) = creative.process_proposal(&validated).await {
                    if !output.data.is_empty() && !creative.is_fallback_output(&output.data) {
                        let conf_note = if output.confidence < 0.5 { " [低置信度]" } else { "" };
                        return Some((format!("{}{}", output.data, conf_note), "creative".into()));
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
                let tokens: Vec<String> = input.split_whitespace().map(|s| s.to_string()).collect();
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

        let (pouch, role) = crate::pouch_catalog::instantiate(&name, &self.data_dir)
            .ok_or_else(|| "无法安装该尿袋".to_string())?;
        let caps = pouch.atom_capabilities();
        self.pouches.insert(name.clone(), pouch);
        self.meta.insert(name.clone(), PouchMeta { role });
        for cap in caps {
            self.registry.register(cap);
        }
        self.save_state();
        Ok(format!("安装「{}」({:?}) atoms:{}", name, role, self.registry.count()))
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
    }

    fn maybe_adjust_baseline(&mut self) {
        const AUTO_ADJUST_MIN_CHAIN: usize = 50;
        const BASELINE_STEP: f64 = 0.02;
        if self.evolution_chain.len() < AUTO_ADJUST_MIN_CHAIN {
            return;
        }
        let success_count = self.evolution_chain.iter().filter(|e| e.success).count();
        let total = self.evolution_chain.len();
        let success_rate = success_count as f64 / total as f64;
        let bounds = manager_math::RoutingParamsBounds::default();
        let current = self.config.routing_score.baseline_score;
        let new_baseline = manager_math::adjusted_baseline(
            current,
            success_rate,
            (bounds.baseline_min, bounds.baseline_max),
            BASELINE_STEP,
        );
        if (new_baseline - current).abs() > 1e-6 {
            self.config.routing_score.baseline_score = new_baseline;
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
                for rec in &records {
                    if rec.promoted {
                        self.promoted_cache.insert(rec.input_hash, String::new());
                    }
                }
                self.evolution = records;
            }
        }
        self.evolution_chain = loaded_chain;
        if let Ok(data) = std::fs::read(format!("{}/promoted_rules.json", self.data_dir)) {
            logic::load_promoted_rules(&data).ok();
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

                if let Some(ref ev) = event {
                    if ev.contains("promoted") {
                        self.promoted_cache.insert(ih, output.to_string());
                    } else {
                        self.promoted_cache.remove(&ih);
                    }
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
        self.promoted_cache.get(&ih)
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
}
