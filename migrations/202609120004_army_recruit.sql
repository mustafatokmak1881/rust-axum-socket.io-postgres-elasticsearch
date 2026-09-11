CREATE TABLE army_recruits (
    job_id UUID PRIMARY KEY
        REFERENCES scheduled_jobs(id),

    village_id UUID NOT NULL
        REFERENCES villages(id) ON DELETE CASCADE,

    unit_kind TEXT NOT NULL
        CHECK (unit_kind IN ('spear')),

    count BIGINT NOT NULL
        CHECK (count > 0),

    completed_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX army_recruits_one_pending_per_village
    ON army_recruits (village_id)
    WHERE completed_at IS NULL;

CREATE INDEX army_recruits_village_idx
    ON army_recruits (village_id);
