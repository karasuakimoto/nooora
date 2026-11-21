-- Curated series so `fseries Naruto` and `/admin series set 086c62` have a real target.
-- The generated catalog below is original placeholder data at production scale.

INSERT INTO series (series_id, series_name) VALUES
    ('086c62', 'Naruto'),
    ('1a2b3c', 'One Piece'),
    ('4d5e6f', 'Jujutsu Kaisen'),
    ('778899', 'Spy x Family'),
    ('aa10bb', 'Frieren'),
    ('cc20dd', 'Demon Slayer'),
    ('ee30ff', 'Attack on Titan'),
    ('112233', 'My Hero Academia')
ON CONFLICT (series_id) DO NOTHING;

INSERT INTO card_templates (character_name, series_id, series_name, base_image_url, rarity_tier, weight) VALUES
    ('Naruto Uzumaki', '086c62', 'Naruto', NULL, 'standard', 1000),
    ('Sasuke Uchiha', '086c62', 'Naruto', NULL, 'rare', 180),
    ('Sakura Haruno', '086c62', 'Naruto', NULL, 'standard', 1000),
    ('Kakashi Hatake', '086c62', 'Naruto', NULL, 'shiny', 40),
    ('Hinata Hyuga', '086c62', 'Naruto', NULL, 'rare', 180),
    ('Itachi Uchiha', '086c62', 'Naruto', NULL, 'event', 4),
    ('Monkey D. Luffy', '1a2b3c', 'One Piece', NULL, 'standard', 1000),
    ('Roronoa Zoro', '1a2b3c', 'One Piece', NULL, 'rare', 180),
    ('Nami', '1a2b3c', 'One Piece', NULL, 'standard', 1000),
    ('Nico Robin', '1a2b3c', 'One Piece', NULL, 'shiny', 40),
    ('Yuji Itadori', '4d5e6f', 'Jujutsu Kaisen', NULL, 'standard', 1000),
    ('Megumi Fushiguro', '4d5e6f', 'Jujutsu Kaisen', NULL, 'rare', 180),
    ('Nobara Kugisaki', '4d5e6f', 'Jujutsu Kaisen', NULL, 'standard', 1000),
    ('Satoru Gojo', '4d5e6f', 'Jujutsu Kaisen', NULL, '3d', 12),
    ('Loid Forger', '778899', 'Spy x Family', NULL, 'rare', 180),
    ('Yor Forger', '778899', 'Spy x Family', NULL, 'shiny', 40),
    ('Anya Forger', '778899', 'Spy x Family', NULL, 'event', 4),
    ('Frieren', 'aa10bb', 'Frieren', NULL, 'shiny', 40),
    ('Fern', 'aa10bb', 'Frieren', NULL, 'rare', 180),
    ('Stark', 'aa10bb', 'Frieren', NULL, 'standard', 1000),
    ('Tanjiro Kamado', 'cc20dd', 'Demon Slayer', NULL, 'standard', 1000),
    ('Nezuko Kamado', 'cc20dd', 'Demon Slayer', NULL, 'rare', 180),
    ('Zenitsu Agatsuma', 'cc20dd', 'Demon Slayer', NULL, 'standard', 1000),
    ('Eren Yeager', 'ee30ff', 'Attack on Titan', NULL, 'rare', 180),
    ('Mikasa Ackerman', 'ee30ff', 'Attack on Titan', NULL, 'shiny', 40),
    ('Armin Arlert', 'ee30ff', 'Attack on Titan', NULL, 'standard', 1000),
    ('Izuku Midoriya', '112233', 'My Hero Academia', NULL, 'standard', 1000),
    ('Katsuki Bakugo', '112233', 'My Hero Academia', NULL, 'rare', 180),
    ('Ochaco Uraraka', '112233', 'My Hero Academia', NULL, 'standard', 1000),
    ('Shoto Todoroki', '112233', 'My Hero Academia', NULL, '3d', 12);

INSERT INTO series (series_id, series_name)
SELECT lpad(to_hex(10000 + bucket), 6, '0'), 'Catalog Series ' || bucket
FROM generate_series(0, 1999) AS bucket
ON CONFLICT (series_id) DO NOTHING;

INSERT INTO card_templates (character_name, series_id, series_name, base_image_url, rarity_tier, weight)
SELECT
    'Unit ' || (((g - 1) % 20) + 1),
    lpad(to_hex(10000 + ((g - 1) / 20)), 6, '0'),
    'Catalog Series ' || ((g - 1) / 20),
    NULL,
    CASE
        WHEN g % 100 < 70 THEN 'standard'
        WHEN g % 100 < 88 THEN 'rare'
        WHEN g % 100 < 95 THEN 'shiny'
        WHEN g % 100 < 98 THEN '3d'
        ELSE 'event'
    END,
    CASE
        WHEN g % 100 < 70 THEN 1000
        WHEN g % 100 < 88 THEN 180
        WHEN g % 100 < 95 THEN 40
        WHEN g % 100 < 98 THEN 12
        ELSE 4
    END
FROM generate_series(1, 40000) AS g;

INSERT INTO cosmetics (cosmetic_id, kind, name, rarity, weight)
SELECT
    kind || '_' || n,
    kind,
    kind || ' ' || n,
    CASE
        WHEN n % 20 = 0 THEN 'event'
        WHEN n % 10 = 0 THEN '3d'
        WHEN n % 5 = 0 THEN 'shiny'
        WHEN n % 3 = 0 THEN 'rare'
        ELSE 'standard'
    END,
    CASE
        WHEN n % 20 = 0 THEN 4
        WHEN n % 10 = 0 THEN 12
        WHEN n % 5 = 0 THEN 30
        WHEN n % 3 = 0 THEN 80
        ELSE 200
    END
FROM (
    SELECT 'frame' AS kind, n FROM generate_series(1, 8000) AS n
    UNION ALL
    SELECT 'filter', n FROM generate_series(1, 2000) AS n
    UNION ALL
    SELECT 'effect_3d', n FROM generate_series(1, 1500) AS n
    UNION ALL
    SELECT 'font', n FROM generate_series(1, 500) AS n
) AS generated;

INSERT INTO quest_defs (quest_id, period, title, metric, target, reward_gold, reward_box) VALUES
    ('daily_drops_5', 'daily', 'Complete 5 drops', 'drops', 5, 10, false),
    ('daily_cosmetic_1', 'daily', 'Apply 1 cosmetic', 'apply_cosmetic', 1, 0, true),
    ('weekly_cup_2', 'weekly', 'Win 2 Nooora Cup matches', 'cup_wins', 2, 25, true),
    ('weekly_drops_20', 'weekly', 'Complete 20 drops', 'drops', 20, 15, false),
    ('monthly_trades_3', 'monthly', 'Complete 3 trades', 'trades', 3, 50, true);

INSERT INTO events (event_id, name, starts_at, ends_at, series_id, bonus_gold) VALUES
    (
        'season_aurora',
        'Aurora Season',
        now() - INTERVAL '1 day',
        now() + INTERVAL '30 days',
        '086c62',
        8
    );
