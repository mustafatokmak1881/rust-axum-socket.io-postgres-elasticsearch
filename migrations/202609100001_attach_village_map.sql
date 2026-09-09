CREATE TABLE worlds (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    width INTEGER NOT NULL CHECK (width = 1000),
    height INTEGER NOT NULL CHECK (height = 1000),
    map_seed INTEGER NOT NULL,
    ruleset_version TEXT NOT NULL DEFAULT 'v1',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO worlds (
    id,
    name,
    width,
    height,
    map_seed
)
VALUES (
    '00000000-0000-0000-0000-000000000001',
    'Bozkır — Dünya 01',
    1000,
    1000,
    731
);

-- Köyler yeniden oluşturulmaz; mevcut tablo genişletilir.
ALTER TABLE villages
    ADD COLUMN world_id UUID,
    ADD COLUMN x INTEGER,
    ADD COLUMN y INTEGER;

DO $$
BEGIN
    IF (SELECT COUNT(*) FROM villages) > 1000000 THEN
        RAISE EXCEPTION 'Village count exceeds world capacity';
    END IF;
END
$$;

-- Mevcut köylere benzersiz koordinatlar ata.
-- İlk köy 450|450 konumundan başlar.
WITH numbered AS (
    SELECT
        id,
        ROW_NUMBER() OVER (ORDER BY created_at, id) - 1 AS position
    FROM villages
)
UPDATE villages AS v
SET
    world_id = '00000000-0000-0000-0000-000000000001'::uuid,
    x = ((450 + numbered.position % 1000) % 1000)::integer,
    y = ((450 + numbered.position / 1000) % 1000)::integer
FROM numbered
WHERE numbered.id = v.id;

ALTER TABLE villages
    ALTER COLUMN world_id SET NOT NULL,
    ALTER COLUMN x SET NOT NULL,
    ALTER COLUMN y SET NOT NULL,

    ADD CONSTRAINT villages_world_fk
        FOREIGN KEY (world_id) REFERENCES worlds(id),

    ADD CONSTRAINT villages_x_bounds
        CHECK (x BETWEEN 0 AND 999),

    ADD CONSTRAINT villages_y_bounds
        CHECK (y BETWEEN 0 AND 999),

    ADD CONSTRAINT villages_world_coordinates_unique
        UNIQUE (world_id, x, y);

CREATE INDEX villages_world_y_x_idx
    ON villages (world_id, y, x);