# syntax=docker/dockerfile:1.7
FROM python:3.12-slim AS base

ENV PYTHONUNBUFFERED=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    UV_LINK_MODE=copy

# Phase 2: Tesseract OCR + language packs (por+eng) for scanned-PDF fallback.
RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends \
      tesseract-ocr \
      tesseract-ocr-por \
      tesseract-ocr-eng \
      libgl1 \
    && rm -rf /var/lib/apt/lists/*

RUN pip install --no-cache-dir uv==0.5.13

WORKDIR /app

# Copy the strategy-spec source (api-ai installs it in editable mode).
COPY packages/strategy-spec /app/packages/strategy-spec

# Copy api-ai project files.
COPY apps/api-ai/pyproject.toml /app/apps/api-ai/pyproject.toml
COPY apps/api-ai/api_ai /app/apps/api-ai/api_ai

WORKDIR /app/apps/api-ai

RUN uv venv --python 3.12 /opt/venv \
 && uv pip install --python /opt/venv/bin/python -e . \
 && uv pip install --python /opt/venv/bin/python "uvicorn[standard]" arq

ENV PATH="/opt/venv/bin:${PATH}"

EXPOSE 8000

CMD ["uvicorn", "api_ai.main:app", "--host", "0.0.0.0", "--port", "8000"]
