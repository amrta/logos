use std::time::Instant;

fn main() {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("runtime: {}", e);
            std::process::exit(1);
        }
    };
    rt.block_on(async {
        let n = 100usize;
        let start = Instant::now();
        for _ in 0..n {
            tokio::task::yield_now().await;
        }
        let avg_us = start.elapsed().as_micros() / n as u128;
        println!(
            "async yield_now avg: {} µs ({} calls) — 用于验证 runtime 可用",
            avg_us, n
        );
    });
}
