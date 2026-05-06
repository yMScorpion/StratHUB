"use server";

import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";
import { createSupabaseServerClient } from "@/lib/supabase/server";

type ReviewAction = "approve" | "reject" | "request_changes";

function getAction(value: FormDataEntryValue | null): ReviewAction {
  if (value === "approve" || value === "reject" || value === "request_changes") {
    return value;
  }
  throw new Error("Invalid review action");
}

export async function submitReviewDecision(strategyId: string, formData: FormData) {
  const action = getAction(formData.get("action"));
  const notes = String(formData.get("notes") ?? "").trim() || null;

  const supabase = await createSupabaseServerClient();
  const {
    data: { user },
  } = await supabase.auth.getUser();

  if (!user) {
    redirect("/login");
  }

  const { error } = await supabase.rpc("review_strategy" as never, {
    p_strategy_id: strategyId,
    p_action: action,
    p_notes: notes,
  } as never);

  if (error) {
    const message = encodeURIComponent(error.message);
    redirect(`/strategies/${strategyId}/review?review_error=${message}`);
  }

  revalidatePath("/hub");
  revalidatePath(`/strategies/${strategyId}/review`);
  redirect(`/strategies/${strategyId}/review?review_action=${action}`);
}
