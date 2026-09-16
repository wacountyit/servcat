CREATE TABLE workflow_instances (
    id                       BINARY(16)  NOT NULL PRIMARY KEY,
    workflow_definition_id   BINARY(16)  NOT NULL,
    catalog_item_id          BINARY(16)  NOT NULL,
    requester_user_id        BINARY(16)  NOT NULL,
    status                   VARCHAR(20) NOT NULL DEFAULT 'in_progress'
                                 CHECK (status IN
                                     ('in_progress', 'awaiting_approval', 'completed', 'rejected', 'cancelled', 'failed')),
    current_step_id          VARCHAR(100) NOT NULL,
    -- Answers may contain requester-submitted personal data; access is limited
    -- to the requester, resolved approvers, agents and admins (see server authz).
    answers_json             JSON      NOT NULL,
    created_at               TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at               TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    completed_at             TIMESTAMP NULL,

    CONSTRAINT fk_instances_workflow_definition
        FOREIGN KEY (workflow_definition_id) REFERENCES workflow_definitions (id)
        ON DELETE RESTRICT,
    CONSTRAINT fk_instances_catalog_item
        FOREIGN KEY (catalog_item_id) REFERENCES service_catalog_items (id)
        ON DELETE RESTRICT,
    CONSTRAINT fk_instances_requester
        FOREIGN KEY (requester_user_id) REFERENCES users (id)
        ON DELETE RESTRICT
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE INDEX idx_instances_requester ON workflow_instances (requester_user_id);
CREATE INDEX idx_instances_status ON workflow_instances (status);

CREATE TABLE pending_approvals (
    id                       BINARY(16)  NOT NULL PRIMARY KEY,
    workflow_instance_id     BINARY(16)  NOT NULL,
    step_id                  VARCHAR(100) NOT NULL,
    approver_user_id         BINARY(16)  NOT NULL,
    status                   VARCHAR(16) NOT NULL DEFAULT 'pending'
                                 CHECK (status IN ('pending', 'approved', 'rejected', 'expired')),
    comment                  TEXT      NULL,
    decided_at               TIMESTAMP NULL,
    expires_at               TIMESTAMP NULL,
    created_at               TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT fk_pending_approvals_instance
        FOREIGN KEY (workflow_instance_id) REFERENCES workflow_instances (id)
        ON DELETE CASCADE,
    CONSTRAINT fk_pending_approvals_approver
        FOREIGN KEY (approver_user_id) REFERENCES users (id)
        ON DELETE RESTRICT
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE INDEX idx_pending_approvals_approver_status ON pending_approvals (approver_user_id, status);
CREATE INDEX idx_pending_approvals_instance ON pending_approvals (workflow_instance_id);
CREATE INDEX idx_pending_approvals_expires_at ON pending_approvals (expires_at);

CREATE TABLE tickets (
    id                       BINARY(16)  NOT NULL PRIMARY KEY,
    workflow_instance_id     BINARY(16)  NOT NULL,
    target_system            VARCHAR(16) NOT NULL
                                 CHECK (target_system IN ('jira', 'glpi', 'freshservice', 'webhook')),
    rendered_payload_json    JSON      NOT NULL,
    external_ticket_id       VARCHAR(255) NULL,
    external_ticket_url      VARCHAR(1024) NULL,
    dispatch_status          VARCHAR(16) NOT NULL DEFAULT 'pending'
                                 CHECK (dispatch_status IN ('pending', 'sent', 'failed', 'acked')),
    last_error               TEXT      NULL,
    created_at               TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at               TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,

    CONSTRAINT fk_tickets_instance
        FOREIGN KEY (workflow_instance_id) REFERENCES workflow_instances (id)
        ON DELETE CASCADE
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE INDEX idx_tickets_instance ON tickets (workflow_instance_id);
CREATE INDEX idx_tickets_dispatch_status ON tickets (dispatch_status);
