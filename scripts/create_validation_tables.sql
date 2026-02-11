CREATE TABLE IF NOT EXISTS validation_history (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  candidate_id TEXT NOT NULL,
  validation_type TEXT NOT NULL,
  score REAL NOT NULL,
  passed INTEGER NOT NULL,
  reason TEXT,
  validated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_validation_candidate ON validation_history(candidate_id);
CREATE INDEX IF NOT EXISTS idx_validation_time ON validation_history(validated_at DESC);

CREATE TABLE IF NOT EXISTS validation_rules (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  rule_type TEXT NOT NULL,
  pattern TEXT NOT NULL,
  severity TEXT NOT NULL,
  score_penalty REAL NOT NULL,
  description TEXT
);

INSERT OR IGNORE INTO validation_rules (rule_type, pattern, severity, score_penalty, description) VALUES
  ('safety', 'frozen::', 'critical', 1.0, '禁止访问frozen层'),
  ('safety', 'unsafe', 'critical', 1.0, '禁止使用unsafe代码'),
  ('safety', 'std::process::', 'critical', 1.0, '禁止执行外部进程'),
  ('safety', 'std::fs::remove', 'critical', 1.0, '禁止删除文件'),
  ('safety', 'eval\\(', 'critical', 1.0, '禁止eval动态执行'),
  ('performance', 'loop\\s*\\{[^}]{500,}\\}', 'warning', 0.3, '循环体过大可能影响性能'),
  ('performance', 'sleep|delay', 'warning', 0.2, '包含阻塞操作'),
  ('alignment', 'password|token|secret', 'warning', 0.3, '可能涉及敏感信息');
