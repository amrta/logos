CREATE TABLE IF NOT EXISTS evolution_candidates (
  id TEXT PRIMARY KEY,
  source TEXT NOT NULL,
  description TEXT NOT NULL,
  code TEXT NOT NULL,
  status TEXT DEFAULT 'pending',
  safety_score REAL,
  performance_score REAL,
  alignment_score REAL,
  proposed_at INTEGER,
  adopted_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_evolution_status ON evolution_candidates(status);
CREATE INDEX IF NOT EXISTS idx_evolution_proposed_at ON evolution_candidates(proposed_at DESC);
