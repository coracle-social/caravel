CREATE TABLE tenant (
  pubkey TEXT PRIMARY KEY,
  return_url TEXT NOT NULL DEFAULT '',
  nwc_url TEXT,
  nwc_error TEXT,
  stripe_error TEXT,
  created_at BIGINT NOT NULL,
  billing_anchor BIGINT,
  stripe_customer_id TEXT NOT NULL,
  stripe_payment_method_id TEXT,
  renewed_at BIGINT,
  overage_billed_at BIGINT,
  churned_at BIGINT,
  banned_at BIGINT,
  stripe_account_id TEXT,
  stripe_account_ready BOOLEAN NOT NULL DEFAULT false
);

CREATE TABLE relay (
  id TEXT PRIMARY KEY,
  tenant_pubkey TEXT NOT NULL REFERENCES tenant (pubkey),
  subdomain TEXT NOT NULL,
  domain TEXT NOT NULL,
  plan_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('active', 'inactive', 'delinquent')),
  pubkey TEXT NOT NULL,
  secret TEXT NOT NULL,
  info_name TEXT NOT NULL DEFAULT '',
  info_icon TEXT NOT NULL DEFAULT '',
  info_description TEXT NOT NULL DEFAULT '',
  policy_public_read BOOLEAN NOT NULL DEFAULT false,
  policy_anonymous_read BOOLEAN NOT NULL DEFAULT false,
  policy_public_write BOOLEAN NOT NULL DEFAULT false,
  policy_public_join BOOLEAN NOT NULL DEFAULT false,
  policy_strip_signatures BOOLEAN NOT NULL DEFAULT false,
  groups_enabled BOOLEAN NOT NULL DEFAULT true,
  management_enabled BOOLEAN NOT NULL DEFAULT true,
  members_can_invite BOOLEAN NOT NULL DEFAULT true,
  blossom_enabled BOOLEAN NOT NULL DEFAULT false,
  livekit_enabled BOOLEAN NOT NULL DEFAULT false,
  push_enabled BOOLEAN NOT NULL DEFAULT true,
  custom_domain TEXT,
  custom_domain_verified BOOLEAN NOT NULL DEFAULT false,
  blossom_authenticated_read BOOLEAN NOT NULL DEFAULT false,
  member_price BIGINT NOT NULL DEFAULT 0,
  member_interval TEXT NOT NULL DEFAULT 'month' CHECK (member_interval IN ('month', 'year')),
  purged_at BIGINT,
  erased_at BIGINT,
  created_at BIGINT NOT NULL,
  CONSTRAINT relay_pubkey_key UNIQUE (pubkey)
);

-- Keeps two live relays from claiming one subdomain; a purged relay's is free again.
CREATE UNIQUE INDEX relay_subdomain_key ON relay (subdomain) WHERE purged_at IS NULL;

-- A tenant's relays, newest first, for its dashboard and for billing.
CREATE INDEX relay_tenant_created_idx ON relay (tenant_pubkey, created_at);

-- Resolves a request's host to the relay whose verified custom domain it is, and
-- keeps two live relays from claiming one domain.
CREATE UNIQUE INDEX relay_custom_domain_key ON relay (custom_domain)
  WHERE custom_domain IS NOT NULL AND custom_domain_verified AND purged_at IS NULL;

-- Purged relays whose data is still to be erased.
CREATE INDEX relay_unerased_idx ON relay (id) WHERE purged_at IS NOT NULL AND erased_at IS NULL;

-- The domain verification poller's queue of custom domains still to verify.
CREATE INDEX relay_unverified_custom_domain_idx ON relay (id)
  WHERE custom_domain IS NOT NULL AND NOT custom_domain_verified AND purged_at IS NULL;

CREATE TABLE host_redirect (
  host TEXT PRIMARY KEY,
  relay_id TEXT NOT NULL REFERENCES relay (id),
  created_at BIGINT NOT NULL
);

-- The hosts that redirect to one relay.
CREATE INDEX host_redirect_relay_idx ON host_redirect (relay_id);

CREATE TABLE activity (
  id TEXT PRIMARY KEY,
  seq BIGINT GENERATED ALWAYS AS IDENTITY,
  tenant_pubkey TEXT NOT NULL REFERENCES tenant (pubkey),
  created_at BIGINT NOT NULL,
  activity_type TEXT NOT NULL,
  relay_id TEXT NOT NULL REFERENCES relay (id),
  billed_at BIGINT,
  snapshot JSONB NOT NULL
);

-- A relay's history in order, for its activity feed and for billing's reads of its
-- state at a period boundary.
CREATE INDEX activity_relay_created_idx ON activity (relay_id, created_at, seq);

-- A tenant's activity that billing has not reconciled yet.
CREATE INDEX activity_unbilled_idx ON activity (tenant_pubkey, created_at, seq) WHERE billed_at IS NULL;

CREATE TABLE invoice (
  id TEXT PRIMARY KEY,
  seq BIGINT GENERATED ALWAYS AS IDENTITY,
  tenant_pubkey TEXT NOT NULL REFERENCES tenant (pubkey),
  method TEXT CHECK (method IS NULL OR method IN ('nwc', 'stripe', 'oob', 'comp')),
  amount BIGINT NOT NULL,
  period_start BIGINT NOT NULL,
  period_end BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  paid_at BIGINT,
  voided_at BIGINT,
  notified_at BIGINT
);

-- A tenant's invoices, newest first.
CREATE INDEX invoice_tenant_created_idx ON invoice (tenant_pubkey, created_at, seq);

-- Dunning scans a tenant's still-open invoices oldest-first to retry payment.
CREATE INDEX invoice_open_idx ON invoice (tenant_pubkey, created_at, seq) WHERE paid_at IS NULL AND voided_at IS NULL;

CREATE TABLE invoice_item (
  id TEXT PRIMARY KEY,
  seq BIGINT GENERATED ALWAYS AS IDENTITY,
  invoice_id TEXT REFERENCES invoice (id),
  activity_id TEXT REFERENCES activity (id),
  tenant_pubkey TEXT NOT NULL REFERENCES tenant (pubkey),
  relay_id TEXT NOT NULL REFERENCES relay (id),
  plan_id TEXT NOT NULL,
  amount BIGINT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  created_at BIGINT NOT NULL,
  voided_at BIGINT
);

-- The items billed on one invoice.
CREATE INDEX invoice_item_invoice_idx ON invoice_item (invoice_id);

-- A tenant's outstanding items, which the next invoice claims.
CREATE INDEX invoice_item_outstanding_idx ON invoice_item (tenant_pubkey) WHERE invoice_id IS NULL AND voided_at IS NULL;

-- At most one line item per billable activity, so an activity is never billed twice.
CREATE UNIQUE INDEX invoice_item_activity_key ON invoice_item (activity_id);

CREATE TABLE bolt11 (
  id TEXT PRIMARY KEY,
  invoice_id TEXT NOT NULL REFERENCES invoice (id),
  lnbc TEXT NOT NULL,
  msats BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  expires_at BIGINT NOT NULL,
  settled_at BIGINT
);

-- An invoice's most recent Lightning invoice.
CREATE INDEX bolt11_invoice_created_idx ON bolt11 (invoice_id, created_at);

CREATE TABLE intent (
  id TEXT PRIMARY KEY,
  invoice_id TEXT NOT NULL REFERENCES invoice (id),
  payment_method_id TEXT NOT NULL,
  payment_intent_id TEXT,
  created_at BIGINT NOT NULL,
  settled_at BIGINT
);

-- At most one unsettled write-ahead intent per invoice, which is also the conflict
-- target of the get-or-create in `ensure_pending_intent`.
CREATE UNIQUE INDEX intent_unsettled_key ON intent (invoice_id) WHERE settled_at IS NULL;

CREATE TABLE checkout (
  id TEXT PRIMARY KEY,
  invoice_id TEXT NOT NULL REFERENCES invoice (id),
  session_id TEXT NOT NULL,
  url TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  expires_at BIGINT NOT NULL,
  settled_at BIGINT
);

-- An invoice's most recent Checkout session, and its unsettled ones.
CREATE INDEX checkout_invoice_created_idx ON checkout (invoice_id, created_at);

CREATE TABLE usage_sample (
  id TEXT PRIMARY KEY,
  relay_id TEXT NOT NULL REFERENCES relay (id),
  tenant_pubkey TEXT NOT NULL REFERENCES tenant (pubkey),
  storage_bytes BIGINT NOT NULL,
  participant_seconds BIGINT NOT NULL,
  created_at BIGINT NOT NULL
);

-- Closing a period rolls up every sample one tenant took inside it.
CREATE INDEX usage_sample_tenant_created_idx ON usage_sample (tenant_pubkey, created_at);

-- That rollup also reaches back for each relay's last sample before the period, to
-- use as the baseline the period's participant clock is measured against.
CREATE INDEX usage_sample_relay_created_idx ON usage_sample (relay_id, created_at);

CREATE TABLE session (
  token_hash TEXT PRIMARY KEY,
  pubkey TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  expires_at BIGINT NOT NULL,
  last_used_at BIGINT NOT NULL
);

-- Revoking every session a pubkey holds.
CREATE INDEX session_pubkey_idx ON session (pubkey);

-- Sweeping expired sessions.
CREATE INDEX session_expires_idx ON session (expires_at);

CREATE TABLE event (
  relay_id TEXT NOT NULL REFERENCES relay (id),
  id BYTEA NOT NULL,
  seq BIGINT GENERATED ALWAYS AS IDENTITY,
  created_at BIGINT NOT NULL,
  kind INTEGER NOT NULL,
  pubkey BYTEA NOT NULL,
  content TEXT NOT NULL,
  tags TEXT NOT NULL,
  sig BYTEA NOT NULL,
  tag_values TEXT[] NOT NULL,
  d TEXT,
  expires_at BIGINT,
  seen_at BIGINT NOT NULL,
  search TSVECTOR GENERATED ALWAYS AS (to_tsvector('simple', content)) STORED,
  PRIMARY KEY (relay_id, id)
);

-- A relay's events newest first, the order every client query reads in.
CREATE INDEX event_relay_created_idx ON event (relay_id, created_at DESC, id);

-- Queries by kind, by author, and by both.
CREATE INDEX event_relay_kind_created_idx ON event (relay_id, kind, created_at DESC, id);
CREATE INDEX event_relay_pubkey_created_idx ON event (relay_id, pubkey, created_at DESC, id);
CREATE INDEX event_relay_kind_pubkey_created_idx ON event (relay_id, kind, pubkey, created_at DESC, id);

-- Queries by single-letter tag, and the deletion tombstones checked on every write.
CREATE INDEX event_tag_values_idx ON event USING gin (tag_values);

-- NIP-50 full-text search.
CREATE INDEX event_search_idx ON event USING gin (search);

-- The moderation feed, which reads every relay's events by when they arrived.
CREATE INDEX event_seen_idx ON event (seen_at DESC, seq DESC);

-- One event per replaceable or addressable slot, which a newer version overwrites
-- in place.
CREATE UNIQUE INDEX event_slot_key ON event (relay_id, kind, pubkey, d) WHERE d IS NOT NULL;

-- The NIP-40 sweep of expired events.
CREATE INDEX event_expires_idx ON event (expires_at) WHERE expires_at IS NOT NULL;

CREATE TABLE relay_member (
  relay_id TEXT NOT NULL REFERENCES relay (id),
  pubkey TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('owner', 'manual', 'claim', 'public_join', 'payment', 'migrated')),
  roles TEXT[] NOT NULL DEFAULT '{}',
  created_at BIGINT NOT NULL,
  PRIMARY KEY (relay_id, pubkey)
);

-- The relays a pubkey belongs to.
CREATE INDEX relay_member_pubkey_idx ON relay_member (pubkey);

CREATE TABLE role (
  relay_id TEXT NOT NULL REFERENCES relay (id),
  id TEXT NOT NULL,
  label TEXT NOT NULL DEFAULT '',
  description TEXT NOT NULL DEFAULT '',
  hue INTEGER NOT NULL DEFAULT 0,
  position INTEGER NOT NULL DEFAULT 0,
  created_at BIGINT NOT NULL,
  PRIMARY KEY (relay_id, id)
);

CREATE TABLE method_grant (
  relay_id TEXT NOT NULL,
  pubkey TEXT NOT NULL,
  method TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  PRIMARY KEY (relay_id, pubkey, method),
  FOREIGN KEY (relay_id, pubkey) REFERENCES relay_member (relay_id, pubkey) ON DELETE CASCADE
);

CREATE TABLE banned_pubkey (
  relay_id TEXT NOT NULL REFERENCES relay (id),
  pubkey TEXT NOT NULL,
  reason TEXT NOT NULL DEFAULT '',
  created_at BIGINT NOT NULL,
  PRIMARY KEY (relay_id, pubkey)
);

CREATE TABLE banned_event (
  relay_id TEXT NOT NULL REFERENCES relay (id),
  event_id TEXT NOT NULL,
  reason TEXT NOT NULL DEFAULT '',
  created_at BIGINT NOT NULL,
  PRIMARY KEY (relay_id, event_id)
);

CREATE TABLE claim (
  relay_id TEXT NOT NULL REFERENCES relay (id),
  code TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  revoked_at BIGINT,
  PRIMARY KEY (relay_id, code)
);

CREATE TABLE claim_use (
  relay_id TEXT NOT NULL,
  claim_code TEXT NOT NULL,
  pubkey TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  FOREIGN KEY (relay_id, claim_code) REFERENCES claim (relay_id, code)
);

-- A claim's log of joins, oldest first.
CREATE INDEX claim_use_claim_idx ON claim_use (relay_id, claim_code, created_at);

CREATE TABLE room (
  relay_id TEXT NOT NULL REFERENCES relay (id),
  id TEXT NOT NULL CHECK (id <> ''),
  tags JSONB NOT NULL DEFAULT '[]',
  pins JSONB NOT NULL DEFAULT '[]',
  created_at BIGINT NOT NULL,
  PRIMARY KEY (relay_id, id)
);

CREATE TABLE room_member (
  relay_id TEXT NOT NULL,
  room_id TEXT NOT NULL,
  pubkey TEXT NOT NULL,
  roles TEXT[] NOT NULL DEFAULT '{}',
  permissions INTEGER[] NOT NULL DEFAULT '{}',
  created_at BIGINT NOT NULL,
  PRIMARY KEY (relay_id, room_id, pubkey),
  FOREIGN KEY (relay_id, room_id) REFERENCES room (relay_id, id) ON DELETE CASCADE
);

CREATE TABLE room_invite (
  relay_id TEXT NOT NULL,
  room_id TEXT NOT NULL,
  code TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  PRIMARY KEY (relay_id, room_id, code),
  FOREIGN KEY (relay_id, room_id) REFERENCES room (relay_id, id) ON DELETE CASCADE
);

CREATE TABLE blob (
  relay_id TEXT NOT NULL REFERENCES relay (id),
  sha256 TEXT NOT NULL,
  type TEXT NOT NULL,
  size BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  PRIMARY KEY (relay_id, sha256)
);

CREATE TABLE blob_owner (
  relay_id TEXT NOT NULL,
  blob_sha256 TEXT NOT NULL,
  pubkey TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  PRIMARY KEY (relay_id, blob_sha256, pubkey),
  FOREIGN KEY (relay_id, blob_sha256) REFERENCES blob (relay_id, sha256) ON DELETE CASCADE
);

-- A pubkey's uploads on a relay, for BUD-02 lists.
CREATE INDEX blob_owner_pubkey_idx ON blob_owner (relay_id, pubkey, created_at);

-- The moderation feed, which reads every relay's uploads by when they arrived.
CREATE INDEX blob_owner_created_idx ON blob_owner (created_at DESC);

CREATE TABLE livekit_session (
  relay_id TEXT NOT NULL REFERENCES relay (id),
  sid TEXT NOT NULL,
  room_id TEXT NOT NULL,
  seconds BIGINT NOT NULL,
  ended_at BIGINT NOT NULL,
  PRIMARY KEY (relay_id, sid)
);

CREATE TABLE notification (
  relay_id TEXT NOT NULL REFERENCES relay (id),
  pubkey TEXT NOT NULL,
  message TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  expires_at BIGINT,
  PRIMARY KEY (relay_id, pubkey)
);

CREATE TABLE membership (
  id TEXT PRIMARY KEY,
  relay_id TEXT NOT NULL REFERENCES relay (id),
  pubkey TEXT,
  invite_code TEXT,
  stripe_customer_id TEXT,
  stripe_subscription_id TEXT,
  status TEXT NOT NULL CHECK (status IN ('pending', 'active', 'canceled', 'lapsed')),
  current_period_end BIGINT,
  bound_at BIGINT,
  created_at BIGINT NOT NULL,
  CHECK (pubkey IS NOT NULL OR invite_code IS NOT NULL)
);

-- A relay and pubkey hold one live membership at a time; a lapsed one is history,
-- so paying again after a lapse opens a new row.
CREATE UNIQUE INDEX membership_live_key ON membership (relay_id, pubkey)
  WHERE status <> 'lapsed' AND pubkey IS NOT NULL;

-- Invite pages and joins find an invite's membership by its code.
CREATE UNIQUE INDEX membership_invite_key ON membership (invite_code) WHERE invite_code IS NOT NULL;

-- A relay's memberships, newest first, and its unused invites holding seats.
CREATE INDEX membership_relay_idx ON membership (relay_id, created_at);

-- A member's account, listing every membership they hold.
CREATE INDEX membership_pubkey_idx ON membership (pubkey) WHERE pubkey IS NOT NULL;

-- The sweep's queue: live memberships, by when their paid period ends.
CREATE INDEX membership_live_idx ON membership (current_period_end) WHERE status <> 'lapsed';
