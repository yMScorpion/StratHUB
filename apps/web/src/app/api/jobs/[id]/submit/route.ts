import { type NextRequest, NextResponse } from "next/server";
import { createSupabaseServerClientReadonly } from "@/lib/supabase/server";
import { apiAiRequest } from "@/lib/api-ai";

export async function POST(
  _req: NextRequest,
  { params }: { params: Promise<{ id: string }> },
): Promise<NextResponse> {
  const supabase = await createSupabaseServerClientReadonly();
  const { data: { user } } = await supabase.auth.getUser();
  if (!user) return NextResponse.json({ error: "Unauthorized" }, { status: 401 });

  const { id } = await params;
  const upstream = await apiAiRequest(`/jobs/${id}/submit`, user.id, { method: "POST" });
  const data = await upstream.json() as unknown;
  return NextResponse.json(data, { status: upstream.status });
}
