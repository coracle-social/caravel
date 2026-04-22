CREATE INDEX IF NOT EXISTS idx_tenant_stripe_customer_id
ON tenant (stripe_customer_id);

CREATE INDEX IF NOT EXISTS idx_relay_tenant_id
ON relay (tenant, id);

CREATE INDEX IF NOT EXISTS idx_relay_tenant_status_plan
ON relay (tenant, status, plan);

CREATE INDEX IF NOT EXISTS idx_activity_resource_type_resource_id_created_at_id
ON activity (resource_type, resource_id, created_at DESC, id DESC);
