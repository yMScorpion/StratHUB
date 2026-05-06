import Link from "next/link";
import { notFound } from "next/navigation";
import { createSupabaseServerClientReadonly } from "@/lib/supabase/server";
import type { Json } from "@/lib/supabase/types";
import { submitReviewDecision } from "./actions";

export const dynamic = "force-dynamic";

type SearchParams = Promise<{
  review_action?: string;
  review_error?: string;
}>;

type SpecObject = Record<string, unknown>;

type Citation = {
  rule_ref: string;
  pdf_id: string;
  pages: number[];
  quote?: string;
};

type AuditEvent = {
  id: string;
  action: string;
  metadata: Json;
  created_at: string;
};

type StrategyDetail = {
  id: string;
  spec_jsonb: Json;
  spec_hash: string;
  status: "needs_review" | "approved" | "rejected";
  reviewed_at: string | null;
  created_at: string;
};

function isObject(value: unknown): value is SpecObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function asSpec(value: Json): SpecObject {
  return isObject(value) ? value : {};
}

function asArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function asCitations(spec: SpecObject): Citation[] {
  return asArray(spec.citations).flatMap((value) => {
    if (!isObject(value) || typeof value.rule_ref !== "string" || typeof value.pdf_id !== "string") {
      return [];
    }
    return [{
      rule_ref: value.rule_ref,
      pdf_id: value.pdf_id,
      pages: asArray(value.pages).filter((page): page is number => typeof page === "number"),
      quote: typeof value.quote === "string" ? value.quote : undefined,
    }];
  });
}

function compact(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return JSON.stringify(value);
}

function ruleTitle(section: string, index: number, rule: unknown): string {
  if (!isObject(rule)) return `${section}[${index}]`;
  const id = typeof rule.id === "string" ? rule.id : undefined;
  const kind = typeof rule.kind === "string" ? rule.kind : undefined;
  const side = typeof rule.side === "string" ? rule.side : undefined;
  return [id, kind, side].filter(Boolean).join(" · ") || `${section}[${index}]`;
}

function citationsFor(citations: Citation[], ref: string): Citation[] {
  return citations.filter((citation) => citation.rule_ref === ref || citation.rule_ref.startsWith(`${ref}.`));
}

function formatPages(pages: number[]): string {
  return pages.length === 1 ? `p. ${pages[0]}` : `pp. ${pages.join(", ")}`;
}

function metadataString(spec: SpecObject, key: string): string | null {
  const metadata = isObject(spec.metadata) ? spec.metadata : {};
  const value = metadata[key];
  return typeof value === "string" && value.trim() ? value : null;
}

function assumptionItems(spec: SpecObject): string[] {
  const metadata = isObject(spec.metadata) ? spec.metadata : {};
  return Object.entries(metadata)
    .filter(([key]) => key.toLowerCase().includes("assumption"))
    .map(([, value]) => compact(value))
    .filter(Boolean);
}

function statusStyle(status: string) {
  if (status === "approved") return { background: "#dcfce7", color: "#166534", border: "#86efac" };
  if (status === "rejected") return { background: "#fee2e2", color: "#991b1b", border: "#fecaca" };
  return { background: "#fef3c7", color: "#92400e", border: "#fde68a" };
}

export default async function ReviewPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: SearchParams;
}) {
  const { id } = await params;
  const query = await searchParams;
  const supabase = await createSupabaseServerClientReadonly();
  const {
    data: { user },
  } = await supabase.auth.getUser();

  if (!user) {
    return (
      <main className="page-shell">
        <Link href="/login" className="link">Sign in to review strategies</Link>
      </main>
    );
  }

  const strategyResult = await supabase
    .from("strategies")
    .select("id, spec_jsonb, spec_hash, status, reviewed_at, created_at")
    .eq("id", id)
    .single();
  const strategy = strategyResult.data as StrategyDetail | null;

  if (!strategy) notFound();

  const { data: auditRows } = await supabase
    .from("audit_log")
    .select("id, action, metadata, created_at")
    .eq("entity_type", "strategy")
    .eq("entity_id", id)
    .order("created_at", { ascending: false })
    .limit(10);

  const auditEvents: AuditEvent[] = auditRows ?? [];
  const spec = asSpec(strategy.spec_jsonb);
  const citations = asCitations(spec);
  const name = metadataString(spec, "name") ?? "Untitled strategy";
  const assumptions = assumptionItems(spec);
  const badge = statusStyle(strategy.status);
  const reviewAction = submitReviewDecision.bind(null, strategy.id);
  const locked = strategy.status !== "needs_review";

  const sections: Array<{ key: string; label: string }> = [
    { key: "patterns", label: "Patterns" },
    { key: "indicators", label: "Indicators" },
    { key: "filters", label: "Filters" },
    { key: "entries", label: "Entries" },
    { key: "exits", label: "Exits" },
  ];

  return (
    <main className="page-shell">
      <div className="crumbs">
        <Link href="/hub" className="link">Strategy Hub</Link>
        <span>/</span>
        <span>Review</span>
      </div>

      <header className="review-header">
        <div>
          <h1>{name}</h1>
          <p>
            {String(spec.exchange ?? "exchange")} · {String(spec.symbols ?? "symbols")} ·{" "}
            {String(spec.timeframe ?? "timeframe")}
          </p>
          <code>{strategy.spec_hash}</code>
        </div>
        <span className="status-badge" style={{ background: badge.background, color: badge.color, borderColor: badge.border }}>
          {strategy.status.replace("_", " ")}
        </span>
      </header>

      {query.review_error ? (
        <div className="notice error">{decodeURIComponent(query.review_error)}</div>
      ) : null}
      {query.review_action ? (
        <div className="notice success">Review action recorded: {query.review_action.replace("_", " ")}.</div>
      ) : null}

      <section className="summary-grid">
        <div>
          <h2>Risk Model</h2>
          <dl className="kv-list">
            {Object.entries(isObject(spec.risk) ? spec.risk : {}).map(([key, value]) => (
              <div key={key}>
                <dt>{key.replaceAll("_", " ")}</dt>
                <dd>{compact(value)}</dd>
              </div>
            ))}
          </dl>
        </div>
        <div>
          <h2>Assumptions</h2>
          {assumptions.length > 0 ? (
            <ul className="plain-list">
              {assumptions.map((assumption, index) => <li key={index}>{assumption}</li>)}
            </ul>
          ) : (
            <p className="muted">No explicit assumptions were emitted in metadata. Use request changes if this needs to be clarified before approval.</p>
          )}
        </div>
      </section>

      <section className="review-grid">
        <div>
          <h2>Extracted Rules</h2>
          {sections.map((section) => (
            <div key={section.key} className="rule-section">
              <h3>{section.label}</h3>
              {asArray(spec[section.key]).length === 0 ? (
                <p className="muted">None</p>
              ) : (
                asArray(spec[section.key]).map((rule, index) => {
                  const ref = `${section.key}[${index}]`;
                  const matchedCitations = citationsFor(citations, ref);
                  return (
                    <article key={ref} className="rule-card">
                      <div>
                        <strong>{ruleTitle(section.key, index, rule)}</strong>
                        <span>{ref}</span>
                      </div>
                      <pre>{JSON.stringify(rule, null, 2)}</pre>
                      {matchedCitations.length > 0 ? (
                        <ul className="citation-list">
                          {matchedCitations.map((citation) => (
                            <li key={`${citation.rule_ref}-${citation.pdf_id}-${citation.pages.join("-")}`}>
                              <strong>{formatPages(citation.pages)}</strong>
                              <span>{citation.quote ?? "Citation present without quote."}</span>
                              <code>{citation.pdf_id.slice(0, 8)}</code>
                            </li>
                          ))}
                        </ul>
                      ) : (
                        <p className="missing-citation">No citation found for {ref}</p>
                      )}
                    </article>
                  );
                })
              )}
            </div>
          ))}

          <div className="rule-section">
            <h3>Risk Citations</h3>
            {citationsFor(citations, "risk").map((citation) => (
              <article className="rule-card" key={`${citation.rule_ref}-${citation.pages.join("-")}`}>
                <strong>{citation.rule_ref}</strong>
                <p>{citation.quote ?? "Citation present without quote."}</p>
                <small>{formatPages(citation.pages)} · {citation.pdf_id}</small>
              </article>
            ))}
          </div>
        </div>

        <aside className="decision-panel">
          <h2>Decision</h2>
          <p className="muted">
            Approval unlocks backtest, paper validation, and live deployment paths for this immutable spec hash.
          </p>
          {locked ? (
            <p className="locked">This strategy is {strategy.status}. Review decisions are terminal.</p>
          ) : (
            <form action={reviewAction}>
              <label htmlFor="notes">Reviewer notes</label>
              <textarea id="notes" name="notes" rows={6} placeholder="Record concerns, changes requested, or approval rationale." />
              <div className="button-row">
                <button name="action" value="approve" className="approve">Approve</button>
                <button name="action" value="request_changes" className="request">Request changes</button>
                <button name="action" value="reject" className="reject">Reject</button>
              </div>
            </form>
          )}

          <h2>Audit</h2>
          {auditEvents.length === 0 ? (
            <p className="muted">No review events yet.</p>
          ) : (
            <ol className="audit-list">
              {auditEvents.map((event) => (
                <li key={event.id}>
                  <strong>{event.action.replace("strategy.review.", "").replace("_", " ")}</strong>
                  <time>{new Date(event.created_at).toLocaleString()}</time>
                  {isObject(event.metadata) && typeof event.metadata.notes === "string" ? (
                    <p>{event.metadata.notes}</p>
                  ) : null}
                </li>
              ))}
            </ol>
          )}
        </aside>
      </section>
    </main>
  );
}
