/*
 * LOGOS 本地终端测试环境：仅调用本地代码，手动输入意图、手动观察变化，无任何 AI 造假。
 * 编译运行：cargo run --bin logos -- --terminal
 */
use crate::orchestrator::Orchestrator;
use std::io::{self, Write};

const DIR: &str = "/tmp/logos_manual_test";

fn trunc(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{}…", t)
    } else {
        t
    }
}

pub async fn run() {
    let _ = std::fs::remove_dir_all(DIR);
    std::fs::create_dir_all(DIR).ok();
    let mut orch = Orchestrator::new(DIR);
    orch.install("capability_comparer").ok();
    orch.install("pilot").ok();
    let mut stdout = io::stdout();
    writeln!(stdout, "LOGOS 本地终端 (data_dir={})", DIR).ok();
    writeln!(stdout, "命令: run <意图> | feed <json_path> | status | chain | score <pouch> | promote | reset | quit | help").ok();

    loop {
        write!(stdout, "> ").ok();
        stdout.flush().ok();
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() || line.is_empty() {
            continue;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
        let cmd = parts[0];
        let rest = parts.get(1).copied().unwrap_or("").trim();

        match cmd {
            "run" => {
                if rest.is_empty() {
                    writeln!(stdout, "用法: run <意图文本>").ok();
                    continue;
                }
                let chain_before = orch.recent_evolution_entries(100).len();
                let prom_before = orch.promoted_cache_len();
                let result = orch.execute_with_pouch(rest).await;
                let chain_after = orch.recent_evolution_entries(100).len();
                let prom_after = orch.promoted_cache_len();
                match &result {
                    Ok((body, pouch)) => {
                        writeln!(stdout, "路径: {}", path_label(pouch.as_str())).ok();
                        writeln!(stdout, "pouch: {}", pouch).ok();
                        writeln!(stdout, "输出: {}", trunc(body, 200)).ok();
                        writeln!(stdout, "chain {} -> {}", chain_before, chain_after).ok();
                        writeln!(
                            stdout,
                            "promoted_cache {} -> {} 新增: {}",
                            prom_before,
                            prom_after,
                            prom_after > prom_before
                        )
                        .ok();
                    }
                    Err((e, pouch)) => {
                        writeln!(stdout, "路径: {}", path_label(pouch.as_str())).ok();
                        writeln!(stdout, "错误: {} pouch={}", e, pouch).ok();
                    }
                }
            }
            "status" => {
                let n = orch.promoted_cache_len();
                writeln!(stdout, "promoted_cache.len() = {}", n).ok();
                let entries = orch.recent_evolution_entries(5);
                for e in &entries {
                    writeln!(
                        stdout,
                        "  {}  out_pre20: {}",
                        e.pouch_name,
                        trunc(&e.output_trunc, 20)
                    )
                    .ok();
                }
            }
            "chain" => {
                let entries = orch.recent_evolution_entries(5);
                for e in &entries {
                    writeln!(
                        stdout,
                        "  ts={} pouch={} in_pre20={} out_pre20={}",
                        e.timestamp,
                        e.pouch_name,
                        trunc(&e.input_trunc, 20),
                        trunc(&e.output_trunc, 20)
                    )
                    .ok();
                }
            }
            "score" => {
                if rest.is_empty() {
                    writeln!(stdout, "用法: score <pouch_name>").ok();
                    continue;
                }
                let s = orch.score_pouch(rest);
                writeln!(stdout, "score_pouch_from_entries(\"{}\") = {}", rest, s).ok();
            }
            "promote" => {
                let msgs = orch.run_promote_check();
                for m in msgs {
                    writeln!(stdout, "{}", m).ok();
                }
            }
            "feed" => {
                if rest.is_empty() {
                    writeln!(stdout, "用法: feed <path> [N]  path=JSON 数组或 JSONL(每行 {{\"human\":\"...\"}})  N=可选条数上限").ok();
                    continue;
                }
                let path = rest.trim();
                let (path, limit): (&str, Option<usize>) = match path.find(char::is_whitespace) {
                    Some(i) => {
                        let (p, tail) = path.split_at(i);
                        let n = tail.trim().parse::<usize>().ok();
                        (p.trim(), n)
                    }
                    None => (path, None),
                };
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        let intents: Vec<String> = match serde_json::from_str::<Vec<String>>(&content) {
                            Ok(v) => v,
                            Err(_) => {
                                let mut out = Vec::new();
                                for line in content.lines() {
                                    let line = line.trim();
                                    if line.is_empty() {
                                        continue;
                                    }
                                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                                        if let Some(s) = v.get("human").and_then(|h| h.as_str()) {
                                            out.push(s.to_string());
                                        }
                                    }
                                }
                                out
                            }
                        };
                        let intents: Vec<&str> = intents.iter().map(String::as_str).collect();
                        let intents: Vec<&str> = if let Some(n) = limit {
                            intents.into_iter().take(n).collect()
                        } else {
                            intents
                        };
                        if intents.is_empty() {
                            let _ = writeln!(stdout, "未解析到任何意图（需 JSON 数组或 JSONL 含 human 字段）");
                            continue;
                        }
                        writeln!(stdout, "批量喂养 {} 个意图...", intents.len()).ok();
                        for (i, intent) in intents.iter().enumerate() {
                            writeln!(stdout, "[{}] \"{}\"", i + 1, trunc(intent, 50)).ok();
                            let chain_before = orch.recent_evolution_entries(100).len();
                            let prom_before = orch.promoted_cache_len();
                            let result = orch.execute_with_pouch(intent).await;
                            let chain_after = orch.recent_evolution_entries(100).len();
                            let prom_after = orch.promoted_cache_len();
                            match &result {
                                Ok((body, pouch)) => {
                                    writeln!(
                                        stdout,
                                        "  路径: {}  pouch: {}  输出: {}",
                                        path_label(pouch.as_str()),
                                        pouch,
                                        trunc(body, 100)
                                    )
                                    .ok();
                                }
                                Err((e, pouch)) => {
                                    writeln!(stdout, "  错误: {}  pouch={}", e, pouch).ok();
                                }
                            }
                            writeln!(
                                stdout,
                                "  chain {} -> {}  promoted {} -> {}",
                                chain_before, chain_after, prom_before, prom_after
                            )
                            .ok();
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        }
                        writeln!(stdout, "批量喂养完成。").ok();
                    }
                    Err(e) => {
                        let _ = writeln!(stdout, "读取文件失败: {}", e);
                    }
                }
            }
            "reset" => {
                let _ = std::fs::remove_dir_all(DIR);
                std::fs::create_dir_all(DIR).ok();
                orch = Orchestrator::new(DIR);
                orch.install("capability_comparer").ok();
                orch.install("pilot").ok();
                writeln!(stdout, "已清空 {} 并重启 Orchestrator", DIR).ok();
            }
            "quit" => break,
            "help" => {
                writeln!(stdout, "run <意图>  - execute_with_pouch(意图)，打印路径/输出/chain/promoted 变化").ok();
                writeln!(stdout, "status      - promoted_cache.len + recent_evolution_entries(5) 摘要").ok();
                writeln!(stdout, "chain       - 最近 5 条 evolution_chain (ts, pouch, input_pre20, output_pre20)").ok();
                writeln!(stdout, "score <名>  - score_pouch_from_entries 值").ok();
                writeln!(stdout, "promote     - 手动跑一次晋升检查（仅检查与说明，实际晋升在 ToPouch record_evolution）").ok();
                writeln!(stdout, "feed <path> - 从 JSON 文件批量 run 意图列表（Vec<String>）").ok();
                writeln!(stdout, "reset       - 清空目录并重启").ok();
                writeln!(stdout, "quit        - 退出").ok();
                writeln!(stdout, "help        - 本列表").ok();
            }
            _ => {
                let _ = writeln!(stdout, "未知命令，输入 help 查看");
            }
        }
    }
}

fn path_label(pouch: &str) -> &'static str {
    match pouch {
        "plan" => "plan",
        "cloud_plan" => "cloud",
        "language" => "language",
        "system" => "system",
        _ => "ToPouch",
    }
}
