-- Phase 3: human review gate + audit trail.
-- Review actions are auditable, and approved/rejected are terminal review states.

SET search_path = public;

CREATE TABLE IF NOT EXISTS audit_log (
  id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id     uuid        NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  actor_id    uuid        REFERENCES auth.users(id) ON DELETE SET NULL,
  action      text        NOT NULL,
  entity_type text        NOT NULL,
  entity_id   uuid        NOT NULL,
  metadata    jsonb       NOT NULL DEFAULT '{}'::jsonb,
  created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS audit_log_user_created_at_idx
  ON audit_log (user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS audit_log_entity_idx
  ON audit_log (entity_type, entity_id, created_at DESC);

ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY;

CREATE POLICY "audit_log readable by owner"
  ON audit_log FOR SELECT TO authenticated
  USING (user_id = auth.uid());

CREATE POLICY "audit_log insertable by owner actor"
  ON audit_log FOR INSERT TO authenticated
  WITH CHECK (user_id = auth.uid() AND actor_id = auth.uid());

DROP POLICY IF EXISTS "strategies updatable by owner" ON strategies;
CREATE POLICY "strategies updatable by owner"
  ON strategies FOR UPDATE TO authenticated
  USING (user_id = auth.uid() AND status = 'needs_review')
  WITH CHECK (user_id = auth.uid() AND status = 'needs_review');

CREATE OR REPLACE FUNCTION enforce_strategy_review_transition()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF new.status = old.status THEN
    RETURN new;
  END IF;

  IF current_setting('app.review_strategy_rpc', true) IS DISTINCT FROM 'on' THEN
    RAISE EXCEPTION 'strategy status can only be changed through review_strategy'
      USING ERRCODE = '42501';
  END IF;

  IF old.status <> 'needs_review' THEN
    RAISE EXCEPTION 'strategy status % is terminal and cannot transition to %', old.status, new.status
      USING ERRCODE = '23514';
  END IF;

  IF new.status NOT IN ('approved', 'rejected') THEN
    RAISE EXCEPTION 'strategy can only transition from needs_review to approved or rejected'
      USING ERRCODE = '23514';
  END IF;

  new.reviewed_by = auth.uid();
  new.reviewed_at = now();
  RETURN new;
END;
$$;

DROP TRIGGER IF EXISTS strategies_enforce_review_transition ON strategies;
CREATE TRIGGER strategies_enforce_review_transition
  BEFORE UPDATE OF status ON strategies
  FOR EACH ROW EXECUTE FUNCTION enforce_strategy_review_transition();

CREATE OR REPLACE FUNCTION enforce_strategy_spec_immutability()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF new.status IN ('approved', 'rejected')
    AND (
      new.spec_jsonb IS DISTINCT FROM old.spec_jsonb
      OR new.spec_hash IS DISTINCT FROM old.spec_hash
    )
  THEN
    RAISE EXCEPTION 'approved or rejected strategy specs are immutable'
      USING ERRCODE = '23514';
  END IF;

  RETURN new;
END;
$$;

DROP TRIGGER IF EXISTS strategies_enforce_spec_immutability ON strategies;
CREATE TRIGGER strategies_enforce_spec_immutability
  BEFORE UPDATE OF spec_jsonb, spec_hash ON strategies
  FOR EACH ROW EXECUTE FUNCTION enforce_strategy_spec_immutability();

CREATE OR REPLACE FUNCTION review_strategy(
  p_strategy_id uuid,
  p_action text,
  p_notes text DEFAULT NULL
)
RETURNS TABLE (id uuid, status text, reviewed_at timestamptz)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
  v_strategy strategies%ROWTYPE;
  v_audit_action text;
BEGIN
  IF auth.uid() IS NULL THEN
    RAISE EXCEPTION 'authentication required' USING ERRCODE = '28000';
  END IF;

  SELECT *
  INTO v_strategy
  FROM strategies
  WHERE strategies.id = p_strategy_id
    AND strategies.user_id = auth.uid()
  FOR UPDATE;

  IF NOT FOUND THEN
    RAISE EXCEPTION 'strategy not found' USING ERRCODE = 'P0002';
  END IF;

  IF p_action NOT IN ('approve', 'reject', 'request_changes') THEN
    RAISE EXCEPTION 'invalid review action: %', p_action USING ERRCODE = '22023';
  END IF;

  IF p_action = 'request_changes' THEN
    IF v_strategy.status <> 'needs_review' THEN
      RAISE EXCEPTION 'request_changes is only allowed while strategy needs_review'
        USING ERRCODE = '23514';
    END IF;
    v_audit_action := 'strategy.review.request_changes';
  ELSIF p_action = 'approve' THEN
    PERFORM set_config('app.review_strategy_rpc', 'on', true);
    UPDATE strategies
    SET status = 'approved'
    WHERE strategies.id = p_strategy_id
    RETURNING * INTO v_strategy;
    v_audit_action := 'strategy.review.approved';
  ELSE
    PERFORM set_config('app.review_strategy_rpc', 'on', true);
    UPDATE strategies
    SET status = 'rejected'
    WHERE strategies.id = p_strategy_id
    RETURNING * INTO v_strategy;
    v_audit_action := 'strategy.review.rejected';
  END IF;

  INSERT INTO audit_log (user_id, actor_id, action, entity_type, entity_id, metadata)
  VALUES (
    v_strategy.user_id,
    auth.uid(),
    v_audit_action,
    'strategy',
    v_strategy.id,
    jsonb_build_object(
      'notes', nullif(trim(coalesce(p_notes, '')), ''),
      'spec_hash', v_strategy.spec_hash,
      'resulting_status', v_strategy.status
    )
  );

  id := v_strategy.id;
  status := v_strategy.status;
  reviewed_at := v_strategy.reviewed_at;
  RETURN NEXT;
END;
$$;

GRANT EXECUTE ON FUNCTION review_strategy(uuid, text, text) TO authenticated;

CREATE OR REPLACE FUNCTION assert_strategy_approved(p_strategy_id uuid)
RETURNS void
LANGUAGE plpgsql
SECURITY INVOKER
AS $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM strategies
    WHERE id = p_strategy_id
      AND user_id = auth.uid()
      AND status = 'approved'
  ) THEN
    RAISE EXCEPTION 'strategy must be approved before execution'
      USING ERRCODE = '23514';
  END IF;
END;
$$;

GRANT EXECUTE ON FUNCTION assert_strategy_approved(uuid) TO authenticated;
