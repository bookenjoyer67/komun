-- Every removal carries a reason and leaves a record the author can appeal.
CREATE TABLE moderation_actions (
    id UUID PRIMARY KEY,
    post_id UUID NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    actor_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    action TEXT NOT NULL CHECK (action IN ('hidden', 'flagged', 'restored')),
    reason TEXT NOT NULL,
    -- The status a granted appeal restores. The list is chk_posts_status (001_schema.sql:162):
    -- a status added there must be added here, or a hide of a post in that status fails.
    prior_status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_moderation_actions_prior_status CHECK (prior_status IN ('active', 'matched', 'fulfilled', 'expired', 'withdrawn', 'hidden', 'flagged'))
);
CREATE INDEX idx_moderation_actions_post ON moderation_actions(post_id, created_at DESC);

CREATE TABLE appeals (
    id UUID PRIMARY KEY,
    action_id UUID NOT NULL REFERENCES moderation_actions(id) ON DELETE CASCADE,
    author_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'granted', 'denied')),
    admin_notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ,
    UNIQUE (action_id)
);
