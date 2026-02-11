use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};

const MIN_HUMAN: usize = 5;
const MAX_HUMAN: usize = 500;
const MIN_GPT: usize = 1;
const MAX_GPT: usize = 3000;

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .trim()
        .to_string()
}

fn is_mostly_cjk(s: &str) -> bool {
    let cjk: usize = s.chars().filter(|c| c.is_alphabetic() && !c.is_ascii_alphanumeric()).count() + s.chars().filter(|c| *c >= '\u{4e00}' && *c <= '\u{9fff}').count();
    let total: usize = s.chars().filter(|c| !c.is_whitespace()).count();
    total == 0 || (cjk as f64 / total as f64) >= 0.3
}

#[derive(Deserialize)]
struct Line {
    human: String,
    gpt: String,
}

#[derive(Serialize)]
struct OutLine {
    human: String,
    gpt: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let input_path = args.get(1).map(|s| s.as_str()).unwrap_or("sharegpt_pairs.zh.simplified_native.jsonl");
    let output_path = args.get(2).map(|s| s.as_str()).unwrap_or("data/cleaned_language.jsonl");
    let split_ratio: f64 = args
        .iter()
        .find(|a| a.starts_with("--split="))
        .and_then(|a| a.strip_prefix("--split=").and_then(|s| s.parse().ok()))
        .unwrap_or(0.0);

    let f = File::open(input_path)?;
    let reader = BufReader::new(f);
    let mut rows: Vec<OutLine> = Vec::new();
    let mut seen = HashSet::new();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let raw: Line = serde_json::from_str(line).map_err(|e| format!("{}: {}", line.get(..80).unwrap_or(line), e))?;
        let human = raw.human.trim();
        let gpt = strip_html(&raw.gpt);
        if human.len() < MIN_HUMAN || human.len() > MAX_HUMAN {
            continue;
        }
        if gpt.len() < MIN_GPT || gpt.len() > MAX_GPT {
            continue;
        }
        if !is_mostly_cjk(human) {
            continue;
        }
        let key = human.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        rows.push(OutLine {
            human: human.to_string(),
            gpt,
        });
    }

    let count = rows.len();
    if split_ratio > 0.0 && split_ratio < 1.0 && !rows.is_empty() {
        let split_at = (rows.len() as f64 * (1.0 - split_ratio)).round() as usize;
        let split_at = split_at.min(rows.len().saturating_sub(1));
        let (train, test) = rows.split_at(split_at);
        let train_path = output_path.replace(".jsonl", "_train.jsonl");
        let test_path = output_path.replace(".jsonl", "_test.jsonl");
        let mut w = BufWriter::new(File::create(&train_path)?);
        for r in train {
            writeln!(w, "{}", serde_json::to_string(r)?)?;
        }
        w.flush()?;
        let mut w = BufWriter::new(File::create(&test_path)?);
        for r in test {
            writeln!(w, "{}", serde_json::to_string(r)?)?;
        }
        w.flush()?;
        eprintln!("cleaned {} -> train {} ({}), test {} ({} lines)", input_path, train_path, train.len(), test_path, test.len());
    } else {
        let mut writer = BufWriter::new(File::create(output_path)?);
        for r in &rows {
            writeln!(writer, "{}", serde_json::to_string(r)?)?;
        }
        writer.flush()?;
        eprintln!("cleaned {} -> {} ({} lines)", input_path, output_path, count);
    }
    Ok(())
}
