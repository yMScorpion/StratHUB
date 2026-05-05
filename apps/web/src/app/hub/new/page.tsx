"use client";

import { useCallback, useRef, useState } from "react";
import { useRouter } from "next/navigation";

interface FileEntry {
  file: File;
  sha256: string | null;
  uploadId: string | null;
  status: "hashing" | "pending" | "uploading" | "done" | "error";
  error?: string;
  progress: number;
}

const MAX_FILES = 50;
const MAX_BYTES = 25 * 1024 * 1024; // 25 MiB

async function sha256hex(buf: ArrayBuffer): Promise<string> {
  const hashBuf = await crypto.subtle.digest("SHA-256", buf);
  return Array.from(new Uint8Array(hashBuf))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export default function NewStrategyPage() {
  const router = useRouter();
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [jobId, setJobId] = useState<string | null>(null);
  const [phase, setPhase] = useState<"select" | "uploading" | "submitting" | "done">("select");
  const [globalError, setGlobalError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const addFiles = useCallback((incoming: FileList | File[]) => {
    const arr = Array.from(incoming);
    const valid = arr.filter((f) => {
      if (f.type !== "application/pdf") return false;
      if (f.size > MAX_BYTES) return false;
      return true;
    });
    setFiles((prev) => {
      const combined = [...prev, ...valid.map<FileEntry>((f) => ({
        file: f,
        sha256: null,
        uploadId: null,
        status: "hashing",
        progress: 0,
      }))].slice(0, MAX_FILES);
      return combined;
    });
  }, []);

  function handleDrop(e: React.DragEvent) {
    e.preventDefault();
    if (e.dataTransfer.files) addFiles(e.dataTransfer.files);
  }

  function handleInputChange(e: React.ChangeEvent<HTMLInputElement>) {
    if (e.target.files) addFiles(e.target.files);
  }

  async function startUpload() {
    setGlobalError(null);
    setPhase("uploading");

    // 1. Create job
    const jobRes = await fetch("/api/jobs", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ idempotency_key: crypto.randomUUID() }),
    });
    if (!jobRes.ok) {
      setGlobalError("Failed to create job: " + (await jobRes.text()));
      setPhase("select");
      return;
    }
    const { job_id } = await jobRes.json() as { job_id: string };
    setJobId(job_id);

    // 2. Hash + register + upload each file
    for (let i = 0; i < files.length; i++) {
      const entry = files[i];
      setFiles((prev) => prev.map((f, idx) => idx === i ? { ...f, status: "hashing" } : f));

      const buf = await entry.file.arrayBuffer();
      const hash = await sha256hex(buf);

      setFiles((prev) => prev.map((f, idx) => idx === i ? { ...f, sha256: hash, status: "uploading", progress: 0 } : f));

      // Register with api-ai (via Next.js route handler)
      const regRes = await fetch(`/api/jobs/${job_id}/pdfs`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          filename: entry.file.name,
          sha256: hash,
          file_size_bytes: entry.file.size,
        }),
      });
      if (!regRes.ok) {
        setFiles((prev) => prev.map((f, idx) => idx === i ? { ...f, status: "error", error: "Registration failed" } : f));
        continue;
      }
      const { upload_id, upload_url } = await regRes.json() as { upload_id: string; upload_url: string };

      // Upload to Supabase Storage via signed PUT URL
      const upRes = await fetch(upload_url, {
        method: "PUT",
        headers: { "Content-Type": "application/pdf" },
        body: buf,
      });
      if (!upRes.ok) {
        setFiles((prev) => prev.map((f, idx) => idx === i ? { ...f, status: "error", error: "Upload failed" } : f));
        continue;
      }

      // Confirm
      await fetch(`/api/jobs/${job_id}/pdfs/${upload_id}/confirm`, { method: "POST" });

      setFiles((prev) => prev.map((f, idx) => idx === i ? { ...f, uploadId: upload_id, status: "done", progress: 100 } : f));
    }

    // 3. Submit
    setPhase("submitting");
    const subRes = await fetch(`/api/jobs/${job_id}/submit`, { method: "POST" });
    if (!subRes.ok) {
      setGlobalError("Submission failed: " + (await subRes.text()));
      setPhase("uploading");
      return;
    }

    setPhase("done");
    setTimeout(() => router.push("/hub"), 1500);
  }

  const allDone = files.length > 0 && files.every((f) => f.status === "done");

  return (
    <main style={{ padding: "2rem 4rem", maxWidth: 700 }}>
      <h1 style={{ fontWeight: 600, fontSize: "1.5rem", marginBottom: "1.5rem" }}>
        New strategy from PDF
      </h1>

      {phase === "done" ? (
        <p style={{ color: "#22c55e", fontSize: "1.1rem" }}>
          ✓ Job submitted! Redirecting to hub…
        </p>
      ) : (
        <>
          {/* Drop zone */}
          <div
            onDrop={handleDrop}
            onDragOver={(e) => e.preventDefault()}
            onClick={() => inputRef.current?.click()}
            style={{
              border: "2px dashed #d1d5db",
              borderRadius: 10,
              padding: "3rem",
              textAlign: "center",
              cursor: "pointer",
              marginBottom: "1.5rem",
              background: "#f9fafb",
            }}
          >
            <p style={{ opacity: 0.6, margin: 0 }}>
              Drop PDF files here, or click to browse
            </p>
            <p style={{ opacity: 0.4, fontSize: "0.8rem", margin: "0.4rem 0 0" }}>
              Max {MAX_FILES} files · 25 MB each · PDF only
            </p>
            <input
              ref={inputRef}
              type="file"
              accept="application/pdf"
              multiple
              style={{ display: "none" }}
              onChange={handleInputChange}
            />
          </div>

          {/* File list */}
          {files.length > 0 && (
            <ul style={{ listStyle: "none", padding: 0, marginBottom: "1.5rem" }}>
              {files.map((f, i) => (
                <li
                  key={i}
                  style={{
                    display: "flex",
                    justifyContent: "space-between",
                    alignItems: "center",
                    padding: "0.4rem 0",
                    borderBottom: "1px solid #f3f4f6",
                    fontSize: "0.875rem",
                  }}
                >
                  <span style={{ maxWidth: "70%", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {f.file.name}
                  </span>
                  <span style={{ opacity: 0.6, marginLeft: "0.5rem" }}>
                    {(f.file.size / 1024 / 1024).toFixed(1)} MB
                  </span>
                  <span
                    style={{
                      color: f.status === "done" ? "#22c55e" : f.status === "error" ? "#ef4444" : "#6b7280",
                      marginLeft: "0.75rem",
                    }}
                  >
                    {f.status === "done" ? "✓" : f.status === "error" ? "✗" : f.status}
                  </span>
                </li>
              ))}
            </ul>
          )}

          {globalError && (
            <p style={{ color: "#ef4444", marginBottom: "1rem" }}>{globalError}</p>
          )}

          <button
            onClick={startUpload}
            disabled={files.length === 0 || phase === "uploading" || phase === "submitting"}
            style={{
              padding: "0.65rem 1.5rem",
              background: "#2563eb",
              color: "#fff",
              border: "none",
              borderRadius: 6,
              cursor: "pointer",
              fontSize: "1rem",
              opacity: files.length === 0 || phase !== "select" ? 0.5 : 1,
            }}
          >
            {phase === "uploading"
              ? "Uploading…"
              : phase === "submitting"
              ? "Submitting…"
              : `Upload & process ${files.length > 0 ? `(${files.length})` : ""}`}
          </button>
        </>
      )}
    </main>
  );
}
