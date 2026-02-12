# 方案一：学习周期内自动吸纳 miss

**目标**：在 `autonomous_learning_cycle` 的 miss_driven 分支中，当 language 对 miss 输入返回 fallback 时，自动 `teach(miss_input, fallback_response)`，将该 miss 转为可命中 pattern。

**执行要求**：完成代码修改后，必须按「完成后自检」清单逐项执行并通过，否则视为未完成。

**修改文件**：`src/orchestrator.rs`

**修改位置**：`autonomous_learning_cycle` 内，`if !misses.is_empty()` 分支，约 1984–1994 行。

**当前逻辑**：
```rust
if let Ok(ref lang_out) = lang_result {
    if lang_out.len() > 20 && !self.language.is_fallback_response(lang_out) {
        let tokens = self.language.tokenize(miss_input);
        let broadcast = vec![(tokens, lang_out.clone(), 1.3)];
        for (_, pouch) in self.pouches.iter_mut() {
            pouch.sync_patterns(&broadcast);
        }
    }
}
```

**修改后逻辑**：当 `lang_out` 为 fallback 时补充 teach，非 fallback 时保持 sync_patterns：
```rust
if let Ok(ref lang_out) = lang_result {
    if lang_out.len() > 20 {
        if self.language.is_fallback_response(lang_out) {
            self.language.teach(miss_input, lang_out);
        } else {
            let tokens = self.language.tokenize(miss_input);
            let broadcast = vec![(tokens, lang_out.clone(), 1.3)];
            for (_, pouch) in self.pouches.iter_mut() {
                pouch.sync_patterns(&broadcast);
            }
        }
    }
}
```

**行为说明**：
- `is_fallback_response` 为 true：表示 miss，执行 `teach` 将 (miss_input, lang_out) 写入 pattern；
- `is_fallback_response` 为 false：表示已有 pattern 命中，保持原 `sync_patterns` 广播逻辑。

---

## 完成后自检（必须全部通过才视为完成）

1. **仅修改 orchestrator.rs**：用 `git diff src/orchestrator.rs` 确认无其他文件被改动。
2. **diff 范围正确**：diff 仅包含上述逻辑替换，无新增注释、TODO、`#[allow]`。
3. **构建通过**：`cargo build 2>&1` 无错误。
4. **逻辑核对**：再次对照「修改后逻辑」代码块，确认 `if/else` 分支与文档一致，未遗漏 `sync_patterns` 的 `else` 分支。

**自检命令**（修改完成后执行并确认无异常）：
```bash
cd /Users/will/Desktop/logos
git diff src/orchestrator.rs
cargo build 2>&1
```
