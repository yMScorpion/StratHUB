import Link from "next/link";

export default function Home() {
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

      <section className="hero-grid" aria-labelledby="home-title">
        <div className="hero">
          <div>
            <p className="eyebrow">AI strategy operations</p>
            <h1 className="page-title" id="home-title">StratHUB</h1>
            <p className="lede">
              Upload trading methodology PDFs, compile them into deterministic Strategy Specs,
              and hold every strategy at a human review gate before capital is risked.
            </p>
            <div className="actions">
              <Link className="button" href="/hub/new">New strategy</Link>
              <Link className="button button-secondary" href="/hub">Open hub</Link>
            </div>
          </div>

          <div className="metric-grid" aria-label="Platform controls">
            <div className="metric">
              <span className="metric-value">50</span>
              <span className="metric-label">PDF methodology files per run</span>
            </div>
            <div className="metric">
              <span className="metric-value">25 MB</span>
              <span className="metric-label">Maximum source document size</span>
            </div>
            <div className="metric">
              <span className="metric-value">0</span>
              <span className="metric-label">Live orders before approval</span>
            </div>
          </div>
        </div>

        <section className="panel panel-pad" aria-label="Workflow routes">
          <p className="eyebrow">Workflow</p>
          <ul className="route-list">
            <li>
              <Link className="route-item" href="/hub">
                <span className="route-main">
                  <span className="route-label">Strategy Hub</span>
                  <span className="route-detail">Review compiled strategies and their current state.</span>
                </span>
                <span aria-hidden="true">-&gt;</span>
              </Link>
            </li>
            <li>
              <Link className="route-item" href="/hub/new">
                <span className="route-main">
                  <span className="route-label">Upload pipeline</span>
                  <span className="route-detail">Add PDFs, hash inputs, and submit an ingestion job.</span>
                </span>
                <span aria-hidden="true">-&gt;</span>
              </Link>
            </li>
            <li className="route-item">
              <span className="route-main">
                <span className="route-label">Human review gate</span>
                <span className="route-detail">Inspect Strategy Specs before backtest and validation phases.</span>
              </span>
              <code className="mono">Phase 3</code>
            </li>
          </ul>
        </section>
      </section>
    </main>
  );
}
