import { type NextRequest, NextResponse } from "next/server";
import { createSupabaseServerClientReadonly } from "@/lib/supabase/server";
import { apiAiRequest } from "@/lib/api-ai";

export async function POST(
  req: NextRequest,
  { params }: { params: Promise<{ id: string }> },
): Promise<NextResponse> {
  const supabase = await createSupabaseServerClientReadonly();
  const { data: { user } } = await supabase.auth.getUser();
  if (!user) return NextResponse.json({ error: "Unauthorized" }, { status: 401 });

  const { id } = await params;
  const body = await req.json() as unknown;
  const upstream = await apiAiRequest(`/jobs/${id}/pdfs`, user.id, {
    method: "POST",
    body: JSON.stringify(body),
  });

  const data = await upstream.json() as unknown;
  return NextResponse.json(data, { status: upstream.status });
}
