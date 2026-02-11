# LOGOS

零熵、分层验证、确定性的任务转化与执行系统。根目录保持干净，迭代可评估、可回顾。

---

## 0. 当前状态与文档索引

| 资源 | 说明 |
|------|------|
| **src/** | 核心代码（frozen / orchestrator / pouch / bin） |
| **ui/** | Web UI（index.html） |
| **worker.js** | Cloudflare Worker 入口 |
| **scripts/** | 部署、训练、Worker 相关脚本 |
| **docs/** | 所有报告与设计文档（见下表） |
| **data/** | 运行时与评估数据（eval_result.jsonl、language.bin 等）；基线 train/eval 用 jsonl 需自备或从备份恢复 |

### docs 关键文档

| 文档 | 用途 |
|------|------|
| **docs/BASELINE_REPORT.md** | 正式基线评估报告；每轮迭代对比此基线，量化进步 |
| **docs/OLD_GETOUT_REUSE_AND_BLUEPRINT.md** | 复用审计、蓝图、里程碑、分层设计、风险、迭代路线（历史归档已清理） |
| **docs/COMPREHENSIVE_ASSESSMENT_AND_PLAN.md** | 综合评估与计划 |
| **docs/LOGOS_DESIGN_COMPLETE.md** | 架构与常数定义 |
| **docs/UNFINISHED_CHECKLIST.md** | 未完成功能清单 |
| **docs/LONG_TERM_TECH_REFERENCE.md** | 长期技术储备（New 同源） |

---

## 1. 主目标（固定）

- **A**：根目录干净、结构稳定、可维护。
- **B**：能力提升经统一评估链路验证（见 BASELINE_REPORT）。
- **C**：路线图可追踪，随时能回答「现在在哪、下一步做什么」。

---

## 2. 执行阶段路线图

### Phase 1：稳定基线 ✅ 完成（95% 基础设施 + 性能优化）

- [x] 目录治理（Old / getout / docs）
- [x] 可编译可测试（48/48）
- [x] 第一轮基线评估与报告（Round 1）
- [x] 异步重构（Pouch 全链路 async，见 `docs/ASYNC_ARCHITECTURE_OPTIONS.md`）
- [x] 评估统计工具 `stats`（`cargo build --bin stats`，`./target/debug/stats --min-hit-rate 0.65`）
- [x] 固化「每次变更必复测」流程（命令见 3.2，报告见 BASELINE_REPORT）

**验收**：`cargo build --bin logos`、`cargo test` 通过；`docs/BASELINE_REPORT.md` 可复现；`cargo clippy -- -D clippy::unwrap_used` 零 warning；可选 `./target/debug/stats --input data/eval_result.jsonl --min-hit-rate 0.65`。

### Phase 2：质量提升（1–2 周）⏳ 准备启动

- [x] 降低模板化回复占比（如「是的，我还在…」）（Round 2 已为 0）
- [x] 消除身份漂移（文心一言等 = 0）（Round 2 已为 0）
- [x] 强化未命中回退策略（3 级 fallback：language → reasoning → creative）
- [x] 增加高频场景样本覆盖（augment_training_data + sharegpt）
- [ ] Round 3 命中率 ≥ 65% 复现验证

**验收**：命中率 ≥ 72%；显式回退 ≤ 30；身份漂移 = 0。

### Phase 3：路由与扩展（2–4 周）

- [ ] Orchestrator 路由优化
- [ ] 尿袋安装与能力注册增强
- [ ] 本地/云端边界与 Worker 契约文档
- [ ] 远程调用纯异步化（消除 block_on 嵌套）

**验收**：关键场景回归通过；远程可降级、可观测。

### Phase 4：工程化闭环（4–8 周）

- [ ] 部署脚本 preflight、分步、可回滚
- [ ] CI 评估门禁（指标不达标阻断合并）
- [ ] 版本对照报告（每轮指标与风险）
- [ ] 知识增长计划（按领域滚动扩容）

**验收**：每次改动有评估报告；可回看「改了什么、提升了什么、风险是什么」。

---

## 3. 每次迭代必做

**3.1 本地验证**

```bash
cd /Users/will/Desktop/logos
cargo build --bin logos
cargo test
```

**3.2 基线评估（统一口径）**

```bash
LOGOS_DATA=./data ./target/debug/logos \
  --import data/cleaned_language_train.jsonl \
  --eval  data/cleaned_language_test.jsonl
./target/debug/stats --input data/eval_result.jsonl --min-hit-rate 0.65
```
（若使用其他路径的 train/eval jsonl，将上述路径替换即可。）

**3.3 更新报告**

- 更新 `docs/BASELINE_REPORT.md`（表 4、表 6 与历史 Round）。
- 若路线或阶段有变，更新本 README 的勾选与验收说明。

---

## 4. 回顾机制

- **每日**：当日里程碑是否完成、是否引入新风险。
- **每周**：指标是否改善、是否偏离路线图。
- **每轮**：回答——解决了什么、指标提升多少、下轮最大阻塞是什么。

---

## 5. 目录与路线图规则

- **生产**：仅 `src/`、`ui/`、`scripts/`、`worker.js`、`wrangler.toml`、`Cargo.toml` 等必要文件在根目录。
- **文档**：仅 `docs/`；历史与审计在 Old，无用沉积在 getout。
- **路线图**：README 不删历史阶段，只做增量更新（勾选、验收、下轮行动）。

本 README 为项目总控；推进与回顾均以本文档及 `docs/BASELINE_REPORT.md`、`docs/OLD_GETOUT_REUSE_AND_BLUEPRINT.md` 为准。
