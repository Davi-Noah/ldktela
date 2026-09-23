COMMENT ON COLUMN room_presence.publishing IS NULL;

ALTER TABLE room_presence
    DROP COLUMN screen_since,
    DROP COLUMN camera_since;
