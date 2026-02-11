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
