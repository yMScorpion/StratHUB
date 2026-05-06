import { createSupabaseServerClientReadonly } from "@/lib/supabase/server";

export async function assertStrategyApprovedForExecution(strategyId: string) {
  const supabase = await createSupabaseServerClientReadonly();
  const {
    data: { user },
  } = await supabase.auth.getUser();

  if (!user) {
    return { ok: false as const, reason: "Sign in before opening execution workflows." };
  }

  const { error } = await supabase.rpc("assert_strategy_approved" as never, {
    p_strategy_id: strategyId,
  } as never);

  if (error) {
    return { ok: false as const, reason: error.message };
  }

  return { ok: true as const };
}
