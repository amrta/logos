# 尿袋层规划（零熵约束内）

仅在 Pouch 层内规划，对应公式中 **∇K(x_t)**（尿袋层能力）与 **Vol(Ψ)→∞**（生成空间扩大）。不涉及 frozen / logic / 常量 / 路由。

---

## 整体与括号：四层是什么

**整体**：括号所包住的，是 **LOGOS** —— 在数学与几何约束下、把意图转化为可执行任务并闭环学习的系统。不是「四块拼在一起」，而是**一层层叠上去的单一系统**：内层是外层的底座，外层只许与相邻层交互。

**括号含义**：
- 最内层 **（公式层）**：不可变常数与规则，零熵、物理不可达。
- **（（公式层）+ 逻辑层）**：在公式之上只做路由与因果判定，只读、无熵。
- **（（（公式层）+ 逻辑层）+ 尿袋管理器）**：在逻辑之上做调度、协调、演化链与参数数学化；只保留机制，功能全在尿袋。
- **（（（（公式层）+ 逻辑层）+ 尿袋管理器）+ 尿袋层）**：在管理器之上做能力实现与尿袋间神经突触式学习（sync_patterns、learn_routing、evolution）。

相对开诚布公合作前的系统，当前在以下方面有实质提升：**管理器净化**（职能下放到尿袋、catalog 与纯函数）、**有熵/无熵软隔离**（性能自洽线头落地）、**管理器数学化**（参数几何、纯函数、自动调节）、**演化链记失败**、**语言袋接 sync_patterns**、**Pipeline 与云端 /learn、/pipeline、/feedback 等闭环**。整体更干净、可推演、可闭环。

---

## 0. 有熵/无熵尿袋软隔离（性能自洽预设）

尿袋管理层预设：**有熵袋与无熵/低熵袋软隔离**，为性能自洽要素。

- **无熵/低熵**：LanguagePouch（E0）、本地确定性执行；必须在本地、不阻塞、极低资源占用。
- **有熵**：E1/E2 中重计算、试错、大状态；优先云端或 RemotePouch，本地仅调用与结果封装。
- **软隔离**：不混用调度路径——本地核心保持零熵/低熵，高熵能力通过云端完成，与「第 0 层本地核心、第 1 层云端尾袋」及 M1 8G 不热不卡约束一致。bedrock 中 `MAX_ENTROPY_ALLOWED` 与逻辑层零熵为边界；管理层内可据此做调度/路由提示（仅尿袋层与管理器，不碰 frozen/logic）。

后续若在管理层显式落地（如 PouchMeta 标记、调度优先策略），均以此预设为线头。

---

## 0.1 尿袋管理器净化（仅机制、仅协调）

尿袋管理器（Orchestrator）只保留**机制**，不承担直接职能；功能实现全部下放到尿袋，形成「尿袋层神经突触互相学习进化」的闭环。

- **调度**：谁在何时跑（call_pouch、execute_plan、run_pipeline、install/uninstall、sleep/wake）。
- **协调**：尿袋间神经网络式协调——sync_patterns 广播、evolution 记录、learn_routing、recent_evolution_entries 与 score 用于路由/晋升；Reject 时 try_fallback_chain 按序尝试尿袋。
- **无直接职能**：不解析业务输出、不实现领域规则。fallback 判定由各袋 `is_fallback_output`；演化差距解析由 capability_comparer 的 `evolution_gaps_from_output`；补齐建议由 defect_scanner 的 `recommended_follow_ups`；语言评估/导入/回滚/导出由 LanguagePouch 的 `eval_from_path`、`import_from_content`、`rollback_from`、`export_summary`；尿袋名→实例由 `pouch_catalog::instantiate`。管理器只做「调用尿袋 + 写链/保存状态」。

---

## 0.2 尿袋管理器数学化（低熵闭环）

管理器参数与评分用**纯函数 + 几何边界**表达，实现低熵闭环。

- **参数几何**：`manager_math::RoutingParamsBounds` 定义 baseline、low_threshold、promote_min_chain 的 [min, max]；所有读到的配置经 `clamp_routing_params` 落入闭区间后再参与计算。
- **纯函数**：`score_from_evolution_stats(matching_count, total_output_len, baseline) → score`；`promote_eligible(score, min_chain) → bool`。评分与晋升判定无副作用，仅依赖输入与已 clamp 的参数。
- **自动调节**：`adjusted_baseline(current, success_rate, bounds, step)` 根据演化链成功率在边界内微调 baseline；`maybe_adjust_baseline` 在 record_evolution 后若链长 ≥ 50 则更新 config 并写回，形成参数自洽闭环。

---

## 1. 能力声明与 μ_π_j 可被选中

- **现状**：各 pouch 实现 `atom_capabilities()`，返回 `Vec<AtomDeclaration>`（name, kind, pouch, confidence_range）。
- **规划**：确保每个已实现 pouch 的 `atom_capabilities()` 与其实质能力一致，覆盖其实际能完成的 AtomKind（Transform / Match / Score / Generate / Validate / Route），且 `confidence_range` 与当前实现相符。这样上层对 kind 的筛选才能正确选到对应的 μ_π_j。
- **范围**：仅修改/补全各 `pouch_*.rs` 与 `language_pouch.rs` 中的 `atom_capabilities()` 实现，不改 registry 或路由逻辑。

---

## 2. 单 pouch 输出空间 Vol(·) 的扩大

- **现状**：`process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String>`，返回 `PouchOutput { data, confidence }`。
- **规划**：在不变更 trait 签名的前提下，在各 pouch 内部通过已有机制扩大可输出的 (data, confidence) 集合，例如：
  - 使用 `sync_patterns` 注入的模式增加对同一 proposal 的不同响应；
  - 使用 pouch 内部状态（如 memory、缓存）使相同输入在不同上下文中产生不同输出；
  - RemotePouch 通过调用更多样化的远端实现，扩大返回的 data 分布。
- **范围**：仅改动各 pouch 的 `process_proposal` 实现及 pouch 内部状态/调用方式，不增加新 trait 方法、不碰 orchestrator 或 logic。

---

## 3. 通过新增尿袋增加 μ_π_j 的个数

- **现状**：`CapabilityRegistry` 按 pouch 注册 atom；每个 pouch 对应一组 μ_π_j。
- **规划**：在尿袋层增加新 pouch 类型（实现 `Pouch` trait），或增加现有 RemotePouch 的实例并正确实现 `atom_capabilities()`，使可选的 μ_π_j 增多，从而在不变 σ_seed、不改路由与常量的前提下，让 ⊕ 链上可组合的尿袋能力更多，Vol(Ψ) 增大。
- **范围**：仅新增或扩展 `src/pouch_*.rs`、`remote_pouch.rs` 及在应用侧注册新 pouch 的代码；不修改 frozen、logic 或 layer guard。

---

## 4. 云端计算与 RemotePouch

- **现状**：RemotePouch 通过 HTTP 调用远端，返回 result/output；Worker 端已有 seed、enhance、expand 等能力。
- **规划**：将「重计算、大状态、可扩展能力」放在云端，本地仅保留 RemotePouch 的调用与结果封装；新增领域能力时，以新 endpoint + 新 RemotePouch 实例 + 对应 `atom_capabilities()` 的方式接入，计算全部在云端完成。
- **范围**：仅涉及 RemotePouch 的 endpoint 配置、以及云端 Worker 上实现的具体能力；不改变 RemotePouch 的 trait 或与 orchestrator 的接口。

---

## 5. 与公式的对应关系（仅作对齐说明，不要求改公式或常量）

- **σ_seed**：由上层/种子侧保证 dim→0；尿袋层不实现、不修改种子或常量。
- **∇K(x_t)**：由各 pouch 的 `atom_capabilities()` 与 `process_proposal` 共同实现；规划 1、2、3、4 均只在此层增强。
- **Vol(Ψ)→∞**：通过更多 μ_π_j（更多/更准的 capability 声明）、更丰富的单 pouch 输出、以及云端扩展，在尿袋层内逐步扩大可生成空间。

---

以上全部为尿袋层内可实现、且不触碰 frozen / logic 的进一步规划。
