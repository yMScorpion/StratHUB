import Link from "next/link";
import { createSupabaseServerClientReadonly } from "@/lib/supabase/server";

export const dynamic = "force-dynamic";

export default async function HubPage() {
  const supabase = await createSupabaseServerClientReadonly();
  const { data: { user } } = await supabase.auth.getUser();

  let strategies: Array<{
    id: string;
    spec_hash: string;
    status: string;
    created_at: string;
  }> = [];

  if (user) {
    const { data } = await supabase
      .from("strategies")
      .select("id, spec_hash, status, created_at")
      .order("created_at", { ascending: false })
      .limit(50);
    strategies = data ?? [];
  }

  const statusColor: Record<string, string> = {
    needs_review: "#f59e0b",
    approved: "#22c55e",
    rejected: "#ef4444",
  };

  return (
    <main style={{ padding: "2rem 4rem", maxWidth: 900 }}>
      <div style={{ display: "flex", alignItems: "center", gap: "2rem", marginBottom: "2rem" }}>
        <h1 style={{ fontWeight: 600, fontSize: "1.75rem" }}>Strategy Hub</h1>
        <Link
          href="/hub/new"
          style={{
            background: "#2563eb",
            color: "#fff",
            padding: "0.5rem 1.2rem",
            borderRadius: 6,
            textDecoration: "none",
            fontSize: "0.9rem",
          }}
        >
          + New strategy
        </Link>
      </div>

      {strategies.length === 0 ? (
        <p style={{ opacity: 0.6 }}>
          No strategies yet.{" "}
          <Link href="/hub/new" style={{ color: "#2563eb" }}>
            Upload a PDF to get started.
          </Link>
        </p>
      ) : (
        <table style={{ width: "100%", borderCollapse: "collapse" }}>
          <thead>
            <tr style={{ borderBottom: "1px solid #e5e7eb", textAlign: "left" }}>
              <th style={{ padding: "0.5rem 0.75rem" }}>Spec hash</th>
              <th style={{ padding: "0.5rem 0.75rem" }}>Status</th>
              <th style={{ padding: "0.5rem 0.75rem" }}>Created</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {strategies.map((s) => (
              <tr key={s.id} style={{ borderBottom: "1px solid #f3f4f6" }}>
                <td style={{ padding: "0.6rem 0.75rem", fontFamily: "monospace", fontSize: "0.8rem" }}>
                  {s.spec_hash.slice(0, 12)}…
                </td>
                <td style={{ padding: "0.6rem 0.75rem" }}>
                  <span
                    style={{
                      background: statusColor[s.status] ?? "#9ca3af",
                      color: "#fff",
                      borderRadius: 4,
                      padding: "0.15rem 0.5rem",
                      fontSize: "0.75rem",
                    }}
                  >
                    {s.status}
                  </span>
                </td>
                <td style={{ padding: "0.6rem 0.75rem", fontSize: "0.85rem", opacity: 0.7 }}>
                  {new Date(s.created_at).toLocaleDateString()}
                </td>
                <td style={{ padding: "0.6rem 0.75rem" }}>
                  <Link href={`/strategies/${s.id}/review`} style={{ color: "#2563eb", fontSize: "0.85rem" }}>
                    Review →
                  </Link>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </main>
  );
}
