-- Nooora initial schema.
-- series_id is TEXT, not CHAR(6): CHAR pads with spaces and breaks lookups.

CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE TABLE users (
    user_id BIGINT PRIMARY KEY,
    gold BIGINT NOT NULL DEFAULT 0,
    total_drops INTEGER NOT NULL DEFAULT 0,
    drop_charges SMALLINT NOT NULL DEFAULT 3,
    grab_charges SMALLINT NOT NULL DEFAULT 1,
    drop_next_regen TIMESTAMPTZ,
    grab_next_regen TIMESTAMPTZ,
    last_daily_claim DATE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT users_gold_nonnegative CHECK (gold >= 0),
    CONSTRAINT users_drops_nonnegative CHECK (total_drops >= 0),
    CONSTRAINT users_drop_charges_nonnegative CHECK (drop_charges >= 0),
    CONSTRAINT users_grab_charges_nonnegative CHECK (grab_charges >= 0)
);

CREATE TABLE series (
    series_id TEXT PRIMARY KEY,
    series_name TEXT NOT NULL,
    CONSTRAINT series_id_format CHECK (series_id ~ '^[0-9a-z]{6}$')
);

CREATE INDEX series_name_trgm_idx ON series USING gin (series_name gin_trgm_ops);
CREATE INDEX series_name_lower_idx ON series (lower(series_name));

CREATE TABLE card_templates (
    template_id BIGSERIAL PRIMARY KEY,
    character_name TEXT NOT NULL,
    series_id TEXT NOT NULL REFERENCES series (series_id),
    series_name TEXT NOT NULL,
    base_image_url TEXT,
    rarity_tier TEXT NOT NULL,
    weight INTEGER NOT NULL,
    CONSTRAINT card_templates_rarity CHECK (
        rarity_tier IN ('standard', 'rare', 'shiny', '3d', 'event')
    ),
    CONSTRAINT card_templates_weight_positive CHECK (weight > 0)
);

CREATE INDEX card_templates_series_idx ON card_templates (series_id);
CREATE INDEX card_templates_name_trgm_idx ON card_templates USING gin (character_name gin_trgm_ops);

CREATE TABLE cosmetics (
    cosmetic_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    rarity TEXT NOT NULL,
    weight INTEGER NOT NULL,
    CONSTRAINT cosmetics_kind CHECK (kind IN ('frame', 'filter', 'effect_3d', 'font')),
    CONSTRAINT cosmetics_rarity CHECK (
        rarity IN ('standard', 'rare', 'shiny', '3d', 'event')
    ),
    CONSTRAINT cosmetics_weight_positive CHECK (weight > 0)
);

CREATE INDEX cosmetics_kind_idx ON cosmetics (kind);

CREATE TABLE card_instances (
    card_uid TEXT PRIMARY KEY,
    owner_id BIGINT NOT NULL REFERENCES users (user_id),
    template_id BIGINT NOT NULL REFERENCES card_templates (template_id),
    print_number INTEGER NOT NULL,
    variant TEXT NOT NULL,
    cosmetics JSONB NOT NULL DEFAULT '{}'::jsonb,
    hold_kind TEXT,
    hold_ref TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT card_instances_uid_format CHECK (card_uid ~ '^[A-Za-z0-9]{8}$'),
    CONSTRAINT card_instances_print_positive CHECK (print_number > 0),
    CONSTRAINT card_instances_variant CHECK (
        variant IN ('normal', 'skin', 'shiny', '3d', 'event')
    ),
    CONSTRAINT card_instances_hold CHECK (
        hold_kind IS NULL OR hold_kind IN ('trade', 'market')
    ),
    CONSTRAINT card_instances_hold_pair CHECK (
        (hold_kind IS NULL AND hold_ref IS NULL)
        OR (hold_kind IS NOT NULL AND hold_ref IS NOT NULL)
    ),
    CONSTRAINT card_instances_print_unique UNIQUE (template_id, print_number)
);

CREATE INDEX card_instances_owner_idx ON card_instances (owner_id);
CREATE INDEX card_instances_template_idx ON card_instances (template_id);

CREATE TABLE template_print_counters (
    template_id BIGINT PRIMARY KEY REFERENCES card_templates (template_id),
    next_print INTEGER NOT NULL,
    CONSTRAINT template_print_counters_positive CHECK (next_print > 0)
);

CREATE TABLE user_cosmetics (
    user_id BIGINT NOT NULL REFERENCES users (user_id),
    cosmetic_id TEXT NOT NULL REFERENCES cosmetics (cosmetic_id),
    acquired_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, cosmetic_id)
);

CREATE TABLE albums (
    album_id BIGSERIAL PRIMARY KEY,
    owner_id BIGINT NOT NULL REFERENCES users (user_id),
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT albums_name_len CHECK (char_length(name) BETWEEN 1 AND 40),
    CONSTRAINT albums_owner_name_unique UNIQUE (owner_id, name)
);

CREATE TABLE album_cards (
    album_id BIGINT NOT NULL REFERENCES albums (album_id) ON DELETE CASCADE,
    card_uid TEXT NOT NULL REFERENCES card_instances (card_uid) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (album_id, card_uid)
);

CREATE TABLE guild_settings (
    guild_id BIGINT PRIMARY KEY,
    linked_series_id TEXT REFERENCES series (series_id),
    series_boost DOUBLE PRECISION NOT NULL DEFAULT 3,
    last_series_change_at TIMESTAMPTZ,
    CONSTRAINT guild_settings_boost_positive CHECK (series_boost > 0)
);

CREATE TABLE drops (
    drop_id BIGSERIAL PRIMARY KEY,
    message_id BIGINT UNIQUE,
    channel_id BIGINT NOT NULL,
    guild_id BIGINT,
    dropper_id BIGINT NOT NULL REFERENCES users (user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX drops_expires_idx ON drops (expires_at);

CREATE TABLE drop_slots (
    drop_id BIGINT NOT NULL REFERENCES drops (drop_id) ON DELETE CASCADE,
    slot SMALLINT NOT NULL,
    template_id BIGINT NOT NULL REFERENCES card_templates (template_id),
    claimed_by BIGINT REFERENCES users (user_id),
    card_uid TEXT REFERENCES card_instances (card_uid),
    PRIMARY KEY (drop_id, slot),
    CONSTRAINT drop_slots_slot_range CHECK (slot BETWEEN 1 AND 3),
    CONSTRAINT drop_slots_claim_pair CHECK (
        (claimed_by IS NULL AND card_uid IS NULL)
        OR (claimed_by IS NOT NULL AND card_uid IS NOT NULL)
    )
);

CREATE TABLE quest_defs (
    quest_id TEXT PRIMARY KEY,
    period TEXT NOT NULL,
    title TEXT NOT NULL,
    metric TEXT NOT NULL,
    target INTEGER NOT NULL,
    reward_gold INTEGER NOT NULL DEFAULT 0,
    reward_box BOOLEAN NOT NULL DEFAULT false,
    CONSTRAINT quest_defs_period CHECK (period IN ('daily', 'weekly', 'monthly')),
    CONSTRAINT quest_defs_target_positive CHECK (target > 0),
    CONSTRAINT quest_defs_reward_nonnegative CHECK (reward_gold >= 0)
);

CREATE TABLE quest_progress (
    user_id BIGINT NOT NULL REFERENCES users (user_id),
    quest_id TEXT NOT NULL REFERENCES quest_defs (quest_id),
    period_key TEXT NOT NULL,
    progress INTEGER NOT NULL DEFAULT 0,
    claimed BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (user_id, quest_id, period_key),
    CONSTRAINT quest_progress_nonnegative CHECK (progress >= 0)
);

CREATE TABLE events (
    event_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    starts_at TIMESTAMPTZ NOT NULL,
    ends_at TIMESTAMPTZ NOT NULL,
    series_id TEXT REFERENCES series (series_id),
    bonus_gold INTEGER NOT NULL DEFAULT 0,
    CONSTRAINT events_window CHECK (ends_at > starts_at),
    CONSTRAINT events_bonus_nonnegative CHECK (bonus_gold >= 0)
);

CREATE TABLE event_claims (
    user_id BIGINT NOT NULL REFERENCES users (user_id),
    event_id TEXT NOT NULL REFERENCES events (event_id),
    claimed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, event_id)
);

CREATE TABLE market_listings (
    listing_id BIGSERIAL PRIMARY KEY,
    seller_id BIGINT NOT NULL REFERENCES users (user_id),
    kind TEXT NOT NULL,
    card_uid TEXT REFERENCES card_instances (card_uid),
    cosmetic_id TEXT REFERENCES cosmetics (cosmetic_id),
    price_gold BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    buyer_id BIGINT REFERENCES users (user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    sold_at TIMESTAMPTZ,
    CONSTRAINT market_kind CHECK (kind IN ('card', 'cosmetic')),
    CONSTRAINT market_price_positive CHECK (price_gold > 0),
    CONSTRAINT market_status CHECK (status IN ('active', 'sold', 'cancelled')),
    CONSTRAINT market_item_shape CHECK (
        (kind = 'card' AND card_uid IS NOT NULL AND cosmetic_id IS NULL)
        OR (kind = 'cosmetic' AND cosmetic_id IS NOT NULL AND card_uid IS NULL)
    )
);

CREATE UNIQUE INDEX market_one_active_card_idx
    ON market_listings (card_uid)
    WHERE status = 'active' AND card_uid IS NOT NULL;

CREATE INDEX market_active_created_idx
    ON market_listings (created_at DESC)
    WHERE status = 'active';

CREATE TABLE trades (
    trade_id TEXT PRIMARY KEY,
    user_a BIGINT NOT NULL REFERENCES users (user_id),
    user_b BIGINT NOT NULL REFERENCES users (user_id),
    cards_a JSONB NOT NULL DEFAULT '[]'::jsonb,
    cards_b JSONB NOT NULL DEFAULT '[]'::jsonb,
    gold_a BIGINT NOT NULL DEFAULT 0,
    gold_b BIGINT NOT NULL DEFAULT 0,
    lock_a BOOLEAN NOT NULL DEFAULT false,
    lock_b BOOLEAN NOT NULL DEFAULT false,
    confirm_a BOOLEAN NOT NULL DEFAULT false,
    confirm_b BOOLEAN NOT NULL DEFAULT false,
    status TEXT NOT NULL DEFAULT 'open',
    channel_id BIGINT NOT NULL,
    message_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT trades_id_format CHECK (trade_id ~ '^[A-Za-z0-9]{12}$'),
    CONSTRAINT trades_distinct_users CHECK (user_a <> user_b),
    CONSTRAINT trades_gold_a_nonnegative CHECK (gold_a >= 0),
    CONSTRAINT trades_gold_b_nonnegative CHECK (gold_b >= 0),
    CONSTRAINT trades_status CHECK (status IN ('open', 'completed', 'cancelled'))
);

CREATE INDEX trades_open_users_idx ON trades (user_a, user_b) WHERE status = 'open';

CREATE TABLE cup_stats (
    user_id BIGINT PRIMARY KEY REFERENCES users (user_id),
    wins INTEGER NOT NULL DEFAULT 0,
    losses INTEGER NOT NULL DEFAULT 0,
    rating INTEGER NOT NULL DEFAULT 1000,
    CONSTRAINT cup_stats_wins_nonnegative CHECK (wins >= 0),
    CONSTRAINT cup_stats_losses_nonnegative CHECK (losses >= 0),
    CONSTRAINT cup_stats_rating_nonnegative CHECK (rating >= 0)
);

CREATE INDEX cup_stats_rating_idx ON cup_stats (rating DESC, wins DESC);

CREATE TABLE economy_audit_log (
    tx_id UUID PRIMARY KEY,
    kind TEXT NOT NULL,
    sender_id BIGINT,
    receiver_id BIGINT,
    cards_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    gold_delta BIGINT NOT NULL DEFAULT 0,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX economy_audit_sender_idx ON economy_audit_log (sender_id, created_at DESC);
CREATE INDEX economy_audit_receiver_idx ON economy_audit_log (receiver_id, created_at DESC);
CREATE INDEX economy_audit_kind_idx ON economy_audit_log (kind, created_at DESC);

CREATE OR REPLACE FUNCTION nooora_forbid_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'economy_audit_log is immutable';
END;
$$;

CREATE TRIGGER economy_audit_log_immutable
    BEFORE UPDATE OR DELETE ON economy_audit_log
    FOR EACH ROW
    EXECUTE PROCEDURE nooora_forbid_mutation();

-- Transaction precondition for the 100-drop gate. Command checks are the
-- other half of the invariant; this function is what economy transactions call
-- while the user row is locked.
CREATE OR REPLACE FUNCTION nooora_assert_trade_gate(p_user_id BIGINT, p_need INTEGER)
RETURNS INTEGER
LANGUAGE plpgsql
AS $$
DECLARE
    drops INTEGER;
BEGIN
    SELECT total_drops INTO drops
    FROM users
    WHERE user_id = p_user_id
    FOR UPDATE;

    IF drops IS NULL THEN
        RAISE EXCEPTION 'trade_gate:0/%', p_need USING ERRCODE = 'P0001';
    END IF;
    IF drops < p_need THEN
        RAISE EXCEPTION 'trade_gate:%/%', drops, p_need USING ERRCODE = 'P0001';
    END IF;
    RETURN drops;
END;
$$;
