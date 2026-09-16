-- Append-only trail for admin/security-relevant actions (catalog and workflow
-- changes, user/role changes, approval decisions, connector dispatch). Never
-- updated or deleted by the application; retention/purge is an operator concern.
CREATE TABLE audit_log (
    id               BINARY(16)     NOT NULL PRIMARY KEY,
    actor_user_id    BINARY(16)     NULL,
    action           VARCHAR(100) NOT NULL,
    entity_type      VARCHAR(100) NOT NULL,
    entity_id        VARCHAR(100) NULL,
    metadata_json    JSON         NULL,
    created_at       TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT fk_audit_log_actor
        FOREIGN KEY (actor_user_id) REFERENCES users (id)
        ON DELETE SET NULL
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE INDEX idx_audit_log_entity ON audit_log (entity_type, entity_id);
CREATE INDEX idx_audit_log_actor ON audit_log (actor_user_id);
CREATE INDEX idx_audit_log_created_at ON audit_log (created_at);
