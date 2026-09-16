-- Optional per-department seal/logo, overriding the org-wide seal from
-- org_settings wherever that specific department's identity is shown
-- (e.g. a particular department's own seal on its catalog items).
ALTER TABLE departments ADD COLUMN logo_url VARCHAR(512) NULL AFTER name;
