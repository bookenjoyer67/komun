-- Saved searches and their digest. Additive: 001-004 are checksum-bookmarked and never edited.
--
-- chk_saved_searches_unique binds for the common case only because of NULLS NOT DISTINCT
-- (Postgres 15+): without it a search with no kind, category or centre is stored twice. The centre
-- and radius are in the key because the same words near two places are two searches, and
-- near_lat/near_lon hold the coarsened cell the server writes, so the key compares what the feed
-- uses.
--
-- last_notified_at is the digest's only bookkeeping. A digest covers the posts created after it,
-- and a run claims a window by moving it forward only while it still holds the value that run
-- read, so two runs cannot both notify for one window.
CREATE TABLE saved_searches (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label TEXT,
    q TEXT,
    kind TEXT,
    category TEXT,
    near_lat DOUBLE PRECISION,
    near_lon DOUBLE PRECISION,
    radius_km DOUBLE PRECISION,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_notified_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_saved_searches_unique UNIQUE NULLS NOT DISTINCT
        (user_id, q, kind, category, near_lat, near_lon, radius_km)
);
CREATE INDEX idx_saved_searches_user ON saved_searches(user_id);
