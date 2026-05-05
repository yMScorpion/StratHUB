"""Dataclasses for the ingest pipeline stages."""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class PageContent:
    page_number: int          # 1-indexed
    text: str
    ocr_confidence: float | None = None  # 0-100; None when PyMuPDF native extraction was used
    is_chart_heavy: bool = False


@dataclass
class ExtractedPdf:
    upload_id: str
    filename: str
    pages: list[PageContent] = field(default_factory=list)
    avg_ocr_confidence: float | None = None

    @property
    def page_count(self) -> int:
        return len(self.pages)


@dataclass
class Chunk:
    upload_id: str
    chunk_index: int
    text: str
    source_page: int | None


@dataclass
class EmbeddedChunk:
    upload_id: str
    chunk_index: int
    text: str
    source_page: int | None
    embedding: list[float]
    embedding_model: str
    embedding_dim: int
