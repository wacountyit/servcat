-- Users authenticate either locally (password_hash, argon2id) or via an external
-- IdP (external_idp_subject = the `sub` claim). At least one of the two should be
-- set by the application layer; the schema does not enforce it so that a user can
-- be migrated from local auth to SSO without a destructive change.
CREATE TABLE users (
    id                     BINARY(16)     NOT NULL PRIMARY KEY,
    email                  VARCHAR(255) NOT NULL,
    display_name           VARCHAR(255) NOT NULL,
    password_hash          VARCHAR(255) NULL,
    external_idp_subject   VARCHAR(255) NULL,
    department_id          BINARY(16)     NULL,
    manager_user_id        BINARY(16)     NULL,
    -- VARCHAR + CHECK rather than a native ENUM: sqlx's checked-enum decode
    -- compares MySQL type metadata, which does not reliably match a native
    -- ENUM column's reported type across drivers/versions.
    role                   VARCHAR(16)  NOT NULL DEFAULT 'requester'
                               CHECK (role IN ('requester', 'approver', 'agent', 'admin')),
    is_active              BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at             TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at             TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,

    CONSTRAINT uq_users_email UNIQUE (email),
    CONSTRAINT uq_users_external_idp_subject UNIQUE (external_idp_subject),
    CONSTRAINT fk_users_department
        FOREIGN KEY (department_id) REFERENCES departments (id)
        ON DELETE SET NULL,
    CONSTRAINT fk_users_manager
        FOREIGN KEY (manager_user_id) REFERENCES users (id)
        ON DELETE SET NULL
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE INDEX idx_users_department_id ON users (department_id);
CREATE INDEX idx_users_manager_user_id ON users (manager_user_id);
CREATE INDEX idx_users_role ON users (role);

-- Refresh-token sessions, so a logout / admin-initiated revocation actually
-- invalidates a session instead of waiting out a stateless JWT's expiry. Only
-- a salted hash of the refresh token is stored, never the token itself.
CREATE TABLE sessions (
    id                   BINARY(16)     NOT NULL PRIMARY KEY,
    user_id              BINARY(16)     NOT NULL,
    refresh_token_hash   CHAR(64)     NOT NULL,
    user_agent           VARCHAR(255) NULL,
    ip_address           VARCHAR(64)  NULL,
    expires_at           TIMESTAMP    NOT NULL,
    revoked_at           TIMESTAMP    NULL,
    created_at           TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT uq_sessions_refresh_token_hash UNIQUE (refresh_token_hash),
    CONSTRAINT fk_sessions_user
        FOREIGN KEY (user_id) REFERENCES users (id)
        ON DELETE CASCADE
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE INDEX idx_sessions_user_id ON sessions (user_id);
CREATE INDEX idx_sessions_expires_at ON sessions (expires_at);
