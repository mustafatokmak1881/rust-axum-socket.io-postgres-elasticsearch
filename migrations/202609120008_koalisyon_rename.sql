-- Marka / varsayılan köy adı güncellemesi.
ALTER TABLE villages
    ALTER COLUMN name SET DEFAULT 'Yeni Köy';

UPDATE villages
SET name = 'Yeni Köy'
WHERE name = 'Yeni Oba';
