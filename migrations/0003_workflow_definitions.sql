-- A WorkflowDefinition is a versioned graph of steps (questions, branches,
-- approvals, ticket submission) stored as JSON and interpreted by the
-- workflow-engine crate. field_mapping_json describes, per target ticketing
-- system, how collected answers map onto that system's required fields
-- (project key, issue type, priority scheme, ...) so a reconfiguration never
-- requires a code change or redeploy.
CREATE TABLE workflow_definitions (
    id                  BINARY(16)     NOT NULL PRIMARY KEY,
    name                VARCHAR(255) NOT NULL,
    version             INT          NOT NULL DEFAULT 1,
    definition_json     JSON         NOT NULL,
    field_mapping_json  JSON         NULL,
    target_system       VARCHAR(16)  NULL
                            CHECK (target_system IN ('jira', 'glpi', 'freshservice', 'webhook')),
    is_published        BOOLEAN      NOT NULL DEFAULT FALSE,
    created_by          BINARY(16)     NULL,
    created_at          TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at          TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,

    CONSTRAINT uq_workflow_definitions_name_version UNIQUE (name, version),
    CONSTRAINT fk_workflow_definitions_created_by
        FOREIGN KEY (created_by) REFERENCES users (id)
        ON DELETE SET NULL
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE INDEX idx_workflow_definitions_published ON workflow_definitions (is_published);

CREATE TABLE service_catalog_items (
    id                       BINARY(16)     NOT NULL PRIMARY KEY,
    name                     VARCHAR(255) NOT NULL,
    description              TEXT         NULL,
    category                 VARCHAR(100) NULL,
    icon                     VARCHAR(100) NULL,
    workflow_definition_id   BINARY(16)     NOT NULL,
    is_active                BOOLEAN      NOT NULL DEFAULT TRUE,
    created_by               BINARY(16)     NULL,
    created_at               TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at               TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,

    CONSTRAINT fk_catalog_items_workflow_definition
        FOREIGN KEY (workflow_definition_id) REFERENCES workflow_definitions (id)
        ON DELETE RESTRICT,
    CONSTRAINT fk_catalog_items_created_by
        FOREIGN KEY (created_by) REFERENCES users (id)
        ON DELETE SET NULL
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE INDEX idx_catalog_items_active ON service_catalog_items (is_active);
CREATE INDEX idx_catalog_items_category ON service_catalog_items (category);
