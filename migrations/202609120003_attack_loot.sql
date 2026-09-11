-- Saldırı ganimeti: sağ kalan birliklerin taşıdığı odun.
ALTER TABLE army_attacks
    ADD COLUMN loot_wood BIGINT
        CHECK (loot_wood IS NULL OR loot_wood >= 0);
