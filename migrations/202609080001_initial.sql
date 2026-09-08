CREATE TABLE users (
    id UUID PRIMARY KEY,
    google_sub TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL,
    email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Kimlik eşleştirmesi email üzerinden değil, Google'ın sabit sub değeriyle yapılır.
-- Email'e bilerek UNIQUE koymuyoruz.


CREATE TABLE oauth_login_flows (
    state_hash TEXT PRIMARY KEY,
    browser_token_hash TEXT NOT NULL,
    nonce TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX oauth_login_flows_expires_at_idx
    ON oauth_login_flows (expires_at);


CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX sessions_user_id_idx
    ON sessions (user_id);

CREATE INDEX sessions_expires_at_idx
    ON sessions (expires_at);


-- Aynı zamanda transactional outbox görevi görür.
-- İleride savaş kaydı ile bu tabloya yapılacak INSERT aynı transaction'da olacak.
CREATE TABLE scheduled_jobs (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    run_at TIMESTAMPTZ NOT NULL,

    last_enqueued_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT scheduled_jobs_payload_object
        CHECK (jsonb_typeof(payload) = 'object')
);

CREATE INDEX scheduled_jobs_pending_idx
    ON scheduled_jobs (last_enqueued_at, run_at)
    WHERE completed_at IS NULL;