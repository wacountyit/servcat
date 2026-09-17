-- Single-use, short-lived tokens for local-account self-service password
-- reset. Only a SHA-256 hash of the raw token is stored, mirroring
-- `sessions.refresh_token_hash` -- the raw value only ever exists in the
-- emailed reset link, never in the database or logs.
CREATE TABLE password_resets (
    id           BINARY(16)   NOT NULL PRIMARY KEY,
    user_id      BINARY(16)   NOT NULL,
    token_hash   CHAR(64)     NOT NULL,
    expires_at   TIMESTAMP    NOT NULL,
    used_at      TIMESTAMP    NULL,
    created_at   TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT uq_password_resets_token_hash UNIQUE (token_hash),
    CONSTRAINT fk_password_resets_user
        FOREIGN KEY (user_id) REFERENCES users (id)
        ON DELETE CASCADE
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE INDEX idx_password_resets_user_id ON password_resets (user_id);
CREATE INDEX idx_password_resets_expires_at ON password_resets (expires_at);
