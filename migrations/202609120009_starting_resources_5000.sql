-- Yeni köy başlangıç hammaddeleri: 5000 odun / kil / demir.
ALTER TABLE villages
    ALTER COLUMN wood SET DEFAULT 5000,
    ALTER COLUMN clay SET DEFAULT 5000,
    ALTER COLUMN iron SET DEFAULT 5000;
