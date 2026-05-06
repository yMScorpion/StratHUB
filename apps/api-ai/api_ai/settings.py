"""Application settings loaded from environment / .env."""

from __future__ import annotations

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", env_file_encoding="utf-8", extra="ignore")

    redis_url: str = "redis://localhost:6379/0"

    supabase_url: str = "http://localhost:54321"
    supabase_service_role_key: str = "replace-me"

    openrouter_api_key: str = "replace-me"
    openrouter_model_primary: str = "deepseek/deepseek-v4-pro"
    openrouter_model_bulk: str = "deepseek/deepseek-v4-flash"
    openrouter_model_fallback: str = "anthropic/claude-3.5-sonnet"

    # Embeddings — OpenAI by default; set EMBEDDING_MODEL + OPENAI_API_KEY.
    openai_api_key: str = "replace-me"
    embedding_model: str = "text-embedding-3-small"
    embedding_dim: int = Field(default=1536, ge=1)

    # Whether users supply their own OpenRouter/OpenAI keys (BYOK) or platform pays.
    byok_mode: bool = False

    # Per-file and per-job limits
    max_pdfs_per_job: int = Field(default=50, ge=1, le=100)
    max_pdf_bytes: int = Field(default=25 * 1024 * 1024, ge=1024)
    max_pages_per_pdf: int = Field(default=500, ge=1, le=2000)

    # Daily / monthly rate limits
    max_jobs_per_day: int = Field(default=10, ge=1, le=100)
    max_ocr_pages_per_day: int = Field(default=2000, ge=0)
    max_llm_tokens_per_month: int = Field(default=2_000_000, ge=0)

    # Concurrent validator cap (enforced in Phase 5)
    max_concurrent_validators: int = Field(default=5, ge=0, le=50)

    # Shared secret the Next.js server must send in X-Internal-Token.
    # Must be overridden in production via INTERNAL_API_KEY env var.
    internal_api_key: str = "replace-me"


def load_settings() -> Settings:
    return Settings()
