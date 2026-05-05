-- Phase 2: ingestion pipeline tables + Storage bucket policy.
-- Tables: strategy_jobs, pdf_uploads, strategy_embeddings, strategies.
-- Path convention for Storage: pdfs/{user_id}/{job_id}/{upload_id}.pdf

SET search_path = public;

-- strategy_jobs: one row per PDF-ingestion batch (1-50 PDFs → 1 strategy)
CREATE TABLE IF NOT EXISTS strategy_jobs (
  id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id         uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  idempotency_key text        NOT NULL,
  status          text        NOT NULL DEFAULT 'queued'
    CHECK (status IN ('queued','processing','needs_review','approved','rejected','failed')),
  pdf_count       integer     NOT NULL DEFAULT 0 CHECK (pdf_count BETWEEN 0 AND 50),
  arq_job_id      text,
  submitted_at    timestamptz,
  completed_at    timestamptz,
  error           text,
  created_at      timestamptz NOT NULL DEFAULT now(),
  updated_at      timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT strategy_jobs_idempotency_unique UNIQUE (user_id, idempotency_key)
);

CREATE TRIGGER strategy_jobs_set_updated_at
  BEFORE UPDATE ON strategy_jobs
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

ALTER TABLE strategy_jobs ENABLE ROW LEVEL SECURITY;

CREATE POLICY "strategy_jobs readable by owner"
  ON strategy_jobs FOR SELECT TO authenticated
  USING (user_id = auth.uid());

CREATE POLICY "strategy_jobs insertable by owner"
  ON strategy_jobs FOR INSERT TO authenticated
  WITH CHECK (user_id = auth.uid());

CREATE POLICY "strategy_jobs updatable by owner"
  ON strategy_jobs FOR UPDATE TO authenticated
  USING (user_id = auth.uid())
  WITH CHECK (user_id = auth.uid());

-- pdf_uploads: one row per PDF registered in a job
CREATE TABLE IF NOT EXISTS pdf_uploads (
  id               uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id          uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  job_id           uuid        NOT NULL REFERENCES strategy_jobs(id) ON DELETE CASCADE,
  filename         text        NOT NULL,
  sha256           text        NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
  file_size_bytes  bigint      NOT NULL CHECK (file_size_bytes > 0),
  storage_path     text        NOT NULL,
  page_count       integer,
  ocr_confidence   numeric(5,2),
  status           text        NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending','uploaded','processing','done','error')),
  error            text,
  created_at       timestamptz NOT NULL DEFAULT now(),
  updated_at       timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX pdf_uploads_job_id_idx ON pdf_uploads (job_id);
CREATE INDEX pdf_uploads_user_sha256_idx ON pdf_uploads (user_id, sha256);

CREATE TRIGGER pdf_uploads_set_updated_at
  BEFORE UPDATE ON pdf_uploads
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

ALTER TABLE pdf_uploads ENABLE ROW LEVEL SECURITY;

CREATE POLICY "pdf_uploads readable by owner"
  ON pdf_uploads FOR SELECT TO authenticated
  USING (user_id = auth.uid());

CREATE POLICY "pdf_uploads insertable by owner"
  ON pdf_uploads FOR INSERT TO authenticated
  WITH CHECK (user_id = auth.uid());

CREATE POLICY "pdf_uploads updatable by owner"
  ON pdf_uploads FOR UPDATE TO authenticated
  USING (user_id = auth.uid())
  WITH CHECK (user_id = auth.uid());

CREATE POLICY "pdf_uploads deletable by owner"
  ON pdf_uploads FOR DELETE TO authenticated
  USING (user_id = auth.uid());

-- strategy_embeddings: text chunks + float embeddings (migrate to vector() in Phase 3)
CREATE TABLE IF NOT EXISTS strategy_embeddings (
  id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id         uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  job_id          uuid        NOT NULL REFERENCES strategy_jobs(id) ON DELETE CASCADE,
  upload_id       uuid        NOT NULL REFERENCES pdf_uploads(id) ON DELETE CASCADE,
  chunk_index     integer     NOT NULL,
  chunk_text      text        NOT NULL,
  embedding       float8[],
  embedding_model text        NOT NULL,
  embedding_dim   integer     NOT NULL,
  source_page     integer,
  created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX strategy_embeddings_job_id_idx ON strategy_embeddings (job_id);

ALTER TABLE strategy_embeddings ENABLE ROW LEVEL SECURITY;

CREATE POLICY "strategy_embeddings readable by owner"
  ON strategy_embeddings FOR SELECT TO authenticated
  USING (user_id = auth.uid());

-- strategies: compiled strategy spec awaiting human review
CREATE TABLE IF NOT EXISTS strategies (
  id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id     uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  job_id      uuid        REFERENCES strategy_jobs(id) ON DELETE SET NULL,
  spec_jsonb  jsonb       NOT NULL,
  spec_hash   text        NOT NULL CHECK (spec_hash ~ '^[0-9a-f]{64}$'),
  status      text        NOT NULL DEFAULT 'needs_review'
    CHECK (status IN ('needs_review','approved','rejected')),
  reviewed_by uuid        REFERENCES auth.users(id),
  reviewed_at timestamptz,
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX strategies_user_id_idx ON strategies (user_id);
CREATE UNIQUE INDEX strategies_spec_hash_unique ON strategies (user_id, spec_hash);

CREATE TRIGGER strategies_set_updated_at
  BEFORE UPDATE ON strategies
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

ALTER TABLE strategies ENABLE ROW LEVEL SECURITY;

CREATE POLICY "strategies readable by owner"
  ON strategies FOR SELECT TO authenticated
  USING (user_id = auth.uid());

CREATE POLICY "strategies insertable by owner"
  ON strategies FOR INSERT TO authenticated
  WITH CHECK (user_id = auth.uid());

CREATE POLICY "strategies updatable by owner"
  ON strategies FOR UPDATE TO authenticated
  USING (user_id = auth.uid())
  WITH CHECK (user_id = auth.uid());

-- Storage: pdfs bucket (25 MiB per file, PDF only, private)
INSERT INTO storage.buckets (id, name, public, file_size_limit, allowed_mime_types)
VALUES ('pdfs', 'pdfs', false, 26214400, ARRAY['application/pdf'])
ON CONFLICT (id) DO NOTHING;

CREATE POLICY "pdfs insertable by owner"
  ON storage.objects FOR INSERT TO authenticated
  WITH CHECK (
    bucket_id = 'pdfs'
    AND (storage.foldername(name))[1] = auth.uid()::text
  );

CREATE POLICY "pdfs readable by owner"
  ON storage.objects FOR SELECT TO authenticated
  USING (
    bucket_id = 'pdfs'
    AND (storage.foldername(name))[1] = auth.uid()::text
  );

CREATE POLICY "pdfs deletable by owner"
  ON storage.objects FOR DELETE TO authenticated
  USING (
    bucket_id = 'pdfs'
    AND (storage.foldername(name))[1] = auth.uid()::text
  );
