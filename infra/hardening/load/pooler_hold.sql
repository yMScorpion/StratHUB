-- Phase 7 pooler hold probe.
-- Run against the Supabase pooler, not the direct database host.
-- Expected result under validator load: no long-lived idle transactions from validator machines.

select
  application_name,
  state,
  count(*) as connection_count,
  max(now() - state_change) as oldest_state_age
from pg_stat_activity
where application_name ilike '%validator%'
   or application_name ilike '%exec-rs%'
   or application_name ilike '%api-ai%'
group by application_name, state
order by oldest_state_age desc nulls last;
