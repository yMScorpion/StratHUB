-- Phase 4: deterministic backtests and snapshot-versioned market data metadata.

CREATE TABLE market_data_snapshots (
  id                text PRIMARY KEY,
  venue             text NOT NULL,
  symbol            text NOT NULL,
  timeframe         text NOT NULL,
  storage_uri       text NOT NULL,
  storage_format    text NOT NULL CHECK (storage_format IN ('duckdb_parquet', 'json_fixture')),
  row_count         bigint NOT NULL CHECK (row_count >= 0),
  starts_at         timestamptz,
  ends_at           timestamptz,
  fee_model_jsonb   jsonb NOT NULL DEFAULT '{}'::jsonb,
  funding_jsonb     jsonb NOT NULL DEFAULT '{}'::jsonb,
  spread_jsonb      jsonb NOT NULL DEFAULT '{}'::jsonb,
  contract_jsonb    jsonb NOT NULL DEFAULT '{}'::jsonb,
  lot_jsonb         jsonb NOT NULL DEFAULT '{}'::jsonb,
  margin_jsonb      jsonb NOT NULL DEFAULT '{}'::jsonb,
  liquidation_jsonb jsonb NOT NULL DEFAULT '{}'::jsonb,
  delistings_jsonb  jsonb NOT NULL DEFAULT '[]'::jsonb,
  content_sha256    text NOT NULL CHECK (content_sha256 ~ '^[0-9a-f]{64}$'),
  created_at        timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE backtests (
  id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id             uuid NOT NULL REFERENCES auth.users (id) ON DELETE CASCADE,
  strategy_id         uuid NOT NULL REFERENCES strategies (id) ON DELETE CASCADE,
  spec_hash           text NOT NULL CHECK (spec_hash ~ '^[0-9a-f]{64}$'),
  data_snapshot_id    text NOT NULL REFERENCES market_data_snapshots (id),
  status              text NOT NULL DEFAULT 'queued'
    CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
  mode                text NOT NULL DEFAULT 'backtest' CHECK (mode = 'backtest'),
  initial_equity      numeric(38, 18) NOT NULL,
  kpi_jsonb           jsonb NOT NULL DEFAULT '{}'::jsonb,
  equity_curve_jsonb  jsonb NOT NULL DEFAULT '[]'::jsonb,
  heatmap_jsonb       jsonb NOT NULL DEFAULT '{}'::jsonb,
  trades_jsonb        jsonb NOT NULL DEFAULT '[]'::jsonb,
  error               text,
  started_at          timestamptz,
  completed_at        timestamptz,
  created_at          timestamptz NOT NULL DEFAULT now(),
  updated_at          timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT backtests_kpi_meta_snapshot_matches
    CHECK (
      kpi_jsonb = '{}'::jsonb OR
      kpi_jsonb #>> '{meta,data_snapshot_id}' = data_snapshot_id
    )
);

CREATE UNIQUE INDEX backtests_reproducible_run_unique
  ON backtests (strategy_id, spec_hash, data_snapshot_id, initial_equity);

CREATE INDEX backtests_user_created_idx ON backtests (user_id, created_at DESC);
CREATE INDEX backtests_strategy_created_idx ON backtests (strategy_id, created_at DESC);

CREATE TRIGGER backtests_set_updated_at
  BEFORE UPDATE ON backtests
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

ALTER TABLE market_data_snapshots ENABLE ROW LEVEL SECURITY;
ALTER TABLE backtests ENABLE ROW LEVEL SECURITY;

CREATE POLICY "market data snapshots are readable by authenticated users"
  ON market_data_snapshots FOR SELECT
  TO authenticated
  USING (true);

CREATE POLICY "users can read their own backtests"
  ON backtests FOR SELECT
  TO authenticated
  USING (auth.uid() = user_id);

CREATE POLICY "users can create their own backtests"
  ON backtests FOR INSERT
  TO authenticated
  WITH CHECK (auth.uid() = user_id);

CREATE POLICY "users can update their own backtests"
  ON backtests FOR UPDATE
  TO authenticated
  USING (auth.uid() = user_id)
  WITH CHECK (auth.uid() = user_id);
