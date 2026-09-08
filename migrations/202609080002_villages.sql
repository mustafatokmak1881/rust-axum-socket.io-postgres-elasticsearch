CREATE TABLE villages (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL UNIQUE
        REFERENCES users(id) ON DELETE CASCADE,

    name TEXT NOT NULL DEFAULT 'Yeni Oba',
    wood BIGINT NOT NULL DEFAULT 1500 CHECK (wood >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT villages_name_length
        CHECK (char_length(name) BETWEEN 3 AND 32)
);

CREATE TABLE village_buildings (
    village_id UUID NOT NULL
        REFERENCES villages(id) ON DELETE CASCADE,

    kind TEXT NOT NULL
        CHECK (kind IN ('headquarters', 'timber', 'warehouse')),

    level INTEGER NOT NULL DEFAULT 1
        CHECK (level BETWEEN 1 AND 20),

    PRIMARY KEY (village_id, kind)
);

CREATE TABLE building_upgrades (
    job_id UUID PRIMARY KEY
        REFERENCES scheduled_jobs(id),

    village_id UUID NOT NULL,
    building_kind TEXT NOT NULL,

    target_level INTEGER NOT NULL
        CHECK (target_level BETWEEN 2 AND 20),

    completed_at TIMESTAMPTZ,

    FOREIGN KEY (village_id, building_kind)
        REFERENCES village_buildings(village_id, kind)
);

CREATE UNIQUE INDEX building_upgrades_one_pending_per_village
    ON building_upgrades (village_id)
    WHERE completed_at IS NULL;

CREATE INDEX building_upgrades_village_idx
    ON building_upgrades (village_id);