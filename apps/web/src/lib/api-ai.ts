/**
 * Thin server-side client for the api-ai internal service.
 * All calls include the authenticated user's ID via X-User-Id header.
 */

const API_AI_BASE = process.env.API_AI_BASE_URL ?? "http://localhost:8000";

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
      ...(init?.headers as Record<string, string> | undefined),
    },
  });
}
