import Link from "next/link";

export default function HubLoading() {
  return (
    <main className="fade-in" aria-busy="true">
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
          <div className="skeleton skeleton-line" style={{ width: 110, marginBottom: 16 }} />
          <div className="skeleton skeleton-line" style={{ width: 250, height: 34, marginBottom: 14 }} />
          <div className="skeleton skeleton-line" style={{ width: 420, maxWidth: "100%" }} />
        </div>
        <div className="skeleton" style={{ width: 136, height: 42 }} />
      </header>

      <section className="panel panel-pad" aria-label="Loading strategies">
        <div className="skeleton skeleton-row" />
        <div className="skeleton skeleton-row" />
        <div className="skeleton skeleton-row" />
        <div className="skeleton skeleton-row" />
      </section>
    </main>
  );
}
