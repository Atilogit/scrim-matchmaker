CREATE TABLE users (
    -- Discord user ID
    id BIGINT PRIMARY KEY,
    timezone VARCHAR(255) NOT NULL
);
CREATE TABLE scrims (
    id SERIAL PRIMARY KEY,
    creator_id BIGINT NOT NULL REFERENCES users(id),
    region VARCHAR(255) NOT NULL,
    platform VARCHAR(255) NOT NULL,
    rank_from INTEGER NOT NULL,
    rank_to INTEGER NOT NULL,
    time TIMESTAMPTZ NOT NULL,
    -- Id of the paired scrim. Can be NULL if the scrim is not paired yet.
    match_id INTEGER REFERENCES scrims(id)
);
