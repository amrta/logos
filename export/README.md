# LOGOS 审计导出包

本目录为 LOGOS 核心内容导出，便于第三方评估或审计。

## 文件说明

| 文件 | 格式 | 用途 |
|------|------|------|
| **LOGOS_CORE_EXPORT.md** | 单文件 Markdown | **Claude 兼容**：可直接粘贴到 Claude 或上传，按路径分块包含全部核心代码与文档，便于对话式审计 |
| **LOGOS_CORE_EXPORT.tar.gz** | 压缩包 | 同一批文件的目录结构打包，便于本地浏览或版本管理 |

## 包含范围

- **根配置**: `Cargo.toml`, `wrangler.toml`, `README.md`, `.cursorrules`
- **Worker**: `worker.js`（Cloudflare Worker 入口，含 /verify/*、/feed/*、/train 等）
- **Rust 核心**: `src/main.rs`, `src/orchestrator.rs`, `src/language_pouch.rs`, `src/pouch_trait.rs`, `src/frozen/*`, 各 `pouch_*.rs`, `src/bin/*`（stats / augment_training_data / benchmark 等）
- **脚本**: `scripts/augment_training_data.mjs`, `scripts/worker_deploy_and_check.sh`, `scripts/language_train_and_eval.sh`
- **文档**: `docs/*.md`（基线报告、迁移计划、验证说明等）

不包含：`target/`、`node_modules/`、二进制数据（如 `*.bin`）、大体积 JSONL 原始数据。

## 使用方式（Claude）

1. 打开 **LOGOS_CORE_EXPORT.md**。
2. 全文复制，粘贴到 Claude 对话（或通过附件上传）。
3. 说明审计目标，例如：「请对 LOGOS 做安全/架构/代码质量审计」或「评估 orchestrator 与 language_pouch 的设计」。

## 重新生成

在项目根目录执行：

```bash
node scripts/export_logos_audit.mjs
```

生成时间会写入 `LOGOS_CORE_EXPORT.md` 顶部。
