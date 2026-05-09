/**
 * Thin server-side client for the api-ai internal service.
 * All calls include the authenticated user's ID via X-User-Id header.
 */

const API_AI_BASE = process.env.API_AI_BASE_URL ?? "http://localhost:8000";
const INTERNAL_API_KEY = process.env.INTERNAL_API_KEY ?? "replace-me";

export async function apiAiRequest(
  path: string,
  userId: string,
  init?: RequestInit,
): Promise<Response> {
  return fetch(`${API_AI_BASE}${path}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      "X-User-Id": userId,
      "X-Internal-Token": INTERNAL_API_KEY,
      ...(init?.headers as Record<string, string> | undefined),
    },
  });
}
