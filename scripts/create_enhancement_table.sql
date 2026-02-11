CREATE TABLE IF NOT EXISTS enhancement_history (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  input TEXT NOT NULL,
  layer_0_output TEXT,
  layer_1_output TEXT,
  layer_2_output TEXT,
  final_layer INTEGER NOT NULL,
  confidence_0 REAL,
  confidence_1 REAL,
  confidence_2 REAL,
  timestamp INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_enhancement_time ON enhancement_history(timestamp DESC);
