-- Koalisyon bina kataloğu.
-- Seviye 0 = henüz inşa edilmemiş.

ALTER TABLE village_buildings
    DROP CONSTRAINT IF EXISTS village_buildings_kind_check;

ALTER TABLE village_buildings
    ADD CONSTRAINT village_buildings_kind_check
    CHECK (kind IN (
        'headquarters',
        'barracks',
        'stable',
        'workshop',
        'academy',
        'smithy',
        'rally_point',
        'statue',
        'market',
        'timber',
        'clay',
        'iron',
        'farm',
        'warehouse',
        'hiding_place',
        'wall'
    ));

ALTER TABLE village_buildings
    DROP CONSTRAINT IF EXISTS village_buildings_level_check;

ALTER TABLE village_buildings
    ADD CONSTRAINT village_buildings_level_check
    CHECK (
        level >= 0
        AND level <= CASE kind
            WHEN 'headquarters' THEN 30
            WHEN 'timber' THEN 30
            WHEN 'clay' THEN 30
            WHEN 'iron' THEN 30
            WHEN 'farm' THEN 30
            WHEN 'warehouse' THEN 30
            WHEN 'barracks' THEN 25
            WHEN 'market' THEN 25
            WHEN 'stable' THEN 20
            WHEN 'smithy' THEN 20
            WHEN 'wall' THEN 20
            WHEN 'workshop' THEN 15
            WHEN 'hiding_place' THEN 10
            WHEN 'academy' THEN 1
            WHEN 'rally_point' THEN 1
            WHEN 'statue' THEN 1
            ELSE 0
        END
    );

ALTER TABLE building_upgrades
    DROP CONSTRAINT IF EXISTS building_upgrades_target_level_check;

ALTER TABLE building_upgrades
    ADD CONSTRAINT building_upgrades_target_level_check
    CHECK (
        target_level >= 1
        AND target_level <= CASE building_kind
            WHEN 'headquarters' THEN 30
            WHEN 'timber' THEN 30
            WHEN 'clay' THEN 30
            WHEN 'iron' THEN 30
            WHEN 'farm' THEN 30
            WHEN 'warehouse' THEN 30
            WHEN 'barracks' THEN 25
            WHEN 'market' THEN 25
            WHEN 'stable' THEN 20
            WHEN 'smithy' THEN 20
            WHEN 'wall' THEN 20
            WHEN 'workshop' THEN 15
            WHEN 'hiding_place' THEN 10
            WHEN 'academy' THEN 1
            WHEN 'rally_point' THEN 1
            WHEN 'statue' THEN 1
            ELSE 0
        END
    );

-- Mevcut köylere eksik binaları ekle.
INSERT INTO village_buildings (village_id, kind, level)
SELECT v.id, kinds.kind, kinds.level
FROM villages v
CROSS JOIN (
    VALUES
        ('headquarters', 1),
        ('barracks', 0),
        ('stable', 0),
        ('workshop', 0),
        ('academy', 0),
        ('smithy', 0),
        ('rally_point', 1),
        ('statue', 0),
        ('market', 0),
        ('timber', 1),
        ('clay', 1),
        ('iron', 1),
        ('farm', 1),
        ('warehouse', 1),
        ('hiding_place', 1),
        ('wall', 0)
) AS kinds(kind, level)
ON CONFLICT (village_id, kind) DO NOTHING;
