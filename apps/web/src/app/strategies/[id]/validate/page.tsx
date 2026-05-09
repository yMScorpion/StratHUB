import Link from "next/link";
import { createSupabaseServerClientReadonly } from "@/lib/supabase/server";

export const dynamic = "force-dynamic";

type ScorecardResult = {
  passed?: boolean;
  reasons?: string[];
};

function asScorecardResult(value: unknown): ScorecardResult {
  if (!value || typeof value !== "object") return {};
  return value as ScorecardResult;
}

export default async function ValidatePage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const supabase = await createSupabaseServerClientReadonly();
  const {
    data: { user },
  } = await supabase.auth.getUser();

  let runs: Array<{
    id: string;
    status: string;
    exchange: string;
    fly_region: string;
    ttl_expires_at: string;
    scorecard_result: unknown;
    created_at: string;
  }> = [];

  if (user) {
    const { data } = await supabase
      .from("validation_runs")
      .select("id,status,exchange,fly_region,ttl_expires_at,scorecard_result,created_at")
      .eq("strategy_id", id)
      .order("created_at", { ascending: false })
      .limit(20);
    runs = data ?? [];
  }

  return (
    <main style={{ padding: "2rem 4rem", maxWidth: 980 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <div>
          <h1 style={{ fontWeight: 600, fontSize: "1.5rem", margin: 0 }}>
            Paper validation
          </h1>
          <p style={{ opacity: 0.68, marginTop: "0.5rem", lineHeight: 1.5 }}>
            Strategy <code>{id}</code> runs against live market data with virtual fills.
          </p>
        </div>
        <Link href="/hub" style={{ color: "#2563eb", fontSize: "0.9rem" }}>
          Back to hub
        </Link>
      </div>

      <section style={{ marginTop: "2rem" }}>
        <h2 style={{ fontSize: "1rem", fontWeight: 600 }}>Validation runs</h2>
        {runs.length === 0 ? (
          <p style={{ opacity: 0.62 }}>
            No validation runs yet. Approved strategies can be provisioned for a 7-day paper run.
          </p>
        ) : (
          <table style={{ width: "100%", borderCollapse: "collapse", marginTop: "0.75rem" }}>
            <thead>
              <tr style={{ borderBottom: "1px solid #e5e7eb", textAlign: "left" }}>
                <th style={{ padding: "0.55rem" }}>Run</th>
                <th style={{ padding: "0.55rem" }}>Status</th>
                <th style={{ padding: "0.55rem" }}>Venue</th>
                <th style={{ padding: "0.55rem" }}>TTL</th>
                <th style={{ padding: "0.55rem" }}>Scorecard explanation</th>
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => {
                const scorecard = asScorecardResult(run.scorecard_result);
                const reasons = scorecard.reasons ?? [];
                return (
                  <tr key={run.id} style={{ borderBottom: "1px solid #f1f5f9" }}>
                    <td style={{ padding: "0.65rem", fontFamily: "monospace", fontSize: "0.8rem" }}>
                      {run.id.slice(0, 8)}
                    </td>
                    <td style={{ padding: "0.65rem" }}>{run.status}</td>
                    <td style={{ padding: "0.65rem" }}>
                      {run.exchange} / {run.fly_region}
                    </td>
                    <td style={{ padding: "0.65rem", fontSize: "0.85rem" }}>
                      {new Date(run.ttl_expires_at).toLocaleString()}
                    </td>
                    <td style={{ padding: "0.65rem", lineHeight: 1.45 }}>
                      {reasons.length === 0
                        ? scorecard.passed
                          ? "Passed all configured checks."
                          : "Awaiting 7-day scorecard."
                        : reasons.join(" ")}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </section>
    </main>
  );
}
