-- Fraksiyon: USA / China / GLA (Command & Conquer Generals).
ALTER TABLE users
    ADD COLUMN faction TEXT
        CHECK (faction IS NULL OR faction IN ('usa', 'china', 'gla'));

ALTER TABLE villages
    ADD COLUMN faction TEXT
        CHECK (faction IS NULL OR faction IN ('usa', 'china', 'gla'));

-- Mevcut üsler varsayılan USA (geliştirme dünyası).
UPDATE villages
SET faction = 'usa'
WHERE faction IS NULL;

UPDATE users AS u
SET faction = v.faction
FROM villages AS v
WHERE v.owner_id = u.id
  AND u.faction IS NULL;

ALTER TABLE villages
    ALTER COLUMN faction SET DEFAULT 'usa',
    ALTER COLUMN faction SET NOT NULL;
