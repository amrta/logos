use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

mod atom;
mod frozen;
mod orchestrator;
mod language_pouch;
mod manager_math;
mod pouch_trait;
mod resource_monitor;
mod remote_pouch;
mod config;
mod pouch_programming;
mod pouch_benchmark;
mod pouch_defect_scanner;
mod pouch_capability_comparer;
mod pouch_catalog;
mod pouch_code_analyzer;
mod pouch_knowledge_retriever;
mod pouch_pilot;
mod pouch_analogy;
mod pouch_induction;
mod pouch_deduction;
mod pouch_code_template;
mod pouch_compose;
mod pouch_fragment;
mod pouch_generator;
mod pouch_explorer;
mod pouch_image;
mod pouch_audio;
mod pouch_realtime;
mod pouch_sanitize;
mod test_terminal;

use orchestrator::Orchestrator;
use resource_monitor::Monitor;

struct App {
    orch: Mutex<Orchestrator>,
    monitor: Mutex<Monitor>,
}

#[derive(Deserialize)]
struct Req {
    message: String,
}

#[derive(Serialize)]
struct Res {
    response: String,
    status: String,
    pouch: String,
}

#[derive(Serialize)]
struct StatusRes {
    version: String,
    cpu: f32,
    mem_mb: usize,
    temp: f32,
    pouches: usize,
    memory_count: usize,
    ready: bool,
}

#[derive(Serialize)]
struct PouchInfo {
    name: String,
    role: String,
    memory: usize,
    awake: bool,
}

#[derive(Serialize)]
struct EventsRes {
    events: Vec<String>,
}

#[derive(Serialize)]
struct ArchitectureRes {
    layers: Vec<LayerInfo>,
    pouches_detail: Vec<PouchDetail>,
    evolution: EvolutionInfo,
    atoms: Vec<atom::AtomDeclaration>,
}

#[derive(Serialize)]
struct LayerInfo {
    name: String,
    description: String,
    status: String,
}

#[derive(Serialize)]
struct PouchDetail {
    name: String,
    role: String,
    memory: usize,
    awake: bool,
    explanation: String,
    atoms: Vec<String>,
}

#[derive(Serialize)]
struct EvolutionInfo {
    total_records: usize,
    promoted: usize,
    candidates: usize,
    l2_rules: usize,
}

#[derive(Serialize)]
struct DataSizeRes {
    data_dir: String,
    language_bin_bytes: u64,
    routes_bin_bytes: u64,
    evolution_json_bytes: u64,
    evolution_chain_json_bytes: u64,
    promoted_rules_json_bytes: u64,
    pouches_json_bytes: u64,
    total_bytes: u64,
    memory_count: usize,
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data_dir = std::env::var("LOGOS_DATA").unwrap_or_else(|_| "./data".into());
    std::fs::create_dir_all(&data_dir).ok();

    let import_path = args.iter().position(|a| a == "--import").and_then(|i| args.get(i + 1)).map(|s| s.as_str());
    let eval_path = args.iter().position(|a| a == "--eval").and_then(|i| args.get(i + 1)).map(|s| s.as_str());
    if import_path.is_some() || eval_path.is_some() {
        let mut orch = Orchestrator::new(&data_dir);
        if let Some(p) = import_path {
            match orch.import_patterns_from_file(p).await {
                Ok(msg) => println!("{}", msg),
                Err(e) => { eprintln!("import error: {}", e); std::process::exit(1); }
            }
        }
        if let Some(p) = eval_path {
            match orch.eval_language(p).await {
                Ok(msg) => println!("{}", msg),
                Err(e) => { eprintln!("eval error: {}", e); std::process::exit(1); }
            }
        }
        return;
    }

    if args.iter().any(|a| a == "--terminal") {
        test_terminal::run().await;
        return;
    }
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    log::info!("LOGOS v{} 启动", frozen::bedrock::VERSION);

    let app = Arc::new(App {
        orch: Mutex::new(Orchestrator::new(&data_dir)),
        monitor: Mutex::new(Monitor::new()),
    });

    let app_c = app.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            let mut m = app_c.monitor.lock().await;
            let status = m.check();
            drop(m);
            match &status {
                resource_monitor::Status::Ok(metrics) => {
                    log::debug!("OK CPU:{:.1}% MEM:{}MB", metrics.cpu, metrics.mem_mb)
                }
                resource_monitor::Status::Warn(metrics, msg) => {
                    log::warn!("{} CPU:{:.1}%", msg, metrics.cpu)
                }
                resource_monitor::Status::Critical(metrics, msg) => {
                    log::error!("OVERLOAD {} CPU:{:.1}% MEM:{}MB", msg, metrics.cpu, metrics.mem_mb);
                    let mut orch = app_c.orch.lock().await;
                    orch.log_event_pub(format!("OVERLOAD CPU:{:.1}% MEM:{}MB", metrics.cpu, metrics.mem_mb));
                }
            }
        }
    });

    let app_learn = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(90));
        loop {
            interval.tick().await;
            let mut orch = app_learn.orch.lock().await;
            if orch.is_ready() {
                orch.autonomous_learning_cycle().await;
            }
        }
    });

    let router = Router::new()
        .route("/", get(ui))
        .route("/docs/:name", get(serve_doc))
        .route("/api/chat", post(chat))
        .route("/api/status", get(status))
        .route("/api/pouches", get(pouches_list))
        .route("/api/events", get(events))
        .route("/api/capabilities", get(capabilities))
        .route("/api/architecture", get(architecture))
        .route("/api/data_size", get(data_size))
        .route("/api/feedback", post(feedback))
        .route("/api/feedback_status", get(feedback_status_handler))
        .route("/api/language_debug", post(language_debug))
        .route("/api/batch_teach", post(batch_teach))
        .route("/api/seed_routes", post(seed_routes))
        .route("/api/dashboard", get(dashboard))
        .route("/api/learning_state", get(learning_state))
        .with_state(app);

    let addr = "127.0.0.1:3000";
    log::info!("http://{}", addr);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("端口绑定失败: {}", e);
            std::process::exit(1);
        }
    };
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await
        .ok();
}

async fn ui() -> Html<&'static str> {
    Html(include_str!("../ui/index.html"))
}

async fn serve_doc(Path(name): Path<String>) -> Response {
    const ALLOWED: [&str; 2] = ["LOGOS_ARCHITECTURE.svg", "POUCH_MANAGER_AND_LAYER.svg"];
    if !ALLOWED.contains(&name.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = std::path::Path::new("docs").join(&name);
    match tokio::fs::read(&path).await {
        Ok(data) => (
            [(header::CONTENT_TYPE, "image/svg+xml; charset=utf-8")],
            data,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn chat(State(app): State<Arc<App>>, Json(req): Json<Req>) -> (StatusCode, Json<Res>) {
    {
        let mut m = app.monitor.lock().await;
        if let resource_monitor::Status::Critical(_, msg) = m.check() {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(Res {
                    response: format!("系统过载: {}", msg),
                    status: "overload".into(),
                    pouch: "system".into(),
                }),
            );
        }
    }
    let mut orch = app.orch.lock().await;
    let input = if req.message.len() > frozen::bedrock::MAX_INPUT_LEN {
        let mut end = frozen::bedrock::MAX_INPUT_LEN;
        while !req.message.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        &req.message[..end]
    } else {
        &req.message
    };
    match orch.execute_with_pouch(input).await {
        Ok((r, pouch)) => (
            StatusCode::OK,
            Json(Res {
                response: r,
                status: "ok".into(),
                pouch,
            }),
        ),
        Err((e, pouch)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Res {
                response: e,
                status: "error".into(),
                pouch,
            }),
        ),
    }
}

async fn status(State(app): State<Arc<App>>) -> Json<StatusRes> {
    let mut m = app.monitor.lock().await;
    let metrics = match m.check() {
        resource_monitor::Status::Ok(m)
        | resource_monitor::Status::Warn(m, _)
        | resource_monitor::Status::Critical(m, _) => m,
    };
    let orch = app.orch.lock().await;
    Json(StatusRes {
        version: frozen::bedrock::VERSION.into(),
        cpu: metrics.cpu,
        mem_mb: metrics.mem_mb,
        temp: metrics.temp_est,
        pouches: orch.installed().len(),
        memory_count: orch.total_memory_count(),
        ready: orch.is_ready(),
    })
}

async fn pouches_list(State(app): State<Arc<App>>) -> Json<Vec<PouchInfo>> {
    let orch = app.orch.lock().await;
    let info: Vec<PouchInfo> = orch.pouches_info().into_iter().map(|(name, role, memory, awake)| {
        PouchInfo { name, role, memory, awake }
    }).collect();
    Json(info)
}

async fn events(State(app): State<Arc<App>>) -> Json<EventsRes> {
    let orch = app.orch.lock().await;
    Json(EventsRes { events: orch.recent_events() })
}

async fn capabilities(State(app): State<Arc<App>>) -> Json<Vec<atom::AtomDeclaration>> {
    let orch = app.orch.lock().await;
    Json(orch.capabilities_info())
}

async fn architecture(State(app): State<Arc<App>>) -> Json<ArchitectureRes> {
    let orch = app.orch.lock().await;
    let layers = vec![
        LayerInfo { name: "Bedrock".into(), description: "公式层：定义系统常量与不变量".into(), status: "active".into() },
        LayerInfo { name: "Logic".into(), description: "逻辑层：路由决策与层守卫".into(), status: "active".into() },
        LayerInfo { name: "Orchestrator".into(), description: "编排层：尿袋管理与执行计划".into(), status: "active".into() },
        LayerInfo { name: "Pouch".into(), description: "尿袋层：可插拔能力模块".into(), status: "active".into() },
    ];

    let atoms = orch.capabilities_info();
    let pouches_detail: Vec<PouchDetail> = orch.pouches_detail().into_iter().map(|(name, role, memory, awake, explanation, pouch_atoms)| {
        PouchDetail { name, role, memory, awake, explanation, atoms: pouch_atoms }
    }).collect();

    let evo = orch.evolution_info();

    Json(ArchitectureRes {
        layers,
        pouches_detail,
        evolution: EvolutionInfo {
            total_records: evo.0,
            promoted: evo.1,
            candidates: evo.2,
            l2_rules: evo.3,
        },
        atoms,
    })
}

#[derive(Deserialize)]
struct FeedbackReq {
    input: String,
    signal: i8,
    correction: Option<String>,
}

#[derive(Serialize)]
struct FeedbackRes {
    status: String,
    message: String,
}

async fn feedback(State(app): State<Arc<App>>, Json(req): Json<FeedbackReq>) -> Json<FeedbackRes> {
    let mut orch = app.orch.lock().await;
    orch.apply_feedback(&req.input, req.signal, req.correction.as_deref());
    Json(FeedbackRes {
        status: "ok".into(),
        message: format!("signal={} applied", req.signal),
    })
}

async fn feedback_status_handler(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    let orch = app.orch.lock().await;
    let (misses, log_count, absorbed, net_positive) = orch.language_feedback_stats();
    serde_json::json!({
        "misses": misses,
        "feedback_log": log_count,
        "absorbed": absorbed,
        "net_positive": net_positive,
        "memory_count": orch.total_memory_count(),
    }).into()
}

#[derive(Deserialize)]
struct LanguageDebugReq {
    input: String,
}

#[derive(Serialize)]
struct LanguageDebugRes {
    response: String,
    status: String,
    is_fallback: bool,
    last_match_weight: f64,
}

async fn language_debug(State(app): State<Arc<App>>, Json(req): Json<LanguageDebugReq>) -> Json<LanguageDebugRes> {
    let mut orch = app.orch.lock().await;
    let input = if req.input.len() > frozen::bedrock::MAX_INPUT_LEN {
        let mut end = frozen::bedrock::MAX_INPUT_LEN;
        while !req.input.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        &req.input[..end]
    } else {
        &req.input
    };
    let (response, is_fallback, last_match_weight) = orch.language_debug(input).await;
    Json(LanguageDebugRes {
        response,
        status: "ok".into(),
        is_fallback,
        last_match_weight,
    })
}

async fn seed_routes(State(app): State<Arc<App>>, body: String) -> Json<serde_json::Value> {
    let mut orch = app.orch.lock().await;
    let mut count = 0usize;
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        if let Some((pouch, query)) = line.split_once('|') {
            let pouch = pouch.trim();
            let query = query.trim();
            if !pouch.is_empty() && !query.is_empty() {
                orch.seed_route(query, pouch);
                count += 1;
            }
        }
    }
    Json(serde_json::json!({ "status": "ok", "seeded": count }))
}

async fn batch_teach(State(app): State<Arc<App>>, body: String) -> Json<serde_json::Value> {
    let mut orch = app.orch.lock().await;
    let before = orch.total_memory_count();
    let taught = orch.batch_teach_content(&body);
    let after = orch.total_memory_count();
    Json(serde_json::json!({
        "status": "ok",
        "taught": taught,
        "before": before,
        "after": after
    }))
}

fn file_size(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

async fn data_size(State(app): State<Arc<App>>) -> Json<DataSizeRes> {
    let (dir, memory_count) = {
        let orch = app.orch.lock().await;
        (orch.data_dir().to_string(), orch.total_memory_count())
    };
    let base = std::path::Path::new(&dir);
    let language_bin_bytes = file_size(&base.join("language.bin"));
    let routes_bin_bytes = file_size(&base.join("routes.bin"));
    let evolution_json_bytes = file_size(&base.join("evolution.json"));
    let evolution_chain_json_bytes = file_size(&base.join("evolution_chain.json"));
    let promoted_rules_json_bytes = file_size(&base.join("promoted_rules.json"));
    let pouches_json_bytes = file_size(&base.join("pouches.json"));
    let total_bytes = language_bin_bytes
        + routes_bin_bytes
        + evolution_json_bytes
        + evolution_chain_json_bytes
        + promoted_rules_json_bytes
        + pouches_json_bytes;
    Json(DataSizeRes {
        data_dir: dir,
        language_bin_bytes,
        routes_bin_bytes,
        evolution_json_bytes,
        evolution_chain_json_bytes,
        promoted_rules_json_bytes,
        pouches_json_bytes,
        total_bytes,
        memory_count,
    })
}

async fn dashboard(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    let mut m = app.monitor.lock().await;
    let metrics = match m.check() {
        resource_monitor::Status::Ok(m)
        | resource_monitor::Status::Warn(m, _)
        | resource_monitor::Status::Critical(m, _) => m,
    };
    drop(m);
    let orch = app.orch.lock().await;
    let pouches_data: Vec<serde_json::Value> = orch.pouches_detail()
        .into_iter()
        .map(|(name, role, memory, awake, _, atoms)| {
            serde_json::json!({"name": name, "role": role, "memory": memory, "awake": awake, "atoms": atoms})
        })
        .collect();
    let (evo_total, evo_promoted, evo_candidates, evo_l2) = orch.evolution_info();
    let evo_records: Vec<serde_json::Value> = orch.evolution_records_snapshot()
        .into_iter()
        .take(12)
        .map(|(pouch, vc, promoted, last)| {
            serde_json::json!({"pouch": pouch, "v": vc, "p": promoted, "t": last})
        })
        .collect();
    let events = orch.recent_events();
    let fb = orch.language_feedback_stats();
    let atoms = orch.capabilities_info();
    let mut kinds: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for a in &atoms {
        *kinds.entry(format!("{:?}", a.kind)).or_insert(0) += 1;
    }
    let (cfg_b, cfg_l, cfg_p) = orch.routing_config_snapshot();
    let ls = orch.learning_snapshot();
    Json(serde_json::json!({
        "v": frozen::bedrock::VERSION,
        "cpu": metrics.cpu,
        "mem": metrics.mem_mb,
        "temp": metrics.temp_est,
        "ready": orch.is_ready(),
        "pouches": pouches_data,
        "evo": {"total": evo_total, "promoted": evo_promoted, "candidates": evo_candidates, "l2": evo_l2, "threshold": 100u32, "records": evo_records},
        "events": events,
        "fb": {"misses": fb.0, "log": fb.1, "absorbed": fb.2, "net": fb.3},
        "memory": orch.total_memory_count(),
        "atoms": {"total": atoms.len(), "kinds": kinds},
        "cfg": {"baseline": cfg_b, "low": cfg_l, "promote": cfg_p},
        "learn": {
            "cycle": ls.cycle_count,
            "phase": ls.phase,
            "ext_fed": ls.external_fed,
            "ext_absorbed": ls.external_absorbed,
            "syn_fed": ls.cross_fed,
            "syn_absorbed": ls.cross_absorbed,
            "saturation": ls.saturation,
            "last_ts": ls.last_cycle_ts,
            "cloud_pulled": ls.cloud_pulled,
            "cloud_pushed": ls.cloud_pushed,
            "cloud_cursor": ls.cloud_sync_cursor
        }
    }))
}

async fn learning_state(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    let orch = app.orch.lock().await;
    let ls = orch.learning_snapshot();
    let (pattern_count, pouch_count, avg_maturity) = orch.learning_metrics_extra();
    Json(serde_json::json!({
        "cycle_count": ls.cycle_count,
        "cross_fed": ls.cross_fed,
        "cross_absorbed": ls.cross_absorbed,
        "saturation": ls.saturation,
        "pattern_count": pattern_count,
        "pouch_count": pouch_count,
        "new_pouches": 0,
        "avg_maturity": avg_maturity
    }))
}
