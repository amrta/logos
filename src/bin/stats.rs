use clap::Parser;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::exit;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "data/eval_result.jsonl")]
    input: String,

    #[arg(long, default_value = "0.60")]
    min_hit_rate: f64,

    #[arg(long)]
    strict: bool,
}

fn is_valid_hit_strict(output: &str) -> bool {
    let templates = [
        "是的，我还在",
        "我在的",
        "您好，我在听",
    ];
    if templates.iter().any(|t| output.contains(t)) {
        return false;
    }
    if output.contains("我的模式库里没有") || output.contains("没有匹配的模式") {
        return false;
    }
    if output.len() < 10 {
        return false;
    }
    true
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let file = File::open(&args.input).map_err(|e| format!("打开失败 {}: {}", args.input, e))?;
    let reader = BufReader::new(file);

    let mut total = 0u64;
    let mut hits = 0u64;
    let mut hits_strict = 0u64;
    let mut fallback = 0u64;
    let mut identity = 0u64;
    let mut template = 0u64;

    for line in reader.lines() {
        let line = line.map_err(|e| format!("读行失败: {}", e))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let val: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("JSON 解析失败: {}", e))?;
        total += 1;

        let output = val["logos_output"].as_str().unwrap_or("");
        let raw_hit = val["hit"].as_bool().unwrap_or(false);
        if raw_hit {
            hits += 1;
        }
        if args.strict && raw_hit && is_valid_hit_strict(output) {
            hits_strict += 1;
        } else if !args.strict && raw_hit {
            hits_strict += 1;
        }

        if output.contains("我的模式库里没有") || output.contains("没有匹配的模式") {
            fallback += 1;
        }
        if output.contains("文心一言") || output.contains("百度") {
            identity += 1;
        }
        if output.contains("是的，我还在") {
            template += 1;
        }
    }

    if total == 0 {
        eprintln!("无有效行");
        exit(1);
    }

    let total_f = total as f64;
    let hit_rate = if args.strict {
        hits_strict as f64 / total_f
    } else {
        hits as f64 / total_f
    };
    let fallback_pct = fallback as f64 / total_f * 100.0;
    let identity_pct = identity as f64 / total_f * 100.0;
    let template_pct = template as f64 / total_f * 100.0;

    println!("Total: {}", total);
    if args.strict {
        println!(
            "Hits (strict): {} ({:.1}%)",
            hits_strict,
            hit_rate * 100.0
        );
    } else {
        println!(
            "Hits: {} ({:.1}%)",
            hits,
            hit_rate * 100.0
        );
    }
    println!(
        "Explicit fallback: {} ({:.1}%)",
        fallback, fallback_pct
    );
    println!(
        "Identity drift: {} ({:.1}%)",
        identity, identity_pct
    );
    println!(
        "Template yes_still: {} ({:.1}%)",
        template, template_pct
    );
    println!();

    if hit_rate >= args.min_hit_rate {
        println!(
            "✅ Hit rate {:.1}% meets threshold {:.1}%",
            hit_rate * 100.0,
            args.min_hit_rate * 100.0
        );
        exit(0);
    } else {
        println!(
            "❌ Hit rate {:.1}% below threshold {:.1}%",
            hit_rate * 100.0,
            args.min_hit_rate * 100.0
        );
        exit(1);
    }
}
