# 怎么跟 LOGOS 说话，让它更有效学习

## 一句话公式

**「动作词 + 具体主题」** 或 **「X 是什么」「如何 X」「为什么 X」**

---

## 可以直接复制用的问题（按领域）

### 编程 / 计算机
```
TCP 三次握手过程
数据库 ACID 如何实现
什么是分布式事务
gRPC 的原理
Docker 的底层实现
线程上下文切换是如何实现的
MySQL 四种隔离级别
```

### 概念解释
```
什么是黑洞
什么是宇宙大爆炸
量子纠缠的基本原理
图灵完备的定义
什么是 CAP 定理
零知识证明是什么
```

### 比较类
```
进程和线程的区别
TCP 和 UDP 的区别
HashMap 和 BTreeMap 的性能场景
在金融领域期货和期权的区别
```

### 操作 / 实现
```
用 Rust 实现栈
写一个二分查找
如何学习操作系统原理
```

### LOGOS 自身
```
LOGOS 是自演化 AI 操作系统
尿袋之间通过管理器中转实现突触互学
sync_patterns 是尿袋间知识同步的标准机制
```

---

## 说什么更容易触发学习

- **完整句子**：至少 5 个字，不要太短
- **有明确主题**：解释 / 比较 / 如何 / 什么是 / 为什么
- **一次一个点**：别在一句话里塞太多问题

---

## 关于「太短、太模糊」

像 `啥`、`嗯`、`继续`、`那个东西`、`帮我看看` 这类口语化输入，LLM 也会给出合理回复，一样会被吸收进 pattern。没有负面影响，可以自然地说。

---

## 使用方式

1. **» 对话模式**：直接输入上面任意一句，回车
2. 若显示「未命中」或 fallback，等 1～2 个学习周期（约 2～3 分钟）后会吸收
3. 再问同样的问题，应能命中

---

## 如果还是不知道怎么问

就从「**X 的核心原理**」「**X 和 Y 的区别**」「**如何实现 X**」三种句式里选，把 X、Y 换成你关心的主题即可。

---

## 批量投喂（用你常用的 eval 数据）

若 backend 已启动（`cargo run`），可一键把 eval_result 里的问题投喂进去：

```bash
./scripts/feed_eval_to_backend.sh
```

可选参数：`LIMIT=50 ./scripts/feed_eval_to_backend.sh` 只投 50 条；`DELAY=2` 每条间隔 2 秒；`BACKEND=http://其他地址:3000` 指定后端。

**直接用 HuggingFace 数据集**（instruction/output、human/assistant 等格式均可）：

```bash
node scripts/feed_hf_to_backend.mjs alpaca-zh
node scripts/feed_hf_to_backend.mjs belle
node scripts/feed_hf_to_backend.mjs sharegpt
```

或指定完整 dataset 和字段：`node scripts/feed_hf_to_backend.mjs shibing624/alpaca-zh instruction output`。`MAX_TOTAL=10000 BATCH=1000` 可调规模。

---

## 云端一次性投喂（无需本地 backend）

数据直接写入 Worker D1 的 `language_pairs` 表：

```bash
node scripts/cloud_feed_hf.mjs alpaca-zh
node scripts/cloud_feed_hf.mjs belle
node scripts/cloud_feed_hf.mjs sharegpt
```

或指定完整 dataset：`node scripts/cloud_feed_hf.mjs shibing624/alpaca-zh instruction output`。`MAX_TOTAL=5000` 可调规模。每条 human 长度 5–500、gpt 最长 3000，超限会被过滤。

---

## 在随机问题里找不足，反哺逼近大模型水平

不从已投喂数据验证，而是从**多源随机抽样**（eval_result、medical、sharegpt、stem、code、finance、law 等未主投领域），测 Worker，miss 则用 reference 反哺，自动写入 `language_pairs`：

```bash
node scripts/improve_language_from_gaps.mjs --rounds 50 --per-round 20
```

`ROUNDS=100 PER_ROUND=30` 可调。通过 `/feed/language-upload` 写入 D1，不依赖 `/feedback`。部署新 Worker 后，UI 反哺（纠正）也会自动写入 language_pairs。
