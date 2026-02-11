fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (in_path, out_path) = match args.len() {
        2 => (args[1].as_str(), None),
        3 => (args[1].as_str(), Some(args[2].as_str())),
        _ => {
            eprintln!("用法: filter_zh_simplified <input.jsonl> [output.jsonl]");
            std::process::exit(1);
        }
    };
    let content = match std::fs::read_to_string(in_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("读取失败 {}: {}", in_path, e);
            std::process::exit(1);
        }
    };
    let mut kept = 0usize;
    let mut dropped = 0usize;
    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(x) => x,
            Err(_) => {
                dropped += 1;
                continue;
            }
        };
        let human = match v.get("human").and_then(|h| h.as_str()) {
            Some(h) => h,
            None => {
                dropped += 1;
                continue;
            }
        };
        if has_japanese(human) || has_traditional(human) {
            dropped += 1;
            continue;
        }
        kept += 1;
        out.push(line);
    }
    let out_final = if out.is_empty() {
        String::new()
    } else {
        format!("{}\n", out.join("\n"))
    };
    match out_path {
        Some(p) => {
            if let Err(e) = std::fs::write(p, &out_final) {
                eprintln!("写入失败 {}: {}", p, e);
                std::process::exit(1);
            }
            eprintln!("保留 {} 行，剔除 {} 行 -> {}", kept, dropped, p);
        }
        None => {
            print!("{}", out_final);
            eprintln!("保留 {} 行，剔除 {} 行", kept, dropped);
        }
    }
}

fn has_japanese(s: &str) -> bool {
    s.chars().any(|c| {
        let u = c as u32;
        (0x3040..=0x309F).contains(&u) || (0x30A0..=0x30FF).contains(&u)
    })
}

fn has_traditional(s: &str) -> bool {
    const TRAD: &[u32] = &[
        0x500B, 0x5011, 0x5099, 0x50B3, 0x50C5, 0x50F9, 0x512A, 0x554F, 0x5B78, 0x5BEB, 0x5C0D,
        0x6642, 0x6703, 0x689D, 0x696D, 0x6A02, 0x6A23, 0x6A5F, 0x6A19, 0x6AA2, 0x6B0A, 0x70BA,
        0x7121, 0x767C, 0x7D93, 0x7E3D, 0x806F, 0x8166, 0x8207, 0x8209, 0x820A, 0x83EF, 0x85DD,
        0x85E5, 0x898B, 0x898F, 0x89C0, 0x8A08, 0x8A0A, 0x8A31, 0x8A71, 0x8A72, 0x8A9E, 0x8AB0,
        0x8ABF, 0x8ACB, 0x8AD6, 0x8B1D, 0x8B58, 0x8B70, 0x8B80, 0x8B8A, 0x8B93, 0x8AAA, 0x9019,
        0x904E, 0x9084, 0x958B, 0x9580, 0x9577, 0x96D6, 0x96DC, 0x96E2, 0x96E3, 0x96FB, 0x98A8,
        0x982D, 0x984C, 0x99AC, 0x9B5A, 0x9EBC, 0x9CE5, 0x9AD4, 0x9EDE, 0x9EBC, 0x9F8D, 0x570B,
        0x4F86, 0x66F8, 0x756B, 0x984C,
    ];
    s.chars().any(|c| TRAD.contains(&(c as u32)))
}
