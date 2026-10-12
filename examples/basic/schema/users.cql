-- Two tables, two access paths: lookup by id and listing by status.
CREATE TABLE users_by_id (
    user_id uuid PRIMARY KEY,
    username text,
    email text,
    created_at timestamp
);

CREATE TABLE users_by_status (
    status text,
    created_at timestamp,
    user_id uuid,
    username text,
    PRIMARY KEY ((status), created_at, user_id)
) WITH CLUSTERING ORDER BY (created_at DESC);
