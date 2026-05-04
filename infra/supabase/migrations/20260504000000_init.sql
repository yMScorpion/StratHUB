-- Phase 0: scaffolding only. Tables for ingestion, strategies, backtests, validation runs,
-- accounts, positions, trades, fills, outbox, heartbeats, and audit_log are added in their
-- respective phase migrations (Phases 2-6). RLS conventions are established here.
--
-- Conventions enforced from day one:
--   1. Every tenant-owned row carries a denormalized user_id (FK to auth.users).
--   2. RLS is enabled on every tenant table; default policy is `user_id = auth.uid()`.
--   3. updated_at is maintained by a trigger.
--   4. Cross-tenant access is asserted-against in CI by the RLS test matrix.

set search_path = public;

-- updated_at trigger helper
create or replace function set_updated_at()
returns trigger
language plpgsql
as $$
begin
  new.updated_at = now();
  return new;
end;
$$;

-- profiles: 1:1 with auth.users; jurisdiction + ToS + risk-ack captured at signup
create table if not exists profiles (
  id uuid primary key references auth.users(id) on delete cascade,
  display_name text,
  avatar_url text,
  theme text check (theme in ('system','light','dark')) default 'system',
  default_account uuid,
  jurisdiction text,
  tos_accepted_at timestamptz,
  risk_ack_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create trigger profiles_set_updated_at
  before update on profiles
  for each row execute function set_updated_at();

alter table profiles enable row level security;

create policy "profiles are viewable by owner"
  on profiles for select
  to authenticated
  using (id = auth.uid());

create policy "profiles are updatable by owner"
  on profiles for update
  to authenticated
  using (id = auth.uid())
  with check (id = auth.uid());

-- New auth.users get a profiles row automatically. No client-side insert allowed.
create or replace function handle_new_user()
returns trigger
language plpgsql
security definer
set search_path = public
as $$
begin
  insert into profiles (id) values (new.id) on conflict (id) do nothing;
  return new;
end;
$$;

drop trigger if exists on_auth_user_created on auth.users;
create trigger on_auth_user_created
  after insert on auth.users
  for each row execute function handle_new_user();

-- api_keys: KMS envelope encryption. Plaintext keys NEVER live in this table.
-- The Rust executor decrypts (key_ciphertext, dek_ciphertext) inside its process at startup,
-- using the cluster's KMS key (kms_key_id).
create table if not exists api_keys (
  id uuid primary key default gen_random_uuid(),
  user_id uuid not null references auth.users(id) on delete cascade,
  provider text not null check (provider in ('binance','bybit','openrouter')),
  label text not null,
  kms_key_id text not null,
  dek_ciphertext bytea not null,
  key_ciphertext bytea not null,
  nonce bytea not null,
  scope text[] not null default array['trade']::text[],
  ip_allowlist inet[],
  rotated_at timestamptz,
  revoked_at timestamptz,
  last_used_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  -- A user has at most one active key per provider+label.
  constraint api_keys_unique_label unique (user_id, provider, label)
);

create trigger api_keys_set_updated_at
  before update on api_keys
  for each row execute function set_updated_at();

alter table api_keys enable row level security;

create policy "api_keys readable by owner"
  on api_keys for select
  to authenticated
  using (user_id = auth.uid());

create policy "api_keys insertable by owner"
  on api_keys for insert
  to authenticated
  with check (user_id = auth.uid());

create policy "api_keys updatable by owner"
  on api_keys for update
  to authenticated
  using (user_id = auth.uid())
  with check (user_id = auth.uid());

create policy "api_keys deletable by owner"
  on api_keys for delete
  to authenticated
  using (user_id = auth.uid());

-- risk_limits: one row per user, enforced both at submission time (UI/api) and
-- inside the Rust executor's risk guard.
create table if not exists risk_limits (
  user_id uuid primary key references auth.users(id) on delete cascade,
  max_daily_loss_pct numeric(6,3) not null default 2.000 check (max_daily_loss_pct >= 0),
  max_position_pct numeric(6,3) not null default 5.000 check (max_position_pct >= 0),
  max_concurrent integer not null default 5 check (max_concurrent between 1 and 32),
  kill_switch_active boolean not null default false,
  monthly_llm_budget_cents integer not null default 2000 check (monthly_llm_budget_cents >= 0),
  max_validators integer not null default 5 check (max_validators between 0 and 50),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create trigger risk_limits_set_updated_at
  before update on risk_limits
  for each row execute function set_updated_at();

alter table risk_limits enable row level security;

create policy "risk_limits readable by owner"
  on risk_limits for select
  to authenticated
  using (user_id = auth.uid());

create policy "risk_limits upsertable by owner"
  on risk_limits for insert
  to authenticated
  with check (user_id = auth.uid());

create policy "risk_limits updatable by owner"
  on risk_limits for update
  to authenticated
  using (user_id = auth.uid())
  with check (user_id = auth.uid());

-- Storage buckets are created by `supabase/seed.sql` (Phase 0 doesn't seed objects yet).
