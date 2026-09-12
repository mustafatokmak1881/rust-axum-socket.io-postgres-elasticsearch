-- Ana üs (komuta merkezi) bayrağı.
-- İleride ele geçirilen üslerde is_capital = false olacak.
ALTER TABLE villages
    ADD COLUMN is_capital BOOLEAN NOT NULL DEFAULT FALSE;

-- Mevcut tek üssü ana üs yap.
UPDATE villages
SET is_capital = TRUE;

-- Aynı anda tek ana üs (oyuncu başına).
CREATE UNIQUE INDEX villages_one_capital_per_owner
    ON villages (owner_id)
    WHERE is_capital;
