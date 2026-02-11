CREATE TABLE IF NOT EXISTS learned_patterns (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  pattern_keywords TEXT NOT NULL,
  full_input TEXT NOT NULL,
  response_template TEXT NOT NULL,
  source TEXT NOT NULL,
  confidence REAL NOT NULL,
  learned_at INTEGER NOT NULL,
  usage_count INTEGER DEFAULT 0,
  last_used INTEGER,
  status TEXT DEFAULT 'pending'
);

CREATE INDEX IF NOT EXISTS idx_pattern_keywords ON learned_patterns(pattern_keywords);
CREATE INDEX IF NOT EXISTS idx_learned_at ON learned_patterns(learned_at DESC);
CREATE INDEX IF NOT EXISTS idx_learned_status ON learned_patterns(status);

CREATE TABLE IF NOT EXISTS evolution_history (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  event_type TEXT NOT NULL,
  source TEXT,
  target TEXT,
  data TEXT,
  success INTEGER,
  timestamp INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_evolution_type ON evolution_history(event_type);
CREATE INDEX IF NOT EXISTS idx_evolution_time ON evolution_history(timestamp DESC);

CREATE TABLE IF NOT EXISTS pouch_feedback (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  from_pouch TEXT NOT NULL,
  to_pouch TEXT NOT NULL,
  feedback_type TEXT NOT NULL,
  content TEXT NOT NULL,
  confidence REAL,
  created_at INTEGER NOT NULL,
  processed INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_feedback_to ON pouch_feedback(to_pouch, processed);
CREATE INDEX IF NOT EXISTS idx_feedback_time ON pouch_feedback(created_at DESC);
