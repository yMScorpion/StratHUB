import Link from "next/link";

export default function Home() {
  return (
    <main style={{ padding: "4rem", maxWidth: 720 }}>
      <h1 style={{ fontWeight: 600, fontSize: "2rem", letterSpacing: "-0.02em" }}>
        StratHUB
      </h1>
      <p style={{ opacity: 0.7, lineHeight: 1.6 }}>
        Upload trading methodology PDFs. DeepSeek reads them, compiles a Strategy Spec, and
        queues it for your review — before any capital is risked.
      </p>
      <ul style={{ marginTop: "2rem", lineHeight: 1.9 }}>
        <li>
          <Link href="/hub" style={{ color: "#2563eb" }}>
            /hub
          </Link>{" "}
          — Strategy Hub
        </li>
        <li>
          <Link href="/hub/new" style={{ color: "#2563eb" }}>
            /hub/new
          </Link>{" "}
          — Upload PDFs + run pipeline
        </li>
        <li>
          <code>/strategies/[id]/review</code> — Human review gate (Phase 3)
        </li>
        <li>
          <code>/strategies/[id]/backtest</code> — Backtest results (Phase 4)
        </li>
        <li>
          <code>/strategies/[id]/validate</code> — 7-day paper validation (Phase 5)
        </li>
        <li>
          <code>/accounts</code> — Paper / live separation (Phase 6)
        </li>
        <li>
          <code>/settings</code> — Broker keys, risk limits (Phase 6)
        </li>
      </ul>
    </main>
  );
}
