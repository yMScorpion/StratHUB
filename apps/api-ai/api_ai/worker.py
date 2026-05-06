"""Arq worker: startup/shutdown context + job functions."""

from __future__ import annotations

import logging

from arq.connections import RedisSettings

from .settings import Settings, load_settings

_LOG = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Lifecycle
# ---------------------------------------------------------------------------


async def startup(ctx: dict) -> None:
    from supabase import create_client

    settings: Settings = load_settings()
    ctx["settings"] = settings
    ctx["supabase"] = create_client(
        settings.supabase_url, settings.supabase_service_role_key
    )
    _LOG.info("Worker started — Supabase: %s", settings.supabase_url)


async def shutdown(ctx: dict) -> None:
    _LOG.info("Worker shutting down")


# ---------------------------------------------------------------------------
# Jobs
# ---------------------------------------------------------------------------


async def ingest_job(ctx: dict, job_id: str, user_id: str) -> str:
    """PDF ingestion pipeline: extract → chunk → embed → orchestrate → store."""
    settings: Settings = ctx["settings"]
    sb = ctx["supabase"]

    try:
        await _run_ingest(job_id, user_id, settings, sb)
    except Exception as exc:
        _LOG.exception("ingest_job %s failed", job_id)
        sb.table("strategy_jobs").update(
            {"status": "failed", "error": str(exc)[:2048]}
        ).eq("id", job_id).execute()
        raise

    return "ok"


async def _run_ingest(job_id: str, user_id: str, settings: Settings, sb: object) -> None:
    import asyncio
    import datetime as _dt

    from strategy_spec import SchemaValidationError, hash_spec, semantic_check, validate, with_hash

    from .ingest.chunker import chunk_pages
    from .ingest.embedder import embed_chunks
    from .ingest.models import Chunk, PageContent
    from .ingest.orchestrator import generate_spec
    from .ingest.pdf import extract_pdf

    # Mark as processing
    sb.table("strategy_jobs").update({"status": "processing"}).eq("id", job_id).execute()

    # Fetch registered uploads
    uploads_resp = (
        sb.table("pdf_uploads")
        .select("id,filename,sha256,storage_path,status")
        .eq("job_id", job_id)
        .eq("user_id", user_id)
        .execute()
    )
    uploads = uploads_resp.data or []
    if not uploads:
        raise ValueError("No PDF uploads found for job")

    all_chunks: list[Chunk] = []
    pdf_ids: list[str] = []
    pdf_filenames: list[str] = []

    for upload in uploads:
        upload_id: str = upload["id"]
        filename: str = upload["filename"]
        storage_path: str = upload["storage_path"]

        sb.table("pdf_uploads").update({"status": "processing"}).eq("id", upload_id).execute()

        # Download from Supabase Storage
        content: bytes = sb.storage.from_("pdfs").download(storage_path)

        # Extract text
        extracted = extract_pdf(
            content,
            upload_id=upload_id,
            filename=filename,
            max_pages=settings.max_pages_per_pdf,
        )

        # Update page_count and OCR confidence
        sb.table("pdf_uploads").update(
            {
                "page_count": extracted.page_count,
                "ocr_confidence": extracted.avg_ocr_confidence,
                "status": "done",
            }
        ).eq("id", upload_id).execute()

        chunks = chunk_pages(extracted.pages, upload_id=upload_id)
        all_chunks.extend(chunks)
        pdf_ids.append(upload_id)
        pdf_filenames.append(filename)

    if not all_chunks:
        raise ValueError("No text could be extracted from the uploaded PDFs")

    # Embed chunks and store — runs concurrently with orchestrator call
    async def _embed_and_store() -> None:
        embedded = await embed_chunks(all_chunks, settings)
        rows = [
            {
                "user_id": user_id,
                "job_id": job_id,
                "upload_id": ec.upload_id,
                "chunk_index": ec.chunk_index,
                "chunk_text": ec.text,
                "embedding": ec.embedding,
                "embedding_model": ec.embedding_model,
                "embedding_dim": ec.embedding_dim,
                "source_page": ec.source_page,
            }
            for ec in embedded
        ]
        # Supabase SDK insert in batches of 500 to stay within payload limits
        batch = 500
        for i in range(0, len(rows), batch):
            sb.table("strategy_embeddings").insert(rows[i : i + batch]).execute()

    embed_task = asyncio.create_task(_embed_and_store())
    spec_task = asyncio.create_task(
        generate_spec(all_chunks, pdf_ids, pdf_filenames, settings)
    )

    spec, _ = await asyncio.gather(spec_task, embed_task)

    # Final validation pass (orchestrator already validates, but belt-and-suspenders)
    validate(spec)
    problems = semantic_check(spec)
    if problems:
        raise ValueError("; ".join(p.message for p in problems))

    spec_hash_val = hash_spec(spec)

    # Check for duplicate spec_hash for this user (idempotency)
    dup = (
        sb.table("strategies")
        .select("id")
        .eq("user_id", user_id)
        .eq("spec_hash", spec_hash_val)
        .limit(1)
        .execute()
    )
    if dup.data:
        _LOG.info("Duplicate spec_hash %s for user %s; skipping insert", spec_hash_val, user_id)
    else:
        sb.table("strategies").insert(
            {
                "user_id": user_id,
                "job_id": job_id,
                "spec_jsonb": spec,
                "spec_hash": spec_hash_val,
                "status": "needs_review",
            }
        ).execute()

    now = _dt.datetime.now(_dt.UTC).isoformat()
    sb.table("strategy_jobs").update(
        {"status": "needs_review", "completed_at": now}
    ).eq("id", job_id).execute()


# ---------------------------------------------------------------------------
# WorkerSettings
# ---------------------------------------------------------------------------


class WorkerSettings:
    """`arq apps.api-ai.api_ai.worker.WorkerSettings` to run."""

    functions = [ingest_job]
    on_startup = startup
    on_shutdown = shutdown
    keep_result = 3600
    max_tries = 1
    # Must be a class-level attribute (not a classmethod) — arq reads it via class.__dict__.
    redis_settings = RedisSettings.from_dsn(load_settings().redis_url)
