-- Singleton table (always exactly one row, id fixed to 1) holding
-- organization-wide branding and auth-policy settings that an admin can
-- change at runtime via the API without a redeploy. The row itself is
-- inserted by the application on first startup (see
-- servcat_db::repositories::org_settings::seed_default), seeded from
-- APP_NAME/ALLOW_LOCAL_SIGNUP env vars if present, so a fresh install can
-- set its organization name without an extra manual step.
CREATE TABLE org_settings (
    id                    TINYINT UNSIGNED NOT NULL PRIMARY KEY,
    app_name              VARCHAR(255) NOT NULL DEFAULT 'ServCat',
    logo_url              VARCHAR(512) NULL,
    allow_local_signup    BOOLEAN      NOT NULL DEFAULT FALSE,
    updated_at            TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,

    CONSTRAINT chk_org_settings_singleton CHECK (id = 1)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;
