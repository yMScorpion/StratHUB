-- Phase 6: live mode tables.
-- Adds accounts, live_runs, positions, trades, fills, and audit_log.
-- All tables follow the same RLS conventions as earlier phases:
--   owner reads use user_id = auth.uid(); executor-owned records are written only by service role.

SET search_path = public;

-- accounts: user-managed exchange accounts eligible for live execution.
CREATE TABLE IF NOT EXISTS accounts (
  id                  uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id             uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  exchange            text        NOT NULL CHECK (exchange IN ('binance','bybit')),
  label               text        NOT NULL,
  broker_account_ref  text        NOT NULL,
  api_key_id          uuid        REFERENCES api_keys(id) ON DELETE SET NULL,
  status              text        NOT NULL DEFAULT 'active'
    CHECK (status IN ('active','disabled','revoked')),
  metadata            jsonb       NOT NULL DEFAULT '{}'::jsonb,
  created_at          timestamptz NOT NULL DEFAULT now(),
  updated_at          timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT accounts_user_label_unique UNIQUE (user_id, exchange, label)
);

CREATE INDEX IF NOT EXISTS accounts_user_idx ON accounts (user_id, status);

CREATE TRIGGER accounts_set_updated_at
  BEFORE UPDATE ON accounts
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

ALTER TABLE accounts ENABLE ROW LEVEL SECURITY;

CREATE POLICY "accounts readable by owner"
  ON accounts FOR SELECT TO authenticated
  USING (user_id = auth.uid());

CREATE POLICY "accounts insertable by owner"
  ON accounts FOR INSERT TO authenticated
  WITH CHECK (user_id = auth.uid());

CREATE POLICY "accounts updatable by owner"
  ON accounts FOR UPDATE TO authenticated
  USING (user_id = auth.uid())
  WITH CHECK (user_id = auth.uid());

-- live_runs: one row per live trading session.
CREATE TABLE IF NOT EXISTS live_runs (
  id                    uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id               uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  strategy_id           uuid        NOT NULL REFERENCES strategies(id) ON DELETE CASCADE,
  spec_hash             text        NOT NULL CHECK (spec_hash ~ '^[0-9a-f]{64}$'),
  exchange              text        NOT NULL CHECK (exchange IN ('binance','bybit')),
  account_id            uuid        NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
  status                text        NOT NULL DEFAULT 'running'
    CHECK (status IN ('running','stopped','error')),
  confirmation_token_digest text    NOT NULL CHECK (confirmation_token_digest ~ '^[0-9a-f]{64}$'),
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

-- No authenticated INSERT/UPDATE/DELETE policies: live_runs are executor/service-role written.

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

-- No authenticated INSERT/UPDATE/DELETE policies: positions are executor/service-role written.

-- trades: logical trade lifecycle grouped from one or more exchange fills.
CREATE TABLE IF NOT EXISTS trades (
  id                uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id           uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  live_run_id       uuid        NOT NULL REFERENCES live_runs(id) ON DELETE CASCADE,
  strategy_id       uuid        NOT NULL REFERENCES strategies(id) ON DELETE CASCADE,
  exchange          text        NOT NULL CHECK (exchange IN ('binance','bybit')),
  symbol            text        NOT NULL,
  side              text        NOT NULL CHECK (side IN ('long','short')),
  entry_qty         numeric     NOT NULL CHECK (entry_qty > 0),
  entry_price       numeric     NOT NULL CHECK (entry_price > 0),
  exit_qty          numeric     CHECK (exit_qty >= 0),
  exit_price        numeric     CHECK (exit_price > 0),
  status            text        NOT NULL DEFAULT 'open'
    CHECK (status IN ('open','closed','cancelled')),
  opened_at         timestamptz NOT NULL DEFAULT now(),
  closed_at         timestamptz,
  created_at        timestamptz NOT NULL DEFAULT now(),
  updated_at        timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS trades_run_idx ON trades (live_run_id, opened_at);
CREATE INDEX IF NOT EXISTS trades_user_idx ON trades (user_id, status);

CREATE TRIGGER trades_set_updated_at
  BEFORE UPDATE ON trades
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

ALTER TABLE trades ENABLE ROW LEVEL SECURITY;

CREATE POLICY "trades readable by owner"
  ON trades FOR SELECT TO authenticated
  USING (user_id = auth.uid());

-- No authenticated INSERT/UPDATE/DELETE policies: trades are executor/service-role written.

-- fills: exchange-confirmed fills from live trading runs.
-- client_order_id ties back to the order_outbox deterministic UUID.
CREATE TABLE IF NOT EXISTS fills (
  id                uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id           uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  live_run_id       uuid        NOT NULL REFERENCES live_runs(id) ON DELETE CASCADE,
  trade_id          uuid        REFERENCES trades(id) ON DELETE SET NULL,
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
  CONSTRAINT fills_client_order_unique UNIQUE (exchange, client_order_id)
);

CREATE INDEX IF NOT EXISTS fills_run_idx ON fills (live_run_id, filled_at);
CREATE INDEX IF NOT EXISTS fills_reconcile_idx ON fills (reconciled, live_run_id);

ALTER TABLE fills ENABLE ROW LEVEL SECURITY;

CREATE POLICY "fills readable by owner"
  ON fills FOR SELECT TO authenticated
  USING (user_id = auth.uid());

-- No authenticated INSERT/UPDATE/DELETE policies: fills are executor/service-role written.

ALTER TABLE order_outbox
  ADD COLUMN IF NOT EXISTS live_run_id uuid REFERENCES live_runs(id) ON DELETE CASCADE,
  ADD COLUMN IF NOT EXISTS account_id uuid REFERENCES accounts(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS order_outbox_live_run_idx
  ON order_outbox (live_run_id, status);

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

-- No authenticated INSERT/UPDATE/DELETE policies: audit rows are append-only service-role writes.
