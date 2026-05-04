export default function Home() {
  return (
    <main style={{ padding: "4rem", maxWidth: 720 }}>
      <h1 style={{ fontWeight: 600, fontSize: "2rem", letterSpacing: "-0.02em" }}>
        Crypto Trading System
      </h1>
      <p style={{ opacity: 0.7, lineHeight: 1.6 }}>
        Phase 0 scaffold. The Strategy Spec contract is in place; backtest, paper, and live
        modes share a single Rust executor binary.
      </p>
      <ul style={{ marginTop: "2rem", lineHeight: 1.9 }}>
        <li>
          <code>/api/healthz</code> — service health
        </li>
        <li>
          <code>/hub</code> — Strategy Hub (placeholder)
        </li>
        <li>
          <code>/hub/new</code> — PDF upload + DeepSeek pipeline (placeholder)
        </li>
        <li>
          <code>/strategies/[id]/backtest</code> — backtest results (placeholder)
        </li>
        <li>
          <code>/strategies/[id]/review</code> — human review gate (placeholder)
        </li>
        <li>
          <code>/strategies/[id]/validate</code> — 7-day paper validation (placeholder)
        </li>
        <li>
          <code>/accounts</code> — paper / live separation (placeholder)
        </li>
        <li>
          <code>/settings</code> — broker keys, OpenRouter key, risk limits (placeholder)
        </li>
      </ul>
    </main>
  );
}
