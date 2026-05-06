"""Integration tests for job management routes (Supabase + Arq mocked)."""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock

import pytest
from fastapi.testclient import TestClient

from api_ai.main import create_app


def _make_app(sb_mock: MagicMock | None = None, arq_mock: MagicMock | None = None):
    """Build an app with Supabase and Arq state injected directly (no lifespan)."""
    app = create_app()
    # Inject mocks into app state directly; TestClient without context manager
    # does not trigger the lifespan, so these won't be overwritten.
    app.state.supabase = sb_mock or MagicMock()
    app.state.arq_redis = arq_mock or AsyncMock()
    return app


def _client(sb_mock: MagicMock | None = None, arq_mock: MagicMock | None = None) -> TestClient:
    return TestClient(_make_app(sb_mock, arq_mock))


HEADERS = {
    "X-User-Id": "00000000-0000-0000-0000-000000000001",
    "X-Internal-Token": "replace-me",
}


# ---------------------------------------------------------------------------
# POST /jobs
# ---------------------------------------------------------------------------


def test_create_job_success():
    sb = MagicMock()
    # No jobs today — one .eq() then .gte() then .execute()
    sb.table.return_value.select.return_value.eq.return_value.gte.return_value.execute.return_value = MagicMock(count=0)
    # Insert succeeds
    sb.table.return_value.insert.return_value.execute.return_value = MagicMock(
        data=[{"id": "job-123", "status": "queued"}]
    )

    r = _client(sb).post("/jobs", json={"idempotency_key": "test-key-1"}, headers=HEADERS)
    assert r.status_code == 201
    assert r.json()["job_id"] == "job-123"


def test_create_job_missing_user_id_header():
    r = _client().post("/jobs", json={"idempotency_key": "k"})
    assert r.status_code == 422


def test_create_job_invalid_user_id():
    r = _client().post("/jobs", json={"idempotency_key": "k"}, headers={"X-User-Id": "not-a-uuid"})
    assert r.status_code == 400


def test_create_job_daily_limit_exceeded():
    sb = MagicMock()
    sb.table.return_value.select.return_value.eq.return_value.gte.return_value.execute.return_value = MagicMock(count=10)

    app = _make_app(sb)
    app.state.settings.max_jobs_per_day = 10

    r = TestClient(app).post("/jobs", json={"idempotency_key": "k"}, headers=HEADERS)
    assert r.status_code == 429


# ---------------------------------------------------------------------------
# POST /jobs/{id}/pdfs
# ---------------------------------------------------------------------------


def _sb_for_register(
    job_status: str = "queued",
    pdf_count: int = 0,
    signed_url: str = "https://example.com/upload",
    dedup_data: list | None = None,
) -> MagicMock:
    sb = MagicMock()
    # get job
    sb.table.return_value.select.return_value.eq.return_value.eq.return_value.single.return_value.execute.return_value = MagicMock(
        data={"id": "job-1", "status": job_status, "pdf_count": pdf_count}
    )
    # dedup check
    sb.table.return_value.select.return_value.eq.return_value.eq.return_value.eq.return_value.limit.return_value.execute.return_value = MagicMock(
        data=dedup_data or []
    )
    # signed URL
    sb.storage.from_.return_value.create_signed_upload_url.return_value = {
        "signedUrl": signed_url
    }
    # insert upload
    sb.table.return_value.insert.return_value.execute.return_value = MagicMock(data=[{"id": "upload-1"}])
    # update pdf_count
    sb.table.return_value.update.return_value.eq.return_value.execute.return_value = MagicMock(data=[])
    return sb


def test_register_pdf_success():
    sb = _sb_for_register()
    r = TestClient(_make_app(sb)).post(
        "/jobs/job-1/pdfs",
        json={
            "filename": "apostila.pdf",
            "sha256": "a" * 64,
            "file_size_bytes": 1024,
        },
        headers=HEADERS,
    )
    assert r.status_code == 201
    body = r.json()
    assert "upload_id" in body
    assert "upload_url" in body


def test_register_pdf_job_not_queued():
    sb = _sb_for_register(job_status="processing")
    r = TestClient(_make_app(sb)).post(
        "/jobs/job-1/pdfs",
        json={"filename": "f.pdf", "sha256": "b" * 64, "file_size_bytes": 100},
        headers=HEADERS,
    )
    assert r.status_code == 409


def test_register_pdf_too_many_pdfs():
    sb = _sb_for_register(pdf_count=50)
    app = _make_app(sb)
    app.state.settings.max_pdfs_per_job = 50
    r = TestClient(app).post(
        "/jobs/job-1/pdfs",
        json={"filename": "f.pdf", "sha256": "c" * 64, "file_size_bytes": 100},
        headers=HEADERS,
    )
    assert r.status_code == 422


def test_register_pdf_file_too_large():
    sb = _sb_for_register()
    app = _make_app(sb)
    app.state.settings.max_pdf_bytes = 1000
    r = TestClient(app).post(
        "/jobs/job-1/pdfs",
        json={"filename": "big.pdf", "sha256": "d" * 64, "file_size_bytes": 50_000_000},
        headers=HEADERS,
    )
    assert r.status_code == 422


# ---------------------------------------------------------------------------
# GET /jobs/{id}
# ---------------------------------------------------------------------------


def test_get_job_not_found():
    sb = MagicMock()
    sb.table.return_value.select.return_value.eq.return_value.eq.return_value.single.return_value.execute.return_value = MagicMock(data=None)
    r = TestClient(_make_app(sb)).get("/jobs/no-such-job", headers=HEADERS)
    assert r.status_code == 404


def test_get_job_success():
    sb = MagicMock()
    sb.table.return_value.select.return_value.eq.return_value.eq.return_value.single.return_value.execute.return_value = MagicMock(
        data={"id": "job-1", "status": "needs_review", "pdf_count": 1}
    )
    sb.table.return_value.select.return_value.eq.return_value.execute.return_value = MagicMock(
        data=[{"id": "upload-1", "filename": "f.pdf", "status": "done"}]
    )
    r = TestClient(_make_app(sb)).get("/jobs/job-1", headers=HEADERS)
    assert r.status_code == 200
    body = r.json()
    assert body["job"]["status"] == "needs_review"
    assert len(body["pdfs"]) == 1


# ---------------------------------------------------------------------------
# POST /jobs/{id}/submit
# ---------------------------------------------------------------------------


def test_submit_job_success():
    sb = MagicMock()
    # job is queued with 1 PDF
    sb.table.return_value.select.return_value.eq.return_value.eq.return_value.single.return_value.execute.return_value = MagicMock(
        data={"id": "job-1", "status": "queued", "pdf_count": 1}
    )
    # all PDFs uploaded
    sb.table.return_value.select.return_value.eq.return_value.neq.return_value.execute.return_value = MagicMock(count=0)
    sb.table.return_value.update.return_value.eq.return_value.execute.return_value = MagicMock(data=[])

    arq_mock = AsyncMock()
    mock_job = MagicMock()
    mock_job.job_id = "arq-123"
    arq_mock.enqueue_job = AsyncMock(return_value=mock_job)
    arq_mock.close = AsyncMock()

    app = _make_app(sb, arq_mock)
    # Inject arq_redis into app state directly
    app.state.arq_redis = arq_mock

    r = TestClient(app).post("/jobs/job-1/submit", headers=HEADERS)
    assert r.status_code == 200
    body = r.json()
    assert body["status"] == "processing"


def test_submit_job_no_pdfs():
    sb = MagicMock()
    sb.table.return_value.select.return_value.eq.return_value.eq.return_value.single.return_value.execute.return_value = MagicMock(
        data={"id": "job-1", "status": "queued", "pdf_count": 0}
    )
    r = TestClient(_make_app(sb)).post("/jobs/job-1/submit", headers=HEADERS)
    assert r.status_code == 422


# ---------------------------------------------------------------------------
# DELETE /jobs/{id}/pdfs/{upload_id}
# ---------------------------------------------------------------------------


def _sb_for_delete(ref_count: int = 0) -> MagicMock:
    sb = MagicMock()
    # job lookup
    sb.table.return_value.select.return_value.eq.return_value.eq.return_value.single.return_value.execute.return_value = MagicMock(
        data={"id": "job-1", "status": "queued", "pdf_count": 1}
    )
    # upload lookup
    sb.table.return_value.select.return_value.eq.return_value.eq.return_value.eq.return_value.single.return_value.execute.return_value = MagicMock(
        data={"id": "upload-1", "storage_path": "user/job-1/upload-1.pdf"}
    )
    # ref-count check: how many OTHER rows share this storage_path
    sb.table.return_value.select.return_value.eq.return_value.neq.return_value.execute.return_value = MagicMock(
        count=ref_count
    )
    sb.table.return_value.delete.return_value.eq.return_value.execute.return_value = MagicMock(data=[])
    sb.table.return_value.update.return_value.eq.return_value.execute.return_value = MagicMock(data=[])
    return sb


def test_delete_upload_removes_storage_when_no_other_refs():
    sb = _sb_for_delete(ref_count=0)
    r = TestClient(_make_app(sb)).delete("/jobs/job-1/pdfs/upload-1", headers=HEADERS)
    assert r.status_code == 200
    sb.storage.from_.return_value.remove.assert_called_once()


def test_delete_upload_skips_storage_when_shared():
    sb = _sb_for_delete(ref_count=1)
    r = TestClient(_make_app(sb)).delete("/jobs/job-1/pdfs/upload-1", headers=HEADERS)
    assert r.status_code == 200
    sb.storage.from_.return_value.remove.assert_not_called()


def test_jobs_routes_reject_missing_internal_token():
    r = TestClient(_make_app()).post(
        "/jobs", json={"idempotency_key": "k"}, headers={"X-User-Id": "00000000-0000-0000-0000-000000000001"}
    )
    assert r.status_code == 422  # missing required header


def test_jobs_routes_reject_wrong_internal_token():
    r = TestClient(_make_app()).post(
        "/jobs",
        json={"idempotency_key": "k"},
        headers={"X-User-Id": "00000000-0000-0000-0000-000000000001", "X-Internal-Token": "wrong"},
    )
    assert r.status_code == 401
