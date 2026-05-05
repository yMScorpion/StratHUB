"""Job management routes: create → register PDFs → submit → status."""

from __future__ import annotations

import logging
import re
import uuid
from datetime import date
from typing import Any

from fastapi import APIRouter, Depends, Header, HTTPException, Request, status
from pydantic import BaseModel, Field

_LOG = logging.getLogger(__name__)
router = APIRouter(prefix="/jobs", tags=["jobs"])

_SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


# ---------------------------------------------------------------------------
# Auth dependency: internal service trust via X-User-Id header
# ---------------------------------------------------------------------------


def _user_id(x_user_id: str = Header(..., alias="X-User-Id")) -> str:
    try:
        uuid.UUID(x_user_id)
    except ValueError:
        raise HTTPException(status_code=400, detail="X-User-Id must be a valid UUID")
    return x_user_id


# ---------------------------------------------------------------------------
# Request / response models
# ---------------------------------------------------------------------------


class CreateJobBody(BaseModel):
    idempotency_key: str = Field(min_length=1, max_length=128)


class RegisterPdfBody(BaseModel):
    filename: str = Field(min_length=1, max_length=256)
    sha256: str = Field(min_length=64, max_length=64, pattern=r"^[0-9a-f]{64}$")
    file_size_bytes: int = Field(gt=0)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _sb(request: Request) -> Any:
    return request.app.state.supabase


def _settings(request: Request) -> Any:
    return request.app.state.settings


# ---------------------------------------------------------------------------
# Routes
# ---------------------------------------------------------------------------


@router.post("", status_code=status.HTTP_201_CREATED)
async def create_job(
    request: Request,
    body: CreateJobBody,
    user_id: str = Depends(_user_id),
) -> dict[str, Any]:
    sb = _sb(request)
    settings = _settings(request)

    # Enforce daily job limit
    today = date.today().isoformat()
    count_resp = (
        sb.table("strategy_jobs")
        .select("id", count="exact")
        .eq("user_id", user_id)
        .gte("created_at", f"{today}T00:00:00Z")
        .execute()
    )
    if (count_resp.count or 0) >= settings.max_jobs_per_day:
        raise HTTPException(
            status_code=429,
            detail=f"Daily job limit of {settings.max_jobs_per_day} reached",
        )

    row = (
        sb.table("strategy_jobs")
        .insert({"user_id": user_id, "idempotency_key": body.idempotency_key})
        .execute()
    )
    if not row.data:
        raise HTTPException(status_code=409, detail="Job with this idempotency key already exists")

    job = row.data[0]
    return {"job_id": job["id"], "status": job["status"]}


@router.post("/{job_id}/pdfs", status_code=status.HTTP_201_CREATED)
async def register_pdf(
    job_id: str,
    request: Request,
    body: RegisterPdfBody,
    user_id: str = Depends(_user_id),
) -> dict[str, Any]:
    sb = _sb(request)
    settings = _settings(request)

    # Validate job ownership + state
    job_resp = (
        sb.table("strategy_jobs")
        .select("id,status,pdf_count")
        .eq("id", job_id)
        .eq("user_id", user_id)
        .single()
        .execute()
    )
    if not job_resp.data:
        raise HTTPException(status_code=404, detail="Job not found")
    job = job_resp.data
    if job["status"] != "queued":
        raise HTTPException(status_code=409, detail="Job is no longer in queued state")
    if job["pdf_count"] >= settings.max_pdfs_per_job:
        raise HTTPException(
            status_code=422,
            detail=f"Job already has {settings.max_pdfs_per_job} PDFs (maximum)",
        )
    if body.file_size_bytes > settings.max_pdf_bytes:
        raise HTTPException(
            status_code=422,
            detail=f"File exceeds {settings.max_pdf_bytes} byte limit",
        )

    upload_id = str(uuid.uuid4())
    storage_path = f"{user_id}/{job_id}/{upload_id}.pdf"

    # Check SHA-256 dedupe: if this user already uploaded this exact file, reuse path.
    dedup_resp = (
        sb.table("pdf_uploads")
        .select("id,storage_path")
        .eq("user_id", user_id)
        .eq("sha256", body.sha256)
        .eq("status", "done")
        .limit(1)
        .execute()
    )
    if dedup_resp.data:
        existing = dedup_resp.data[0]
        # Still create a new pdf_uploads row for this job (different job_id).
        storage_path = existing["storage_path"]

    # Generate signed upload URL (PUT to Supabase Storage)
    signed = sb.storage.from_("pdfs").create_signed_upload_url(storage_path)
    upload_url: str = signed.get("signedUrl") or signed.get("signed_url", "")

    # Persist upload row
    sb.table("pdf_uploads").insert(
        {
            "id": upload_id,
            "user_id": user_id,
            "job_id": job_id,
            "filename": body.filename,
            "sha256": body.sha256,
            "file_size_bytes": body.file_size_bytes,
            "storage_path": storage_path,
        }
    ).execute()

    # Increment job pdf_count
    sb.table("strategy_jobs").update({"pdf_count": job["pdf_count"] + 1}).eq(
        "id", job_id
    ).execute()

    return {
        "upload_id": upload_id,
        "upload_url": upload_url,
        "storage_path": storage_path,
    }


@router.post("/{job_id}/pdfs/{upload_id}/confirm")
async def confirm_upload(
    job_id: str,
    upload_id: str,
    request: Request,
    user_id: str = Depends(_user_id),
) -> dict[str, Any]:
    sb = _sb(request)

    row = (
        sb.table("pdf_uploads")
        .select("id,status")
        .eq("id", upload_id)
        .eq("job_id", job_id)
        .eq("user_id", user_id)
        .single()
        .execute()
    )
    if not row.data:
        raise HTTPException(status_code=404, detail="Upload not found")
    if row.data["status"] not in ("pending", "uploaded"):
        raise HTTPException(status_code=409, detail="Upload already confirmed or in error state")

    sb.table("pdf_uploads").update({"status": "uploaded"}).eq("id", upload_id).execute()
    return {"upload_id": upload_id, "status": "uploaded"}


@router.post("/{job_id}/submit")
async def submit_job(
    job_id: str,
    request: Request,
    user_id: str = Depends(_user_id),
) -> dict[str, Any]:
    import datetime as _dt

    from arq.connections import ArqRedis

    sb = _sb(request)

    job_resp = (
        sb.table("strategy_jobs")
        .select("id,status,pdf_count")
        .eq("id", job_id)
        .eq("user_id", user_id)
        .single()
        .execute()
    )
    if not job_resp.data:
        raise HTTPException(status_code=404, detail="Job not found")
    job = job_resp.data
    if job["status"] != "queued":
        raise HTTPException(status_code=409, detail="Job is not in queued state")
    if job["pdf_count"] == 0:
        raise HTTPException(status_code=422, detail="Job has no PDFs; add at least one PDF first")

    # Check all registered PDFs are confirmed (uploaded)
    not_ready = (
        sb.table("pdf_uploads")
        .select("id", count="exact")
        .eq("job_id", job_id)
        .neq("status", "uploaded")
        .execute()
    )
    if (not_ready.count or 0) > 0:
        raise HTTPException(
            status_code=422,
            detail="Not all PDFs are confirmed as uploaded yet",
        )

    arq_redis: ArqRedis = request.app.state.arq_redis
    arq_job = await arq_redis.enqueue_job("ingest_job", job_id, user_id)
    arq_job_id = arq_job.job_id if arq_job else None

    now = _dt.datetime.now(_dt.UTC).isoformat()
    sb.table("strategy_jobs").update(
        {
            "status": "processing",
            "submitted_at": now,
            "arq_job_id": arq_job_id,
        }
    ).eq("id", job_id).execute()

    return {"job_id": job_id, "status": "processing", "arq_job_id": arq_job_id}


@router.get("/{job_id}")
async def get_job(
    job_id: str,
    request: Request,
    user_id: str = Depends(_user_id),
) -> dict[str, Any]:
    sb = _sb(request)

    job_resp = (
        sb.table("strategy_jobs")
        .select("*")
        .eq("id", job_id)
        .eq("user_id", user_id)
        .single()
        .execute()
    )
    if not job_resp.data:
        raise HTTPException(status_code=404, detail="Job not found")

    pdfs_resp = (
        sb.table("pdf_uploads")
        .select("id,filename,sha256,file_size_bytes,page_count,ocr_confidence,status,error")
        .eq("job_id", job_id)
        .execute()
    )

    return {"job": job_resp.data, "pdfs": pdfs_resp.data or []}


@router.delete("/{job_id}/pdfs/{upload_id}", status_code=status.HTTP_200_OK)
async def delete_upload(
    job_id: str,
    upload_id: str,
    request: Request,
    user_id: str = Depends(_user_id),
) -> dict[str, Any]:
    sb = _sb(request)

    job_resp = (
        sb.table("strategy_jobs")
        .select("id,status,pdf_count")
        .eq("id", job_id)
        .eq("user_id", user_id)
        .single()
        .execute()
    )
    if not job_resp.data:
        raise HTTPException(status_code=404, detail="Job not found")
    if job_resp.data["status"] != "queued":
        raise HTTPException(status_code=409, detail="Cannot remove PDFs from a non-queued job")

    upload_resp = (
        sb.table("pdf_uploads")
        .select("id,storage_path")
        .eq("id", upload_id)
        .eq("job_id", job_id)
        .eq("user_id", user_id)
        .single()
        .execute()
    )
    if not upload_resp.data:
        raise HTTPException(status_code=404, detail="Upload not found")

    storage_path = upload_resp.data["storage_path"]
    try:
        sb.storage.from_("pdfs").remove([storage_path])
    except Exception as exc:
        _LOG.warning("Storage remove failed for %s: %s", storage_path, exc)

    sb.table("pdf_uploads").delete().eq("id", upload_id).execute()
    new_count = max(0, job_resp.data["pdf_count"] - 1)
    sb.table("strategy_jobs").update({"pdf_count": new_count}).eq("id", job_id).execute()

    return {"ok": True}
