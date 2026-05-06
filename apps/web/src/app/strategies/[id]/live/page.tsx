import Link from "next/link";
import { assertStrategyApprovedForExecution } from "@/lib/strategy-execution-gate";

export const dynamic = "force-dynamic";

export default async function LiveGatePage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const gate = await assertStrategyApprovedForExecution(id);

  if (!gate.ok) {
    return (
      <main className="page-shell">
        <h1>Live trading locked</h1>
        <p className="muted">{gate.reason}</p>
        <Link href={`/strategies/${id}/review`} className="link">Open review gate</Link>
      </main>
    );
  }

  return (
    <main className="page-shell">
      <h1>Go live</h1>
      <p className="muted">Approved strategy accepted by the execution gate. Live deployment safeguards ship in Phase 6.</p>
    </main>
  );
}
