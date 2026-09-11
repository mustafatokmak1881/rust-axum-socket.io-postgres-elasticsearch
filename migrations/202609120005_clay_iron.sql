-- Klanlar.org hammaddeleri: odun, kil, demir.
ALTER TABLE villages
    ADD COLUMN clay BIGINT NOT NULL DEFAULT 1500 CHECK (clay >= 0),
    ADD COLUMN iron BIGINT NOT NULL DEFAULT 1500 CHECK (iron >= 0),
    ADD COLUMN clay_remainder BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN iron_remainder BIGINT NOT NULL DEFAULT 0;

ALTER TABLE villages
    ADD CONSTRAINT villages_clay_remainder_bounds
    CHECK (
        clay_remainder >= 0
        AND clay_remainder < 3600000000
    ),
    ADD CONSTRAINT villages_iron_remainder_bounds
    CHECK (
        iron_remainder >= 0
        AND iron_remainder < 3600000000
    );

ALTER TABLE army_attacks
    ADD COLUMN loot_clay BIGINT
        CHECK (loot_clay IS NULL OR loot_clay >= 0),
    ADD COLUMN loot_iron BIGINT
        CHECK (loot_iron IS NULL OR loot_iron >= 0);
