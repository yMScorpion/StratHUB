/**
 * Thin server-side client for the api-ai internal service.
 * All calls include the authenticated user's ID and the shared internal token.
 */

const API_AI_BASE = process.env.API_AI_BASE_URL ?? "http://localhost:8000";
const API_AI_INTERNAL_TOKEN = process.env.API_AI_INTERNAL_TOKEN ?? "replace-me";

export async function apiAiRequest(
  path: string,
  userId: string,
  init?: RequestInit,
): Promise<Response> {
  return fetch(`${API_AI_BASE}${path}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      "X-Internal-Token": API_AI_INTERNAL_TOKEN,
      "X-User-Id": userId,
      ...(init?.headers as Record<string, string> | undefined),
    },
  });
}
