-- IANA timezone name (e.g. 'America/Chicago') used only to *render*
-- timestamps in the web UI and in notification emails -- every stored
-- timestamp remains UTC in every table regardless of this setting.
ALTER TABLE org_settings
    ADD COLUMN timezone VARCHAR(64) NOT NULL DEFAULT 'UTC' AFTER allow_local_signup;
