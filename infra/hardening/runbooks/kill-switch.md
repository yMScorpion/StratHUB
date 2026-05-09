# Kill-Switch Runbook

## Trigger

- User activates kill-switch.
- Risk guard breaches daily loss, position, or exchange error threshold.
- Operator declares unsafe execution.

## Procedure

1. Set `risk_limits.kill_switch_active = true` for the user or affected cohort.
2. Stop new live launches for the user immediately.
3. For each active live run, send cancel-all to the exchange adapter.
4. Flatten positions when the user setting or incident commander requires it.
5. Confirm no open `order_outbox` item remains in `pending` or `sent` state.
6. Record an audit log entry with reason, actor, run ids, exchange responses, and timing.

## Recovery

- User must explicitly clear the kill-switch after reviewing positions and risk limits.
- Do not resume any live run automatically.
