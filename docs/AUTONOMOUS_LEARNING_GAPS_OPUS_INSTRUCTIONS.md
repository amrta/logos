# Logos 自主学习缺口修复指令（交付 Opus 4.6）

**项目路径**: `/Users/will/Desktop/logos`  
**约束**: 遵循 `.cursorrules`、`docs/POUCH_LAYER_PLANNING.md`；禁止注释/TODO/mock；每次修改需提供可复现的验证命令（仅输出 diff）。

---

## 缺口总览

| ID | 缺口 | 影响 | 优先级 |
|----|------|------|--------|
| G1 | 突触互学永不触发 | synapse 阶段条件过严，cross_fed 恒为 0 | P0 |
| G2 | 云端 Worker 无完整学习 | 仅有 pattern 采纳，无成熟度/突触 | P0 |
| G3 | 自我优化/对标需手动 | 未在周期内自动触发 | P1 |
| G4 | 意图库固定 | learning_intents_for 仅 ~20 条/尿袋 | P2 |
| G5 | 无外部知识摄入 | 仅语言袋自产自销 | P2 |

---

## G1：修复突触互学触发条件

**现状**：`autonomous_learning_cycle` 进入 synapse 需 `hungry.is_empty() || saturation >= 0.65`。saturation 公式为 `(1 - absorbed/fed).clamp(0,1)`，当 absorbed > fed 时恒为 0，导致长期停留在 miss_driven，cross_fed 恒为 0。

**修改文件**：`src/orchestrator.rs`

**要求**：
1. 增加「周期轮替」机制：当 `cycle_count % N == 0` 时强制进入 synapse 阶段一次（N 建议 5 或 10），与 saturation 条件取并集。
2. 或：当 `teachers.len() > 0 && students.len() > 0` 时，允许在 saturation < 0.65 情况下以较低频率（如每 3 周期 1 次）执行 synapse。
3. 保证 synapse 分支在「有教师且有学生」时有机会执行，不能仅依赖 saturation 和 hungry。

**验证**：修改后运行 `cargo run`，观察 `data/learning_state.json` 中 `cross_fed` 在若干周期后是否增加；`phase` 应出现 `synapse`。

---

## G2：云端 Worker 自主学习补全

**现状**：Worker 的 `handleAutoTrainStep` 仅将 `learned_patterns` 中 status='pending' 的写入 `language_pairs`，无 maturity、无突触、无 autonomous_learning_cycle。

**修改文件**：`worker.js`

**要求**：
1. 当 `env.LOGOS_BACKEND` 存在时：在 `handleAutoTrainStep` 内，除现有 pattern 采纳逻辑外，增加对后端的 `POST {backend}/api/chat` 调用，message 为 `自主学习`（与 Rust 端需新增对应快捷命令）。
2. Rust 端 `src/orchestrator.rs`：在 `execute_with_pouch` 的 lower 判断中，增加 `"自主学习"` 或 `"autonomous learn"` 分支，直接调用 `autonomous_learning_cycle().await` 并返回简要结果（周期数、phase、fed 等）。
3. Worker 端可选：增加定时 Cron 或 Alarm，定期调用 `handleAutoTrainStep` 或上述 backend 学习触发，使云端在无用户请求时也能推进学习。
4. 若 backend 不可用，Worker 保持现有 pattern 采纳行为，不报错。

**验证**：设置 `LOGOS_BACKEND` 后，对 Worker 发 `POST /internal/auto-train-step` 或 `/activate`，检查返回中是否包含学习触发结果；本地 `learning_state.json` cycle_count 应增加。

---

## G3：学习周期内自动触发自我优化与对标

**现状**：`自我优化`、`对标` 需用户显式输入命令。学习周期未自动调用。

**修改文件**：`src/orchestrator.rs`、`src/main.rs`

**要求**：
1. 在 `autonomous_learning_cycle` 末尾（`save_state` 之前），增加条件触发：
   - 每 `C` 个周期（建议 C=20）调用一次 `self_optimize()`；
   - 每 `E` 个周期（建议 E=30）调用一次 `evolve()`。
2. 使用 `cycle_count % C == 0` 与 `cycle_count % E == 0` 判断，且 C、E 取互质或错开，避免同周期重复。
3. `self_optimize` 和 `evolve` 为 async，需 `.await`；若失败，记录 log 或 event，不中断 cycle。
4. 在 `log_event` 中记录 `SELF_OPT_AUTO`、`EVOLVE_AUTO` 等事件，便于观察。

**验证**：`cargo run` 后等待 20～30 个学习周期（约 30～45 分钟），查 `GET /api/events` 或事件流，应出现 `SELF_OPT_AUTO`、`EVOLVE_AUTO`。

---

## G4：意图库多样性扩展

**现状**：`learning_intents_for` 返回固定中文 prompt 数组，按 cycle 取模，多样性有限。

**修改文件**：`src/orchestrator.rs`

**要求**：
1. 将各 pouch 的 intent bank 从 ~6 条扩展到至少 15～20 条，覆盖更多领域与句式。
2. 增加「组合生成」：在部分 cycle 中，用 `{领域}+{动作}+{主题}` 模板随机组合（如「解释 X 的原理」「比较 X 和 Y」「实现 X 的步骤」），其中 X/Y 从固定词表取样，词表至少 20 项。
3. 不引入外部 LLM 调用做意图生成（保持低熵）；仅用纯逻辑与固定词表。
4. `_` 默认分支的兜底 intent 也扩充 2～3 倍。

**验证**：运行若干 cycle，通过 log 或 debug 输出观察投喂的 intent 字符串是否多样化，无重复循环感。

---

## G5：外部知识摄入（可选，P2）

**现状**：外部 phase 仅用语言袋输出作为投喂来源，无文档、网页等外部输入。

**修改文件**：`src/orchestrator.rs`、`data/` 或新增 `data/external_seeds/`、可能的新 RemotePouch

**要求**（最小可行）：
1. 在 `data/external_seeds/` 下新增 JSON 或 JSONL 文件，格式如 `[{"intent":"...", "response":"..."}]`，存放静态种子对（可从项目 docs、README 提炼）。
2. `autonomous_learning_cycle` 的 external 分支中，当 `misses.is_empty()` 且 `hungry` 非空时，以一定概率（如 20%）从 `external_seeds` 读取一条，作为 `(intent, response)` 投喂，替代本次从 language 生成的流程。
3. 加载逻辑为纯 IO，失败时静默回退到原有 language 生成路径。
4. 不实现网络爬取；仅本地文件。为后续扩展预留接口（如 `load_external_seeds() -> Vec<(String,String)>`）。

**验证**：添加 `data/external_seeds/seed.json` 后运行，确认部分 cycle 的投喂来源为文件内容；可用 event 或计数器验证。

---

## 实施顺序建议

1. **G1**（突触触发）— 立即见效，修复 cross_fed=0
2. **G2**（Worker 学习）— 云端/后端联动
3. **G3**（自动自优化/对标）— 增强闭环
4. **G4**（意图扩展）— 提升多样性
5. **G5**（外部种子）— 可选扩展

---

## 风险预演（改动前需确认）

| 改动 | 潜在风险 | 缓解 |
|------|----------|------|
| G1 | synapse 执行过频导致负载上升 | 用周期取模限制频率 |
| G2 | Worker 频繁调后端增加延迟 | 合并到现有时机，不新增高频轮询 |
| G3 | self_optimize/evolve 耗时 | 在 cycle 末尾顺序执行，必要时加超时 |
| G4 | 组合意图语义不连贯 | 词表与模板需人工审核 |
| G5 | 种子文件格式错误 | 解析失败时回退，不打日志轰炸 |

---

## 验证命令汇总

```bash
# 本地完整验证
cd /Users/will/Desktop/logos
cargo build 2>&1
cargo run &
sleep 3600
curl -s http://127.0.0.1:3000/api/status | jq .
curl -s http://127.0.0.1:3000/api/events | jq '.events[-20:]'
cat data/learning_state.json

# 检查 cross_fed、phase、cycle_count
jq '.' data/learning_state.json
```

所有改动完成后，`learning_state.json` 中 `cross_fed` 应大于 0，`phase` 应间歇出现 `synapse`；事件流中应出现 `SELF_OPT_AUTO`、`EVOLVE_AUTO`（若 G3 已实施）。
