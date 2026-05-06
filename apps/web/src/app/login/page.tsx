"use client";

import { useState } from "react";
import { createSupabaseBrowserClient } from "@/lib/supabase/client";

export default function LoginPage() {
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setLoading(true);
    setError(null);

    const supabase = createSupabaseBrowserClient();
    const { error: authError } = await supabase.auth.signInWithOtp({
      email,
      options: { emailRedirectTo: `${window.location.origin}/hub` },
    });

    setLoading(false);
    if (authError) {
      setError(authError.message);
    } else {
      setSent(true);
    }
  }

  return (
    <main style={{ padding: "4rem", maxWidth: 400, margin: "0 auto" }}>
      <h1 style={{ fontWeight: 600, fontSize: "1.5rem", marginBottom: "1.5rem" }}>
        Sign in to StratHUB
      </h1>
      {sent ? (
        <p style={{ color: "#22c55e" }}>Check your email for the magic link.</p>
      ) : (
        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
          <input
            type="email"
            required
            placeholder="you@example.com"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            style={{ padding: "0.6rem 0.8rem", fontSize: "1rem", borderRadius: 6, border: "1px solid #ccc" }}
          />
          <button
            type="submit"
            disabled={loading}
            style={{
              padding: "0.6rem 1rem",
              background: "#2563eb",
              color: "#fff",
              border: "none",
              borderRadius: 6,
              cursor: "pointer",
              fontSize: "1rem",
            }}
          >
            {loading ? "Sending…" : "Send magic link"}
          </button>
          {error && <p style={{ color: "#ef4444" }}>{error}</p>}
        </form>
      )}
    </main>
  );
}
