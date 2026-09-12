-- Üs ızgarasını 32 → 128 genişlet; mevcut yerleşimleri merkeze kaydır.

ALTER TABLE village_buildings
    DROP CONSTRAINT IF EXISTS village_buildings_tile_bounds;

-- Eski merkez 15 → yeni merkez 63 (+48).
UPDATE village_buildings
SET
    tile_x = tile_x + 48,
    tile_y = tile_y + 48
WHERE tile_x IS NOT NULL
  AND tile_y IS NOT NULL
  AND tile_x <= 31
  AND tile_y <= 31;

ALTER TABLE village_buildings
    ADD CONSTRAINT village_buildings_tile_bounds
    CHECK (
        (tile_x IS NULL AND tile_y IS NULL)
        OR (
            tile_x IS NOT NULL
            AND tile_y IS NOT NULL
            AND tile_x BETWEEN 0 AND 127
            AND tile_y BETWEEN 0 AND 127
        )
    );
