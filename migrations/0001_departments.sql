CREATE TABLE departments (
    id                    BINARY(16)     NOT NULL PRIMARY KEY,
    name                  VARCHAR(255) NOT NULL,
    parent_department_id  BINARY(16)     NULL,
    created_at            TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at            TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,

    CONSTRAINT uq_departments_name UNIQUE (name),
    CONSTRAINT fk_departments_parent
        FOREIGN KEY (parent_department_id) REFERENCES departments (id)
        ON DELETE SET NULL
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;
