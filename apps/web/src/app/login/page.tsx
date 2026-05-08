"use client";

import { useState } from "react";
import Link from "next/link";
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
    <main className="fade-in">
      <nav aria-label="Primary" className="topbar">
        <Link className="brand" href="/">
          <span className="brand-mark" aria-hidden="true">S</span>
          <span>StratHUB</span>
        </Link>
        <div className="topnav">
          <Link className="nav-link" href="/hub">Hub</Link>
          <Link className="nav-link" href="/hub/new">Upload</Link>
          <Link className="nav-link" href="/login">Sign in</Link>
        </div>
      </nav>

      <section className="split-auth" aria-labelledby="login-title">
        <div className="hero">
          <div>
            <p className="eyebrow">Secure access</p>
            <h1 className="page-title" id="login-title">Sign in to StratHUB</h1>
            <p className="lede">
              Use a magic link to access strategy compilation, review, and validation workflows.
            </p>
          </div>
        </div>
        <div className="panel panel-pad">
      {sent ? (
        <div className="success-note" role="status">
          <h2 className="section-title">Check your email</h2>
          <p>The magic link is on its way.</p>
        </div>
      ) : (
        <form className="form-stack" onSubmit={handleSubmit}>
          <label className="subtle" htmlFor="email">Email address</label>
          <input
            className="input"
            id="email"
            type="email"
            required
            placeholder="you@example.com"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
          <button
            className="button"
            type="submit"
            disabled={loading}
            aria-busy={loading}
          >
            {loading ? "Sending..." : "Send magic link"}
          </button>
          {error && <p className="alert" role="alert">{error}</p>}
        </form>
      )}
        </div>
      </section>
    </main>
  );
}
