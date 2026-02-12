use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};

const MIN_HUMAN: usize = 5;
const MAX_HUMAN: usize = 500;
const MIN_GPT: usize = 1;
const MAX_GPT: usize = 3000;
const DEFAULT_MAX_ADD: usize = 5000;

fn is_fallback_output(s: &str) -> bool {
    s.contains("我的模式库里没有")
        || s.contains("没有匹配的模式")
        || s.contains("我暂时无法处理")
}

fn has_domain_keyword(s: &str) -> bool {
    let keywords = [
        "解释", "怎么", "什么", "如何", "为什么", "治疗", "医学", "代码", "编程", "学习", "教育",
        "量子", "物理", "化学", "数学", "历史", "文化", "艺术", "心理", "经济", "法律",
    ];
    keywords.iter().any(|k| s.contains(k))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let eval_path = args.next().unwrap_or_else(|| "data/eval_result.jsonl".into());
    let sharegpt_path = args.next().unwrap_or_else(|| "Old/sharegpt_pairs.zh.simplified_native.jsonl".into());
    let train_path = args.next().unwrap_or_else(|| "Old/data/cleaned_language_train.jsonl".into());
    let max_add: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_ADD);

    let mut fallback_inputs = HashSet::new();
    if let Ok(f) = File::open(&eval_path) {
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                let out = v.get("logos_output").and_then(|o| o.as_str()).unwrap_or("");
                if is_fallback_output(out) {
                    if let Some(inp) = v.get("input").and_then(|i| i.as_str()) {
                        fallback_inputs.insert(inp.to_string());
                    }
                }
            }
        }
    }
    eprintln!("fallback samples from eval: {}", fallback_inputs.len());

    let mut existing = HashSet::new();
    if let Ok(f) = File::open(&train_path) {
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(h) = v.get("human").and_then(|x| x.as_str()) {
                    existing.insert(h.to_string());
                }
            }
        }
    }
    eprintln!("existing train lines: {}", existing.len());

    #[derive(serde::Deserialize)]
    struct ShareLine {
        human: String,
        gpt: String,
    }

    let mut added = Vec::new();
    let sharegpt_file = File::open(&sharegpt_path).map_err(|e| format!("open {}: {}", sharegpt_path, e))?;
    for line in BufReader::new(sharegpt_file).lines().map_while(Result::ok) {
        if added.len() >= max_add {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let raw: ShareLine = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let human = raw.human.trim();
        let gpt = raw.gpt.trim();
        if human.len() < MIN_HUMAN || human.len() > MAX_HUMAN {
            continue;
        }
        if gpt.len() < MIN_GPT || gpt.len() > MAX_GPT {
            continue;
        }
        if existing.contains(human) {
            continue;
        }
        if !has_domain_keyword(human) {
            continue;
        }
        existing.insert(human.to_string());
        added.push(serde_json::json!({ "human": human, "gpt": gpt }));
    }

    if added.is_empty() {
        eprintln!("no new samples to add");
        return Ok(());
    }

    let mut out = BufWriter::new(File::options().append(true).open(&train_path)?);
    for row in &added {
        writeln!(out, "{}", serde_json::to_string(row)?)?;
    }
    out.flush()?;
    eprintln!("appended {} lines to {}", added.len(), train_path);
    Ok(())
}
