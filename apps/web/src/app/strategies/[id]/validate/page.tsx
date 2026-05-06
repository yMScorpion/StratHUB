import Link from "next/link";
import { assertStrategyApprovedForExecution } from "@/lib/strategy-execution-gate";

export const dynamic = "force-dynamic";

export default async function ValidateGatePage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const gate = await assertStrategyApprovedForExecution(id);

  if (!gate.ok) {
    return (
      <main className="page-shell">
        <h1>Validation locked</h1>
        <p className="muted">{gate.reason}</p>
        <Link href={`/strategies/${id}/review`} className="link">Open review gate</Link>
      </main>
    );
  }

  return (
    <main className="page-shell">
      <h1>Paper validation</h1>
      <p className="muted">Approved strategy accepted by the execution gate. Seven-day validation controls ship in Phase 5.</p>
    </main>
  );
}
