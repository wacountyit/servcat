-- Extends service_catalog_items with the fields needed for a real,
-- editable service catalog: a stable seeding key, a short card summary
-- (separate from the longer `description`), an adjustable target
-- fulfillment time, a display-only approval label, and structured
-- included/not-included/needed/keyword content.
--
-- `slug` is nullable (not every future catalog item needs to come from a
-- seed file) but unique when set, so the starter-catalog seed can safely
-- upsert-by-slug: skip any item whose slug already exists, so an admin's
-- edits (or a soft-delete via is_active = FALSE) are never overwritten by
-- re-running the seed.
ALTER TABLE service_catalog_items
    ADD COLUMN slug            VARCHAR(150) NULL AFTER id,
    ADD COLUMN summary         VARCHAR(500) NULL AFTER description,
    ADD COLUMN details_json    JSON         NULL AFTER summary,
    ADD COLUMN approval_label  VARCHAR(100) NULL AFTER details_json,
    ADD COLUMN target_value    INT          NULL AFTER approval_label,
    ADD COLUMN target_unit     VARCHAR(20)  NULL AFTER target_value
                                   CHECK (target_unit IS NULL OR target_unit IN
                                       ('minutes', 'hours', 'business_days', 'calendar_days')),
    ADD COLUMN sort_order      INT          NOT NULL DEFAULT 0 AFTER target_unit,
    ADD CONSTRAINT uq_catalog_items_slug UNIQUE (slug);
