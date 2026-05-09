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

  const statusClass: Record<string, string> = {
    needs_review: "status-review",
    approved: "status-approved",
    rejected: "status-rejected",
  };

  return (
    <main className="fade-in">
      <nav aria-label="Primary" className="topbar">
        <Link className="brand" href="/">
          <span className="brand-mark" aria-hidden="true">S</span>
          <span>StratHUB</span>
        </Link>
        <div className="topnav">
          <Link className="nav-link" href="/hub">Hub</Link>
          <Link className="nav-link" href="/hub/new">Upload</Link>
          <Link className="nav-link" href="/login">Sign in</Link>
        </div>
      </nav>

      <header className="page-header">
        <div>
          <p className="eyebrow">Review queue</p>
          <h1 className="section-title">Strategy Hub</h1>
          <p className="subtle">
            Track compiled strategies from ingestion through the approval gate.
          </p>
        </div>
        <Link className="button" href="/hub/new">
          + New strategy
        </Link>
      </header>

      {strategies.length === 0 ? (
        <section className="panel empty-state" aria-labelledby="empty-hub-title">
          <div className="empty-state-inner">
            <div className="empty-icon" aria-hidden="true">+</div>
            <h2 className="section-title" id="empty-hub-title">No strategies yet</h2>
            <p className="subtle">
              Upload a methodology PDF to create the first ingestion job and begin the review flow.
            </p>
            <div className="actions actions-center">
              <Link className="button" href="/hub/new">Upload PDFs</Link>
            </div>
          </div>
        </section>
      ) : (
        <section className="panel table-wrap" aria-label="Strategies">
          <table className="data-table">
            <thead>
              <tr>
                <th scope="col">Spec hash</th>
                <th scope="col">Status</th>
                <th scope="col">Created</th>
                <th scope="col">
                  <span className="subtle">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {strategies.map((s) => (
                <tr key={s.id}>
                  <td>
                    <code className="mono">{s.spec_hash.slice(0, 12)}...</code>
                  </td>
                  <td>
                    <span className={`status-pill ${statusClass[s.status] ?? "status-neutral"}`}>
                      {s.status.replaceAll("_", " ")}
                    </span>
                  </td>
                  <td className="subtle">
                    {new Intl.DateTimeFormat("en", {
                      month: "short",
                      day: "numeric",
                      year: "numeric",
                    }).format(new Date(s.created_at))}
                  </td>
                  <td>
                    <div className="actions">
                      <Link className="button button-secondary" href={`/strategies/${s.id}/review`}>
                        Review
                      </Link>
                      <Link className="button button-secondary" href={`/strategies/${s.id}/validate`}>
                        Validate
                      </Link>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </main>
  );
}
