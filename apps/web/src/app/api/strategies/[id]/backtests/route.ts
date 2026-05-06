import { NextResponse } from "next/server";
import { createSupabaseServerClientReadonly } from "@/lib/supabase/server";

export async function POST(
  _request: Request,
  { params }: { params: Promise<{ id: string }> },
) {
  const { id } = await params;
  const supabase = await createSupabaseServerClientReadonly();
  const {
    data: { user },
  } = await supabase.auth.getUser();

  if (!user) {
    return NextResponse.json({ error: "unauthorized" }, { status: 401 });
  }

  const { data: strategyRaw, error } = await supabase
    .from("strategies")
    .select("id, status, spec_hash")
    .eq("id", id)
    .single();
  const strategy = strategyRaw as
    | { id: string; status: "needs_review" | "approved" | "rejected"; spec_hash: string }
    | null;

  if (error || !strategy) {
    return NextResponse.json({ error: "strategy not found" }, { status: 404 });
  }
  if (strategy.status !== "approved") {
    return NextResponse.json(
      { error: "strategy must be approved before backtest" },
      { status: 409 },
    );
  }

  return NextResponse.json(
    {
      queued: true,
      strategy_id: strategy.id,
      spec_hash: strategy.spec_hash,
      message: "Backtest enqueue wiring lands with the exec-rs worker deployment.",
    },
    { status: 202 },
  );
}
