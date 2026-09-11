-- Mevcut köylere makul başlangıç ordusu ver (geliştirme dünyası).
UPDATE village_armies
SET spears = GREATEST(spears, 100)
WHERE spears < 100;
