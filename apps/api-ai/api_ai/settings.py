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

    max_pdfs_per_job: int = Field(default=50, ge=1, le=100)
    max_pdf_bytes: int = Field(default=25 * 1024 * 1024, ge=1024)
    max_ocr_pages_per_day: int = Field(default=2000, ge=0)
    max_llm_tokens_per_month: int = Field(default=2_000_000, ge=0)


def load_settings() -> Settings:
    return Settings()
