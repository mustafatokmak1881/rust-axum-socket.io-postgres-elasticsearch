CREATE TABLE village_armies (
    village_id UUID PRIMARY KEY
        REFERENCES villages(id) ON DELETE CASCADE,

    spears BIGINT NOT NULL DEFAULT 0
        CHECK (spears >= 0)
);

CREATE TABLE army_attacks (
    id UUID PRIMARY KEY,

    owner_id UUID NOT NULL REFERENCES users(id),
    source_id UUID NOT NULL REFERENCES villages(id),
    target_id UUID NOT NULL REFERENCES villages(id),

    arrival_job_id UUID NOT NULL UNIQUE REFERENCES scheduled_jobs(id),
    return_job_id UUID UNIQUE REFERENCES scheduled_jobs(id),

    sent_spears BIGINT NOT NULL CHECK (sent_spears > 0),
    surviving_spears BIGINT,

    defender_before BIGINT,
    defender_after BIGINT,

    travel_seconds INTEGER NOT NULL CHECK (travel_seconds > 0),

    departed_at TIMESTAMPTZ NOT NULL,
    arrives_at TIMESTAMPTZ NOT NULL,
    resolved_at TIMESTAMPTZ,
    returns_at TIMESTAMPTZ,
    returned_at TIMESTAMPTZ,

    status TEXT NOT NULL DEFAULT 'outbound'
        CHECK (status IN ('outbound', 'returning', 'completed')),

    CONSTRAINT army_attacks_different_villages
        CHECK (source_id <> target_id)
);

CREATE INDEX army_attacks_owner_idx
    ON army_attacks (owner_id, departed_at DESC);

CREATE INDEX army_attacks_target_idx
    ON army_attacks (target_id, arrives_at);

-- Yalnızca geliştirme başlangıç ordusu.
-- Migration bir kez çalıştığı için tekrar tekrar asker vermez.
INSERT INTO village_armies (village_id, spears)
SELECT id, 50
FROM villages;