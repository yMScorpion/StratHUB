import Link from "next/link";
import { createSupabaseServerClientReadonly } from "@/lib/supabase/server";
import type { Json } from "@/lib/supabase/types";

export const dynamic = "force-dynamic";

type Metrics = Record<string, string>;
type StrategyRow = {
  id: string;
  spec_hash: string;
  status: "needs_review" | "approved" | "rejected";
  spec_jsonb: Json;
};
type BacktestRow = {
  id: string;
  status: "queued" | "running" | "succeeded" | "failed";
  data_snapshot_id: string;
  initial_equity: number;
  kpi_jsonb: Json;
  equity_curve_jsonb: Json;
  heatmap_jsonb: Json;
  created_at: string;
};

function metricValue(kpi: unknown, key: string) {
  const metrics = (kpi as { metrics?: Metrics } | null)?.metrics;
  return metrics?.[key] ?? "0";
}

export default async function BacktestPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const supabase = await createSupabaseServerClientReadonly();
  const {
    data: { user },
  } = await supabase.auth.getUser();

  const { data: strategyRaw } = user
    ? await supabase
        .from("strategies")
        .select("id, spec_hash, status, spec_jsonb")
        .eq("id", id)
        .single()
    : { data: null };
  const strategy = strategyRaw as StrategyRow | null;

  const { data: backtestsRaw } = user
    ? await supabase
        .from("backtests")
        .select("id, status, data_snapshot_id, initial_equity, kpi_jsonb, equity_curve_jsonb, heatmap_jsonb, created_at")
        .eq("strategy_id", id)
        .order("created_at", { ascending: false })
        .limit(1)
    : { data: null };
  const backtests = backtestsRaw as BacktestRow[] | null;

  const latest = backtests?.[0] ?? null;
  const risk = (strategy?.spec_jsonb as { risk?: Record<string, string | number> } | null)?.risk ?? {};
  const modes = ["fixed", "percent", "Kelly", "vol-target", "trailing"];
  const metricCards = [
    ["Sharpe", "sharpe"],
    ["Sortino", "sortino"],
    ["Calmar", "calmar"],
    ["Max DD", "max_dd_pct"],
    ["Profit Factor", "profit_factor"],
    ["Win Rate", "win_rate"],
    ["Avg R:R", "avg_rr"],
    ["Expectancy", "expectancy"],
    ["CAGR", "cagr"],
    ["Time-in-Market", "time_in_market"],
  ];

  return (
    <main style={{ padding: "2rem 4rem", maxWidth: 1180 }}>
      <div style={{ display: "flex", justifyContent: "space-between", gap: "1rem", alignItems: "center" }}>
        <div>
          <h1 style={{ fontSize: "1.6rem", fontWeight: 650, margin: 0 }}>Backtest</h1>
          <p style={{ margin: "0.35rem 0 0", opacity: 0.62, fontFamily: "monospace", fontSize: "0.8rem" }}>
            {strategy?.spec_hash ?? "No strategy loaded"}
          </p>
        </div>
        <form action={`/api/strategies/${id}/backtests`} method="post">
          <button
            type="submit"
            disabled={!strategy || strategy.status !== "approved"}
            style={{
              background: strategy?.status === "approved" ? "#2563eb" : "#9ca3af",
              color: "#fff",
              border: 0,
              borderRadius: 6,
              padding: "0.62rem 1rem",
              fontWeight: 600,
              cursor: strategy?.status === "approved" ? "pointer" : "not-allowed",
            }}
          >
            Re-run
          </button>
        </form>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 280px", gap: "1.5rem", marginTop: "2rem" }}>
        <section>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(5, minmax(120px, 1fr))", gap: "0.75rem" }}>
            {metricCards.map(([label, key]) => (
              <div key={key} style={{ border: "1px solid #e5e7eb", borderRadius: 8, padding: "0.8rem", minHeight: 74 }}>
                <div style={{ opacity: 0.58, fontSize: "0.76rem" }}>{label}</div>
                <div style={{ marginTop: "0.45rem", fontSize: "1.05rem", fontWeight: 650 }}>
                  {latest ? metricValue(latest.kpi_jsonb, key) : "-"}
                </div>
              </div>
            ))}
          </div>

          <div style={{ marginTop: "1.25rem", border: "1px solid #e5e7eb", borderRadius: 8, padding: "1rem" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <h2 style={{ fontSize: "1rem", margin: 0 }}>Equity curve</h2>
              <span style={{ fontSize: "0.78rem", opacity: 0.6 }}>
                Snapshot {latest?.data_snapshot_id ?? "not run"}
              </span>
            </div>
            <pre style={{ overflowX: "auto", margin: "1rem 0 0", fontSize: "0.78rem", opacity: 0.72 }}>
              {JSON.stringify(latest?.equity_curve_jsonb ?? [], null, 2)}
            </pre>
          </div>

          <div style={{ marginTop: "1.25rem", border: "1px solid #e5e7eb", borderRadius: 8, padding: "1rem" }}>
            <h2 style={{ fontSize: "1rem", margin: 0 }}>Heatmap</h2>
            <pre style={{ overflowX: "auto", margin: "1rem 0 0", fontSize: "0.78rem", opacity: 0.72 }}>
              {JSON.stringify(latest?.heatmap_jsonb ?? {}, null, 2)}
            </pre>
          </div>
        </section>

        <aside style={{ borderLeft: "1px solid #e5e7eb", paddingLeft: "1.25rem" }}>
          <h2 style={{ fontSize: "1rem", margin: 0 }}>Risk Model</h2>
          <div style={{ display: "grid", gap: "0.45rem", marginTop: "0.9rem" }}>
            {modes.map((mode) => (
              <label key={mode} style={{ display: "flex", alignItems: "center", gap: "0.5rem", fontSize: "0.86rem" }}>
                <input type="radio" name="risk-mode" defaultChecked={String(risk.model ?? "").includes(mode.toLowerCase())} readOnly />
                {mode}
              </label>
            ))}
          </div>
          <dl style={{ display: "grid", gap: "0.65rem", marginTop: "1.4rem", fontSize: "0.84rem" }}>
            {Object.entries(risk).map(([key, value]) => (
              <div key={key}>
                <dt style={{ opacity: 0.56 }}>{key}</dt>
                <dd style={{ margin: "0.18rem 0 0", fontWeight: 600 }}>{String(value)}</dd>
              </div>
            ))}
          </dl>
          <Link href={`/strategies/${id}/review`} style={{ display: "inline-block", marginTop: "1.5rem", color: "#2563eb", fontSize: "0.85rem" }}>
            Back to review
          </Link>
        </aside>
      </div>
    </main>
  );
}
