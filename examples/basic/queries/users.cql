-- name: GetUserById :one
SELECT user_id, username, email, created_at
FROM users_by_id
WHERE user_id = ?;

-- name: GetUsersByStatus :many
SELECT user_id, username, created_at
FROM users_by_status
WHERE status = ?;

-- name: InsertUser :exec
INSERT INTO users_by_id (user_id, username, email, created_at)
VALUES (?, ?, ?, ?);

-- name: InsertUserByStatus :exec
INSERT INTO users_by_status (status, created_at, user_id, username)
VALUES (?, ?, ?, ?);

-- name: UpdateEmail :exec
UPDATE users_by_id SET email = ? WHERE user_id = ?;

-- name: DeleteUser :exec
DELETE FROM users_by_id WHERE user_id = ?;
