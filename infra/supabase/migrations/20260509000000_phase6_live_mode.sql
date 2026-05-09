-- Phase 6: live mode tables.
-- Adds live_runs, positions, live_fills, and audit_log.
-- All tables follow the same RLS conventions as earlier phases:
--   user_id = auth.uid() for reads/inserts; audit_log is append-only (no UPDATE policy).

SET search_path = public;

-- live_runs: one row per live trading session.
CREATE TABLE IF NOT EXISTS live_runs (
  id                    uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id               uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  strategy_id           uuid        NOT NULL REFERENCES strategies(id) ON DELETE CASCADE,
  spec_hash             text        NOT NULL CHECK (spec_hash ~ '^[0-9a-f]{64}$'),
  exchange              text        NOT NULL CHECK (exchange IN ('binance','bybit')),
  account_id            text        NOT NULL,
  status                text        NOT NULL DEFAULT 'running'
    CHECK (status IN ('running','stopped','error')),
  confirmation_token    text        NOT NULL,
  flatten_on_kill       boolean     NOT NULL DEFAULT false,
  kill_switch_fired_at  timestamptz,
  kill_switch_reason    text,
  started_at            timestamptz NOT NULL DEFAULT now(),
  stopped_at            timestamptz,
  error                 text,
  created_at            timestamptz NOT NULL DEFAULT now(),
  updated_at            timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS live_runs_user_status_idx
  ON live_runs (user_id, status);

CREATE TRIGGER live_runs_set_updated_at
  BEFORE UPDATE ON live_runs
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

ALTER TABLE live_runs ENABLE ROW LEVEL SECURITY;

CREATE POLICY "live_runs readable by owner"
  ON live_runs FOR SELECT TO authenticated
  USING (user_id = auth.uid());

CREATE POLICY "live_runs insertable by owner"
  ON live_runs FOR INSERT TO authenticated
  WITH CHECK (user_id = auth.uid());

CREATE POLICY "live_runs updatable by owner"
  ON live_runs FOR UPDATE TO authenticated
  USING (user_id = auth.uid())
  WITH CHECK (user_id = auth.uid());

-- positions: current signed-quantity per symbol for an active live run.
-- Upserted on each fill; removed when qty reaches zero.

CREATE TABLE IF NOT EXISTS positions (
  id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id     uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  live_run_id uuid        NOT NULL REFERENCES live_runs(id) ON DELETE CASCADE,
  exchange    text        NOT NULL CHECK (exchange IN ('binance','bybit')),
  symbol      text        NOT NULL,
  qty         numeric     NOT NULL,
  updated_at  timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT positions_run_symbol_unique UNIQUE (live_run_id, symbol)
);

CREATE INDEX IF NOT EXISTS positions_user_idx ON positions (user_id);

ALTER TABLE positions ENABLE ROW LEVEL SECURITY;

CREATE POLICY "positions readable by owner"
  ON positions FOR SELECT TO authenticated
  USING (user_id = auth.uid());

CREATE POLICY "positions insertable by owner"
  ON positions FOR INSERT TO authenticated
  WITH CHECK (user_id = auth.uid());

CREATE POLICY "positions updatable by owner"
  ON positions FOR UPDATE TO authenticated
  USING (user_id = auth.uid())
  WITH CHECK (user_id = auth.uid());

CREATE POLICY "positions deletable by owner"
  ON positions FOR DELETE TO authenticated
  USING (user_id = auth.uid());

-- live_fills: exchange-confirmed fills from live trading runs.
-- client_order_id ties back to the order_outbox deterministic UUID.

CREATE TABLE IF NOT EXISTS live_fills (
  id                uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id           uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  live_run_id       uuid        NOT NULL REFERENCES live_runs(id) ON DELETE CASCADE,
  client_order_id   uuid        NOT NULL,
  exchange_order_id text        NOT NULL,
  exchange          text        NOT NULL CHECK (exchange IN ('binance','bybit')),
  symbol            text        NOT NULL,
  side              text        NOT NULL CHECK (side IN ('buy','sell')),
  qty               numeric     NOT NULL CHECK (qty > 0),
  price             numeric     NOT NULL CHECK (price > 0),
  quote_qty         numeric     NOT NULL CHECK (quote_qty > 0),
  reconciled        boolean     NOT NULL DEFAULT false,
  recon_result      text,
  filled_at         timestamptz NOT NULL,
  created_at        timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT live_fills_client_order_unique UNIQUE (exchange, client_order_id)
);

CREATE INDEX IF NOT EXISTS live_fills_run_idx ON live_fills (live_run_id, filled_at);
CREATE INDEX IF NOT EXISTS live_fills_reconcile_idx ON live_fills (reconciled, live_run_id);

ALTER TABLE live_fills ENABLE ROW LEVEL SECURITY;

CREATE POLICY "live_fills readable by owner"
  ON live_fills FOR SELECT TO authenticated
  USING (user_id = auth.uid());

CREATE POLICY "live_fills insertable by owner"
  ON live_fills FOR INSERT TO authenticated
  WITH CHECK (user_id = auth.uid());

-- audit_log: immutable append-only record of every live action.
-- No UPDATE or DELETE policy. Rows survive even if the live_run is deleted (ON DELETE SET NULL).

CREATE TABLE IF NOT EXISTS audit_log (
  id          bigint      GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
  user_id     uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  live_run_id uuid        REFERENCES live_runs(id) ON DELETE SET NULL,
  spec_hash   text        NOT NULL CHECK (spec_hash ~ '^[0-9a-f]{64}$'),
  exchange    text        NOT NULL CHECK (exchange IN ('binance','bybit')),
  action      text        NOT NULL,
  payload     jsonb       NOT NULL DEFAULT '{}'::jsonb,
  result      text        NOT NULL,
  logged_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS audit_log_user_idx ON audit_log (user_id, logged_at);
CREATE INDEX IF NOT EXISTS audit_log_run_idx  ON audit_log (live_run_id, logged_at);

ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY;

CREATE POLICY "audit_log readable by owner"
  ON audit_log FOR SELECT TO authenticated
  USING (user_id = auth.uid());

-- Append-only: only INSERT is allowed; no UPDATE, no DELETE.
CREATE POLICY "audit_log insertable by owner"
  ON audit_log FOR INSERT TO authenticated
  WITH CHECK (user_id = auth.uid());
