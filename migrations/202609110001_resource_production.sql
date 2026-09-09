ALTER TABLE villages
    ADD COLUMN resources_updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ADD COLUMN wood_remainder BIGINT NOT NULL DEFAULT 0;

-- Bir saat = 3.600.000.000 mikrosaniye.
-- Tam oduna dönüşmeyen üretim payı burada tutulur.
ALTER TABLE villages
    ADD CONSTRAINT villages_wood_remainder_bounds
    CHECK (
        wood_remainder >= 0
        AND wood_remainder < 3600000000
    );

-- Gönderdiğin migration'daki otomatik constraint isimleri.
ALTER TABLE village_buildings
    DROP CONSTRAINT village_buildings_level_check;

ALTER TABLE village_buildings
    ADD CONSTRAINT village_buildings_level_check
    CHECK (
        level >= 1
        AND level <= CASE
            WHEN kind = 'timber' THEN 30
            ELSE 20
        END
    );

ALTER TABLE building_upgrades
    DROP CONSTRAINT building_upgrades_target_level_check;

ALTER TABLE building_upgrades
    ADD CONSTRAINT building_upgrades_target_level_check
    CHECK (
        target_level >= 2
        AND target_level <= CASE
            WHEN building_kind = 'timber' THEN 30
            ELSE 20
        END
    );