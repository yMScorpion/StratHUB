"""Sliding-window word-level text chunker."""

from __future__ import annotations

from .models import Chunk, PageContent


def chunk_pages(
    pages: list[PageContent],
    upload_id: str,
    chunk_size: int = 512,
    overlap: int = 64,
) -> list[Chunk]:
    """Split page text into overlapping word windows.

    chart-heavy pages (very low text density) are skipped — they're flagged
    separately for human review and don't contribute useful text to the LLM.
    """
    chunks: list[Chunk] = []
    chunk_index = 0

    for page in pages:
        if not page.text or page.is_chart_heavy:
            continue

        words = page.text.split()
        if not words:
            continue

        start = 0
        while start < len(words):
            end = start + chunk_size
            text = " ".join(words[start:end]).strip()
            if text:
                chunks.append(
                    Chunk(
                        upload_id=upload_id,
                        chunk_index=chunk_index,
                        text=text,
                        source_page=page.page_number,
                    )
                )
                chunk_index += 1
            if end >= len(words):
                break
            start += chunk_size - overlap

    return chunks
