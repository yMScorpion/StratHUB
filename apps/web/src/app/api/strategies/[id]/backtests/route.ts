import { NextResponse } from "next/server";
import { createSupabaseServerClient } from "@/lib/supabase/server";
import type { Json } from "@/lib/supabase/types";

type StrategySpec = {
  exchange?: string;
  symbols?: string[];
  timeframe?: string;
};

export async function POST(
  request: Request,
  { params }: { params: Promise<{ id: string }> },
) {
  const { id } = await params;
  const supabase = await createSupabaseServerClient();
  const {
    data: { user },
  } = await supabase.auth.getUser();

  if (!user) {
    return NextResponse.json({ error: "unauthorized" }, { status: 401 });
  }

  const { data: strategyRaw, error } = await supabase
    .from("strategies")
    .select("id, status, spec_hash, spec_jsonb")
    .eq("id", id)
    .single();
  const strategy = strategyRaw as
    | {
        id: string;
        status: "needs_review" | "approved" | "rejected";
        spec_hash: string;
        spec_jsonb: Json;
      }
    | null;

  if (error || !strategy) {
    return NextResponse.json({ error: "strategy not found" }, { status: 404 });
  }
  if (strategy.status !== "approved") {
    return NextResponse.json(
      { error: "strategy must be approved before backtest" },
      { status: 409 },
    );
  }

  const isFormPost = request.headers
    .get("content-type")
    ?.includes("application/x-www-form-urlencoded") ?? false;
  const formData = isFormPost ? await request.formData() : null;
  const initialEquityRaw = formData?.get("initial_equity")?.toString() ?? "10000";
  const initialEquity = Number(initialEquityRaw);
  if (!Number.isFinite(initialEquity) || initialEquity <= 0) {
    return NextResponse.json({ error: "initial_equity must be positive" }, { status: 400 });
  }

  const spec = strategy.spec_jsonb as StrategySpec;
  const symbol = spec.symbols?.[0];
  if (!spec.exchange || !symbol || !spec.timeframe) {
    return NextResponse.json(
      { error: "strategy spec is missing exchange, symbol, or timeframe" },
      { status: 422 },
    );
  }

  const { data: snapshotRaw, error: snapshotError } = await supabase
    .from("market_data_snapshots")
    .select("id")
    .eq("venue", spec.exchange)
    .eq("symbol", symbol)
    .eq("timeframe", spec.timeframe)
    .order("created_at", { ascending: false })
    .limit(1)
    .single();
  const snapshot = snapshotRaw as { id: string } | null;

  if (snapshotError || !snapshot) {
    return NextResponse.json(
      { error: "no market data snapshot found for strategy symbol/timeframe" },
      { status: 409 },
    );
  }

  const backtestPayload = {
    user_id: user.id,
    strategy_id: strategy.id,
    spec_hash: strategy.spec_hash,
    data_snapshot_id: snapshot.id,
    status: "queued" as const,
    mode: "backtest" as const,
    initial_equity: initialEquity,
    kpi_jsonb: {},
    equity_curve_jsonb: [],
    heatmap_jsonb: {},
    trades_jsonb: [],
    error: null,
    started_at: null,
    completed_at: null,
  };

  const { data: backtestRaw, error: backtestError } = await supabase
    .from("backtests")
    .upsert(backtestPayload, {
      onConflict: "strategy_id,spec_hash,data_snapshot_id,initial_equity",
    })
    .select("id, status, data_snapshot_id, initial_equity")
    .single();

  if (backtestError || !backtestRaw) {
    return NextResponse.json(
      { error: backtestError?.message ?? "failed to enqueue backtest" },
      { status: 500 },
    );
  }

  if (isFormPost) {
    return NextResponse.redirect(new URL(`/strategies/${id}/backtest`, request.url), 303);
  }

  return NextResponse.json(
    {
      queued: true,
      backtest: backtestRaw,
      strategy_id: strategy.id,
      spec_hash: strategy.spec_hash,
    },
    { status: 202 },
  );
}
