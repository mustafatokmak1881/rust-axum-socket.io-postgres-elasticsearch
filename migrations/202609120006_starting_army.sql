-- Mevcut köylere makul başlangıç ordusu ver (geliştirme dünyası).
UPDATE village_armies
SET spears = GREATEST(spears, 10000)
WHERE spears < 10000;
