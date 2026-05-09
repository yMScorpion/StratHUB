"use client";

import { useCallback, useRef, useState } from "react";
import Link from "next/link";
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
    const rejected = arr.length;
    const valid = arr.filter((f) => {
      if (f.type !== "application/pdf") return false;
      if (f.size > MAX_BYTES) return false;
      return true;
    });
    if (valid.length !== rejected) {
      setGlobalError("Some files were skipped. Upload PDF files up to 25 MB each.");
    } else {
      setGlobalError(null);
    }
    setFiles((prev) => {
      const combined = [...prev, ...valid.map<FileEntry>((f) => ({
        file: f,
        sha256: null,
        uploadId: null,
        status: "hashing",
        progress: 0,
      }))].slice(0, MAX_FILES);
      if (prev.length + valid.length > MAX_FILES) {
        setGlobalError(`Only the first ${MAX_FILES} files were added.`);
      }
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

  function handleDropzoneKey(e: React.KeyboardEvent<HTMLDivElement>) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      inputRef.current?.click();
    }
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

  const isBusy = phase === "uploading" || phase === "submitting";
  const canSubmit = files.length > 0 && phase === "select";

  function fileStatusClass(status: FileEntry["status"]) {
    if (status === "done") return "status-done";
    if (status === "error") return "status-error";
    return "status-neutral";
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

      <header className="page-header">
        <div>
          <p className="eyebrow">Source ingestion</p>
          <h1 className="section-title">New strategy from PDF</h1>
          <p className="subtle">
            Add source documents, hash them locally, and submit a compile job for review.
          </p>
        </div>
        <Link className="button button-secondary" href="/hub">Back to hub</Link>
      </header>

      {phase === "done" ? (
        <section className="success-note" role="status" aria-live="polite">
          <h2 className="section-title">Job submitted</h2>
          <p>Redirecting to the Strategy Hub...</p>
          {jobId && <p className="mono">Job {jobId}</p>}
        </section>
      ) : (
        <>
          <section className="panel panel-pad">
            <div
              className="dropzone"
              onDrop={handleDrop}
              onDragOver={(e) => e.preventDefault()}
              onClick={() => inputRef.current?.click()}
              onKeyDown={handleDropzoneKey}
              role="button"
              tabIndex={0}
              aria-label="Choose PDF files to upload"
            >
              <div>
                <p className="dropzone-title">Drop PDFs here, or click to browse</p>
                <p className="dropzone-hint">
                  Max {MAX_FILES} files, 25 MB each. PDF sources only.
                </p>
              </div>
            </div>
            <input
              ref={inputRef}
              type="file"
              accept="application/pdf"
              multiple
              style={{ display: "none" }}
              onChange={handleInputChange}
            />

            {files.length > 0 ? (
              <ul className="file-list" aria-label="Selected files">
                {files.map((f, i) => (
                  <li className="file-row" key={`${f.file.name}-${i}`}>
                    <span className="file-name" title={f.file.name}>
                      {f.file.name}
                    </span>
                    <span className="file-meta">
                      {(f.file.size / 1024 / 1024).toFixed(1)} MB
                    </span>
                    <span className={`status-pill ${fileStatusClass(f.status)}`}>
                      {f.status}
                    </span>
                    {f.error && <span className="alert">{f.error}</span>}
                  </li>
                ))}
              </ul>
            ) : (
              <div className="empty-state" aria-live="polite">
                <div className="empty-state-inner">
                  <div className="empty-icon" aria-hidden="true">PDF</div>
                  <h2 className="section-title">No files selected</h2>
                  <p className="subtle">
                    Choose the source PDFs that define the methodology. They stay queued here before upload starts.
                  </p>
                </div>
              </div>
            )}

            {globalError && (
              <p className="alert" role="alert">{globalError}</p>
            )}

            <div className="actions">
              <button
                className="button"
                onClick={startUpload}
                disabled={!canSubmit}
                aria-busy={isBusy}
              >
                {phase === "uploading"
                  ? "Uploading..."
                  : phase === "submitting"
                    ? "Submitting..."
                    : `Upload and process${files.length > 0 ? ` (${files.length})` : ""}`}
              </button>
              {files.length > 0 && phase === "select" && (
                <button className="button button-secondary" type="button" onClick={() => setFiles([])}>
                  Clear
                </button>
              )}
            </div>
          </section>
        </>
      )}
    </main>
  );
}
