CREATE TABLE worlds (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    width INTEGER NOT NULL CHECK (width = 1000),
    height INTEGER NOT NULL CHECK (height = 1000),
    map_seed INTEGER NOT NULL,
    ruleset_version TEXT NOT NULL DEFAULT 'v1',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE villages (
    id UUID PRIMARY KEY,
    world_id UUID NOT NULL REFERENCES worlds(id),
    owner_id UUID REFERENCES users(id) ON DELETE RESTRICT,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    x INTEGER NOT NULL CHECK (x BETWEEN 0 AND 999),
    y INTEGER NOT NULL CHECK (y BETWEEN 0 AND 999),
    points INTEGER NOT NULL DEFAULT 0 CHECK (points >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT villages_world_coordinates_unique
        UNIQUE (world_id, x, y)
);

CREATE INDEX villages_owner_idx
    ON villages (world_id, owner_id);

CREATE INDEX villages_world_y_x_idx
    ON villages (world_id, y, x);

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

-- Geliştirme dünyasında 400 kalıcı barbar köyü.
-- Yerleşimler merkez çevresindeki 200 x 200 alana dağıtılır.
-- UUID ve koordinatlar deterministiktir.
INSERT INTO villages (
    id,
    world_id,
    owner_id,
    name,
    x,
    y,
    points
)
SELECT
    md5('umaykut-barbar-' || gx::text || '-' || gy::text)::uuid,
    '00000000-0000-0000-0000-000000000001'::uuid,
    NULL,
    'Terk edilmiş oba',
    400 + gx * 10 + ((gx * 17 + gy * 11) % 7),
    400 + gy * 10 + ((gx * 13 + gy * 19) % 7),
    80 + ((gx * 29 + gy * 37) % 420)
FROM generate_series(0, 19) AS gx
CROSS JOIN generate_series(0, 19) AS gy;