"""OpenAI text embedding for strategy chunks."""

from __future__ import annotations

import asyncio
import logging
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from ..settings import Settings

from .models import Chunk, EmbeddedChunk

_LOG = logging.getLogger(__name__)
_BATCH_SIZE = 96  # well within OpenAI's 2048 input limit


async def embed_chunks(chunks: list[Chunk], settings: "Settings") -> list[EmbeddedChunk]:
    if not chunks:
        return []

    from openai import AsyncOpenAI

    client = AsyncOpenAI(api_key=settings.openai_api_key)
    results: list[EmbeddedChunk] = []

    for i in range(0, len(chunks), _BATCH_SIZE):
        batch = chunks[i : i + _BATCH_SIZE]
        resp = await client.embeddings.create(
            model=settings.embedding_model,
            input=[c.text for c in batch],
            encoding_format="float",
        )
        for chunk, emb_obj in zip(batch, resp.data):
            results.append(
                EmbeddedChunk(
                    upload_id=chunk.upload_id,
                    chunk_index=chunk.chunk_index,
                    text=chunk.text,
                    source_page=chunk.source_page,
                    embedding=emb_obj.embedding,
                    embedding_model=settings.embedding_model,
                    embedding_dim=len(emb_obj.embedding),
                )
            )
        if i + _BATCH_SIZE < len(chunks):
            await asyncio.sleep(0.05)

    return results
