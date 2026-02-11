CREATE INDEX IF NOT EXISTS idx_evolution_status ON evolution_candidates(status);
CREATE INDEX IF NOT EXISTS idx_evolution_proposed_at ON evolution_candidates(proposed_at DESC);
