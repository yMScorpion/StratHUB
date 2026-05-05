"""Unit tests for the ingest pipeline (no real external services)."""

from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from api_ai.ingest.chunker import chunk_pages
from api_ai.ingest.models import Chunk, PageContent


FIXTURE = (
    Path(__file__).resolve().parents[3]
    / "packages"
    / "strategy-spec"
    / "fixtures"
    / "wyckoff_spring_btc_15m.json"
)


# ---------------------------------------------------------------------------
# chunker (pure Python — no mocking needed)
# ---------------------------------------------------------------------------


def _pages(texts: list[str]) -> list[PageContent]:
    return [PageContent(page_number=i + 1, text=t) for i, t in enumerate(texts)]


def test_chunk_pages_basic():
    pages = _pages(["word " * 600])  # 600 words → 2 chunks at size=512, overlap=64
    chunks = chunk_pages(pages, upload_id="uid-1")
    assert len(chunks) >= 2
    assert all(isinstance(c, Chunk) for c in chunks)
    assert chunks[0].upload_id == "uid-1"
    assert chunks[0].source_page == 1


def test_chunk_pages_skips_chart_heavy():
    pages = [
        PageContent(page_number=1, text="hello world", is_chart_heavy=True),
        PageContent(page_number=2, text="word " * 100),
    ]
    chunks = chunk_pages(pages, upload_id="uid-2")
    # Only page 2 contributes
    assert all(c.source_page == 2 for c in chunks)


def test_chunk_pages_empty_text():
    chunks = chunk_pages(_pages([""]), upload_id="uid-3")
    assert chunks == []


def test_chunk_pages_preserves_order():
    pages = _pages(["alpha " * 600, "beta " * 600])
    chunks = chunk_pages(pages, upload_id="uid-4")
    indices = [c.chunk_index for c in chunks]
    assert indices == sorted(indices)


def test_chunk_pages_overlap_produces_correct_window():
    words = list(range(600))
    text = " ".join(str(w) for w in words)
    pages = _pages([text])
    chunks = chunk_pages(pages, upload_id="uid-5", chunk_size=100, overlap=10)
    # Second window should start at word 90 (100 - 10)
    first_end = chunks[0].text.split()[-1]
    second_start = chunks[1].text.split()[0]
    assert int(second_start) == 90


# ---------------------------------------------------------------------------
# pdf extractor (mocked fitz)
# ---------------------------------------------------------------------------


def _make_fitz_page(text: str, char_count: int | None = None):
    page = MagicMock()
    page.get_text.return_value = text
    page.get_pixmap.return_value = MagicMock(tobytes=MagicMock(return_value=b""))
    return page


def test_extract_pdf_basic():
    fake_page = _make_fitz_page("This is a test page with plenty of text content here.")
    fake_doc = MagicMock()
    fake_doc.__len__ = MagicMock(return_value=1)
    fake_doc.__getitem__ = MagicMock(return_value=fake_page)
    fake_doc.close = MagicMock()

    with patch("api_ai.ingest.pdf._HAS_PYMUPDF", True), patch(
        "api_ai.ingest.pdf.fitz"
    ) as mock_fitz:
        mock_fitz.open.return_value = fake_doc
        from api_ai.ingest.pdf import extract_pdf

        result = extract_pdf(b"%PDF-1.4 fake", upload_id="uid-1", filename="test.pdf")

    assert result.page_count == 1
    assert result.upload_id == "uid-1"
    assert result.filename == "test.pdf"
    assert result.pages[0].page_number == 1


def test_extract_pdf_respects_max_pages():
    fake_pages = [_make_fitz_page(f"Page {i} content " * 20) for i in range(10)]
    fake_doc = MagicMock()
    fake_doc.__len__ = MagicMock(return_value=10)
    fake_doc.__getitem__ = MagicMock(side_effect=lambda i: fake_pages[i])
    fake_doc.close = MagicMock()

    with patch("api_ai.ingest.pdf._HAS_PYMUPDF", True), patch(
        "api_ai.ingest.pdf.fitz"
    ) as mock_fitz:
        mock_fitz.open.return_value = fake_doc
        from api_ai.ingest.pdf import extract_pdf

        result = extract_pdf(b"fake", upload_id="uid", filename="f.pdf", max_pages=3)

    assert result.page_count == 3


def test_extract_pdf_raises_without_pymupdf():
    with patch("api_ai.ingest.pdf._HAS_PYMUPDF", False):
        from api_ai.ingest.pdf import extract_pdf

        with pytest.raises(RuntimeError, match="PyMuPDF"):
            extract_pdf(b"fake", upload_id="uid", filename="f.pdf")


# ---------------------------------------------------------------------------
# embedder (mocked OpenAI)
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_embed_chunks_returns_embedded():
    from api_ai.ingest.embedder import embed_chunks
    from api_ai.ingest.models import Chunk

    chunks = [Chunk(upload_id="u1", chunk_index=0, text="hello world", source_page=1)]

    fake_emb = MagicMock()
    fake_emb.embedding = [0.1] * 1536

    fake_resp = MagicMock()
    fake_resp.data = [fake_emb]

    mock_create = AsyncMock(return_value=fake_resp)

    # Patch at the source since AsyncOpenAI is lazily imported inside the function.
    with patch("openai.AsyncOpenAI") as MockOpenAI:
        MockOpenAI.return_value.embeddings.create = mock_create
        settings = MagicMock()
        settings.openai_api_key = "test-key"
        settings.embedding_model = "text-embedding-3-small"

        result = await embed_chunks(chunks, settings)

    assert len(result) == 1
    assert result[0].embedding_dim == 1536
    assert result[0].embedding_model == "text-embedding-3-small"
    assert result[0].source_page == 1


@pytest.mark.asyncio
async def test_embed_chunks_empty_returns_empty():
    from api_ai.ingest.embedder import embed_chunks

    result = await embed_chunks([], MagicMock())
    assert result == []


# ---------------------------------------------------------------------------
# orchestrator (mocked httpx)
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_generate_spec_success():
    from api_ai.ingest.models import Chunk
    from api_ai.ingest.orchestrator import generate_spec

    golden = json.loads(FIXTURE.read_text("utf-8"))
    # Remove spec_hash so the orchestrator can add it
    golden.pop("spec_hash", None)

    chunks = [Chunk(upload_id="abc", chunk_index=0, text="VSA spring buy setup", source_page=1)]

    mock_response = MagicMock()
    mock_response.raise_for_status = MagicMock()
    mock_response.json.return_value = {
        "choices": [{"message": {"content": json.dumps(golden)}}]
    }

    mock_client = AsyncMock()
    mock_client.__aenter__ = AsyncMock(return_value=mock_client)
    mock_client.__aexit__ = AsyncMock(return_value=False)
    mock_client.post = AsyncMock(return_value=mock_response)

    settings = MagicMock()
    settings.openrouter_api_key = "test"
    settings.openrouter_model_primary = "deepseek/deepseek-v4-pro"
    settings.openrouter_model_bulk = "deepseek/deepseek-v4-flash"
    settings.openrouter_model_fallback = "anthropic/claude-3.5-sonnet"

    pdf_ids = [u["pdf_id"] for u in golden["citations"]][:1]

    with patch("api_ai.ingest.orchestrator.httpx.AsyncClient", return_value=mock_client):
        result = await generate_spec(chunks, pdf_ids, ["test.pdf"], settings)

    assert "spec_hash" in result
    assert result["citations"]


@pytest.mark.asyncio
async def test_generate_spec_all_attempts_fail_raises():
    from api_ai.ingest.models import Chunk
    from api_ai.ingest.orchestrator import generate_spec

    chunks = [Chunk(upload_id="abc", chunk_index=0, text="text", source_page=1)]

    mock_client = AsyncMock()
    mock_client.__aenter__ = AsyncMock(return_value=mock_client)
    mock_client.__aexit__ = AsyncMock(return_value=False)
    mock_client.post = AsyncMock(
        return_value=MagicMock(
            raise_for_status=MagicMock(),
            json=MagicMock(return_value={"choices": [{"message": {"content": "not-json"}}]}),
        )
    )

    settings = MagicMock()
    settings.openrouter_api_key = "test"
    settings.openrouter_model_primary = "m1"
    settings.openrouter_model_bulk = "m2"
    settings.openrouter_model_fallback = "m3"

    with patch("api_ai.ingest.orchestrator.httpx.AsyncClient", return_value=mock_client):
        with pytest.raises(RuntimeError, match="attempts failed"):
            await generate_spec(chunks, ["some-id"], ["f.pdf"], settings)
