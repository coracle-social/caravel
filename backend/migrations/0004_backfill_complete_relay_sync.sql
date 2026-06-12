-- A manual migration created relays without recording the `complete_relay_sync`
-- activity that `complete_relay_sync` normally emits. Backfill one per synced relay
-- that lacks it, mirroring `insert_activity_tx`: a fresh v4 UUID id, the relay's tenant,
-- a NULL billed_at, and a snapshot of the relay's current plan and status. This
-- activity type is not billable (see `list_billable_activity`), so the NULL
-- billed_at bills no one.
INSERT INTO activity (id, tenant_pubkey, created_at, activity_type, resource_type, resource_id, snapshot)
SELECT
  lower(
    hex(randomblob(4)) || '-' ||
    hex(randomblob(2)) || '-4' ||
    substr(hex(randomblob(2)), 2) || '-' ||
    substr('89ab', abs(random() % 4) + 1, 1) ||
    substr(hex(randomblob(2)), 2) || '-' ||
    hex(randomblob(6))
  ),
  relay.tenant_pubkey,
  CAST(strftime('%s', 'now') AS INTEGER),
  'complete_relay_sync',
  'relay',
  relay.id,
  json_object('resource_type', 'relay', 'plan', relay.plan_id, 'status', relay.status)
FROM relay
WHERE relay.synced = 1
  AND NOT EXISTS (
    SELECT 1 FROM activity
    WHERE activity.resource_id = relay.id
      AND activity.activity_type = 'complete_relay_sync'
  );
