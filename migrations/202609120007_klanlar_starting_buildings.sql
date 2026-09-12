-- Mevcut köyleri Klanlar.org başlangıç düzenine yaklaştır.
-- Kaynak binaları (oduncu/kil/demir) başlangıçta yoktur; oyuncu kurar.
-- İleride yükseltilmiş kaynak binalarına dokunulmaz.

UPDATE village_buildings AS vb
SET level = 0
FROM villages v
WHERE vb.village_id = v.id
  AND vb.kind IN ('timber', 'clay', 'iron')
  AND vb.level = 1
  AND NOT EXISTS (
      SELECT 1
      FROM building_upgrades u
      WHERE u.village_id = v.id
        AND u.building_kind = vb.kind
  );

-- Klasik başlangıç binalarının en az seviye 1 olduğundan emin ol.
INSERT INTO village_buildings (village_id, kind, level)
SELECT v.id, kinds.kind, kinds.level
FROM villages v
CROSS JOIN (
    VALUES
        ('headquarters', 1),
        ('rally_point', 1),
        ('farm', 1),
        ('warehouse', 1),
        ('hiding_place', 1),
        ('timber', 0),
        ('clay', 0),
        ('iron', 0),
        ('barracks', 0),
        ('stable', 0),
        ('workshop', 0),
        ('academy', 0),
        ('smithy', 0),
        ('statue', 0),
        ('market', 0),
        ('wall', 0)
) AS kinds(kind, level)
ON CONFLICT (village_id, kind) DO NOTHING;
