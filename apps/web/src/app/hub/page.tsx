import Link from "next/link";
import { createSupabaseServerClientReadonly } from "@/lib/supabase/server";
import type { Json } from "@/lib/supabase/types";

export const dynamic = "force-dynamic";

type SearchParams = Promise<{
  q?: string;
  status?: string;
  exchange?: string;
  timeframe?: string;
  sort?: string;
  selected?: string;
}>;

type StrategyRow = {
  id: string;
  spec_jsonb: Json;
  spec_hash: string;
  status: "needs_review" | "approved" | "rejected";
  reviewed_at: string | null;
  created_at: string;
};

type SpecObject = Record<string, unknown>;

function isObject(value: unknown): value is SpecObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function specOf(value: Json): SpecObject {
  return isObject(value) ? value : {};
}

function textValue(value: unknown, fallback = "unknown"): string {
  return typeof value === "string" && value.trim() ? value : fallback;
}

function nameFor(strategy: StrategyRow): string {
  const spec = specOf(strategy.spec_jsonb);
  const metadata = isObject(spec.metadata) ? spec.metadata : {};
  return textValue(metadata.name, "Untitled strategy");
}

function exchangeFor(strategy: StrategyRow): string {
  return textValue(specOf(strategy.spec_jsonb).exchange);
}

function timeframeFor(strategy: StrategyRow): string {
  return textValue(specOf(strategy.spec_jsonb).timeframe);
}

function symbolsFor(strategy: StrategyRow): string {
  const symbols = specOf(strategy.spec_jsonb).symbols;
  return Array.isArray(symbols) ? symbols.join(", ") : "unknown symbols";
}

function riskSummary(strategy: StrategyRow): string {
  const risk = specOf(strategy.spec_jsonb).risk;
  if (!isObject(risk)) return "No risk model";
  const model = textValue(risk.model, "risk");
  const perTrade = textValue(risk.per_trade_pct, "?");
  const maxConcurrent = typeof risk.max_concurrent === "number" ? risk.max_concurrent : "?";
  const minRr = textValue(risk.min_rr, "?");
  return `${model} · ${perTrade}% risk · max ${maxConcurrent} · RR ${minRr}`;
}

function statusStyle(status: string) {
  if (status === "approved") return { background: "#dcfce7", color: "#166534", border: "#86efac" };
  if (status === "rejected") return { background: "#fee2e2", color: "#991b1b", border: "#fecaca" };
  return { background: "#fef3c7", color: "#92400e", border: "#fde68a" };
}

function filteredStrategies(strategies: StrategyRow[], query: Awaited<SearchParams>): StrategyRow[] {
  const q = query.q?.trim().toLowerCase();
  const status = query.status && query.status !== "all" ? query.status : null;
  const exchange = query.exchange && query.exchange !== "all" ? query.exchange : null;
  const timeframe = query.timeframe && query.timeframe !== "all" ? query.timeframe : null;

  const filtered = strategies.filter((strategy) => {
    const haystack = [
      nameFor(strategy),
      strategy.spec_hash,
      strategy.status,
      exchangeFor(strategy),
      timeframeFor(strategy),
      symbolsFor(strategy),
    ].join(" ").toLowerCase();

    return (!q || haystack.includes(q))
      && (!status || strategy.status === status)
      && (!exchange || exchangeFor(strategy) === exchange)
      && (!timeframe || timeframeFor(strategy) === timeframe);
  });

  const sort = query.sort ?? "created_desc";
  return filtered.sort((a, b) => {
    if (sort === "created_asc") return Date.parse(a.created_at) - Date.parse(b.created_at);
    if (sort === "name_asc") return nameFor(a).localeCompare(nameFor(b));
    if (sort === "status_asc") return a.status.localeCompare(b.status);
    return Date.parse(b.created_at) - Date.parse(a.created_at);
  });
}

function uniqueValues(strategies: StrategyRow[], getter: (strategy: StrategyRow) => string): string[] {
  return Array.from(new Set(strategies.map(getter).filter((value) => value !== "unknown"))).sort();
}

export default async function HubPage({ searchParams }: { searchParams: SearchParams }) {
  const query = await searchParams;
  const supabase = await createSupabaseServerClientReadonly();
  const {
    data: { user },
  } = await supabase.auth.getUser();

  let strategies: StrategyRow[] = [];

  if (user) {
    const { data } = await supabase
      .from("strategies")
      .select("id, spec_jsonb, spec_hash, status, reviewed_at, created_at")
      .order("created_at", { ascending: false })
      .limit(100);
    strategies = data ?? [];
  }

  const visibleStrategies = filteredStrategies(strategies, query);
  const selected = visibleStrategies.find((strategy) => strategy.id === query.selected) ?? visibleStrategies[0];
  const exchanges = uniqueValues(strategies, exchangeFor);
  const timeframes = uniqueValues(strategies, timeframeFor);

  return (
    <main className="page-shell">
      <header className="hub-header">
        <div>
          <h1>Strategy Hub</h1>
          <p>Review, filter, and inspect compiled specs before any execution path opens.</p>
        </div>
        <Link href="/hub/new" className="new-strategy-link">New strategy</Link>
      </header>

      {!user ? (
        <p className="muted">
          <Link href="/login" className="link">Sign in</Link> to view your strategies.
        </p>
      ) : strategies.length === 0 ? (
        <p className="muted">
          No strategies yet. <Link href="/hub/new" className="link">Upload a PDF to get started.</Link>
        </p>
      ) : (
        <>
          <form className="filter-bar">
            <label>
              Search
              <input name="q" defaultValue={query.q ?? ""} placeholder="Name, symbol, spec hash" />
            </label>
            <label>
              Status
              <select name="status" defaultValue={query.status ?? "all"}>
                <option value="all">All</option>
                <option value="needs_review">Needs review</option>
                <option value="approved">Approved</option>
                <option value="rejected">Rejected</option>
              </select>
            </label>
            <label>
              Exchange
              <select name="exchange" defaultValue={query.exchange ?? "all"}>
                <option value="all">All</option>
                {exchanges.map((exchange) => <option key={exchange} value={exchange}>{exchange}</option>)}
              </select>
            </label>
            <label>
              Timeframe
              <select name="timeframe" defaultValue={query.timeframe ?? "all"}>
                <option value="all">All</option>
                {timeframes.map((timeframe) => <option key={timeframe} value={timeframe}>{timeframe}</option>)}
              </select>
            </label>
            <label>
              Sort
              <select name="sort" defaultValue={query.sort ?? "created_desc"}>
                <option value="created_desc">Newest</option>
                <option value="created_asc">Oldest</option>
                <option value="name_asc">Name</option>
                <option value="status_asc">Status</option>
              </select>
            </label>
            <button>Apply</button>
          </form>

          <div className="hub-grid">
            <section>
              {visibleStrategies.length === 0 ? (
                <p className="muted">No strategies match the current filters.</p>
              ) : (
                <table className="strategy-table">
                  <thead>
                    <tr>
                      <th>Strategy</th>
                      <th>Status</th>
                      <th>Risk</th>
                      <th>Created</th>
                      <th></th>
                    </tr>
                  </thead>
                  <tbody>
                    {visibleStrategies.map((strategy) => {
                      const badge = statusStyle(strategy.status);
                      const selectedParams = new URLSearchParams();
                      Object.entries(query).forEach(([key, value]) => {
                        if (value && key !== "selected") selectedParams.set(key, value);
                      });
                      selectedParams.set("selected", strategy.id);
                      return (
                        <tr key={strategy.id}>
                          <td>
                            <div className="strategy-name">{nameFor(strategy)}</div>
                            <div className="strategy-subtext">{symbolsFor(strategy)} · {exchangeFor(strategy)} · {timeframeFor(strategy)}</div>
                            <code>{strategy.spec_hash.slice(0, 16)}</code>
                          </td>
                          <td>
                            <span className="status-badge" style={{ background: badge.background, color: badge.color, borderColor: badge.border }}>
                              {strategy.status.replace("_", " ")}
                            </span>
                          </td>
                          <td>{riskSummary(strategy)}</td>
                          <td>{new Date(strategy.created_at).toLocaleDateString()}</td>
                          <td><Link href={`/hub?${selectedParams.toString()}`} className="link">Details</Link></td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              )}
            </section>

            <aside className="side-sheet">
              {selected ? (
                <>
                  <h2>{nameFor(selected)}</h2>
                  <span
                    className="status-badge"
                    style={{
                      background: statusStyle(selected.status).background,
                      color: statusStyle(selected.status).color,
                      borderColor: statusStyle(selected.status).border,
                    }}
                  >
                    {selected.status.replace("_", " ")}
                  </span>
                  <dl>
                    <div>
                      <dt>Spec hash</dt>
                      <dd><code>{selected.spec_hash}</code></dd>
                    </div>
                    <div>
                      <dt>Market</dt>
                      <dd>{symbolsFor(selected)} · {exchangeFor(selected)} · {timeframeFor(selected)}</dd>
                    </div>
                    <div>
                      <dt>Risk</dt>
                      <dd>{riskSummary(selected)}</dd>
                    </div>
                    <div>
                      <dt>Review</dt>
                      <dd>{selected.reviewed_at ? new Date(selected.reviewed_at).toLocaleString() : "Pending"}</dd>
                    </div>
                  </dl>
                  <div className="side-actions">
                    <Link href={`/strategies/${selected.id}/review`}>Open review</Link>
                    <Link href={selected.status === "approved" ? `/strategies/${selected.id}/backtest` : `/strategies/${selected.id}/review`}>
                      {selected.status === "approved" ? "Backtest" : "Backtest locked"}
                    </Link>
                    <Link href={selected.status === "approved" ? `/strategies/${selected.id}/validate` : `/strategies/${selected.id}/review`}>
                      {selected.status === "approved" ? "Validate" : "Validate locked"}
                    </Link>
                    <Link href={selected.status === "approved" ? `/strategies/${selected.id}/live` : `/strategies/${selected.id}/review`}>
                      {selected.status === "approved" ? "Go live" : "Live locked"}
                    </Link>
                  </div>
                </>
              ) : (
                <p className="muted">Select a strategy to inspect details.</p>
              )}
            </aside>
          </div>
        </>
      )}
    </main>
  );
}
