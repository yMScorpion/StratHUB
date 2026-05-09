// Human review gate — Phase 3.

import Link from "next/link";

export default async function ReviewPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
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

      <section className="panel empty-state" aria-labelledby="review-title">
        <div className="empty-state-inner">
          <div className="empty-icon" aria-hidden="true">R</div>
          <p className="eyebrow">Human review gate</p>
          <h1 className="section-title" id="review-title">Strategy review</h1>
          <p className="subtle">
            Strategy <code className="mono review-id">{id}</code> is ready for the full Phase 3 review surface.
          </p>
          <div className="actions actions-center">
            <Link className="button button-secondary" href="/hub">Back to hub</Link>
          </div>
        </div>
      </section>
    </main>
  );
}
