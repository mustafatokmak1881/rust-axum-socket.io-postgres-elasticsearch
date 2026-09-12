-- Base yerleşim ızgarası (Generals tarzı yerel tile, 0..31).

ALTER TABLE village_buildings
    ADD COLUMN tile_x INTEGER,
    ADD COLUMN tile_y INTEGER;

ALTER TABLE village_buildings
    ADD CONSTRAINT village_buildings_tile_bounds
    CHECK (
        (tile_x IS NULL AND tile_y IS NULL)
        OR (
            tile_x IS NOT NULL
            AND tile_y IS NOT NULL
            AND tile_x BETWEEN 0 AND 31
            AND tile_y BETWEEN 0 AND 31
        )
    );

-- Bilinen başlangıç düzeni.
UPDATE village_buildings SET tile_x = 15, tile_y = 15
WHERE kind = 'headquarters' AND level > 0;

UPDATE village_buildings SET tile_x = 15, tile_y = 17
WHERE kind = 'rally_point' AND level > 0;

UPDATE village_buildings SET tile_x = 13, tile_y = 15
WHERE kind = 'farm' AND level > 0;

UPDATE village_buildings SET tile_x = 17, tile_y = 15
WHERE kind = 'warehouse' AND level > 0;

UPDATE village_buildings SET tile_x = 15, tile_y = 13
WHERE kind = 'hiding_place' AND level > 0;

-- Diğer kurulu binalar: boş karelere spiral yerleştir.
DO $$
DECLARE
    rec RECORD;
    ox INTEGER;
    oy INTEGER;
    layer INTEGER;
    placed BOOLEAN;
    try_x INTEGER;
    try_y INTEGER;
BEGIN
    FOR rec IN
        SELECT village_id, kind
        FROM village_buildings
        WHERE level > 0
          AND tile_x IS NULL
        ORDER BY village_id, kind
    LOOP
        placed := FALSE;

        FOR layer IN 0..15 LOOP
            FOR oy IN -layer..layer LOOP
                FOR ox IN -layer..layer LOOP
                    IF layer > 0 AND abs(ox) <> layer AND abs(oy) <> layer THEN
                        CONTINUE;
                    END IF;

                    try_x := 15 + ox;
                    try_y := 15 + oy;

                    IF try_x < 0 OR try_x > 31 OR try_y < 0 OR try_y > 31 THEN
                        CONTINUE;
                    END IF;

                    IF NOT EXISTS (
                        SELECT 1
                        FROM village_buildings
                        WHERE village_id = rec.village_id
                          AND tile_x = try_x
                          AND tile_y = try_y
                    ) THEN
                        UPDATE village_buildings
                        SET tile_x = try_x, tile_y = try_y
                        WHERE village_id = rec.village_id
                          AND kind = rec.kind;

                        placed := TRUE;
                        EXIT;
                    END IF;
                END LOOP;

                EXIT WHEN placed;
            END LOOP;

            EXIT WHEN placed;
        END LOOP;
    END LOOP;
END $$;

CREATE UNIQUE INDEX village_buildings_tile_unique
    ON village_buildings (village_id, tile_x, tile_y)
    WHERE tile_x IS NOT NULL AND tile_y IS NOT NULL;
