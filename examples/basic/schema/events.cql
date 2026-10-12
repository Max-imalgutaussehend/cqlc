CREATE TABLE events (
    tenant_id uuid,
    event_time timestamp,
    event_id uuid,
    payload text,
    PRIMARY KEY ((tenant_id), event_time, event_id)
);
