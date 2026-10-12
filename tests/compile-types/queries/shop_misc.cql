-- name: CountView :exec
UPDATE product_views SET views = views + ? WHERE product_id = ?;

-- name: ViewCount :one
SELECT views FROM product_views WHERE product_id = ?;

-- name: AddReading :exec
INSERT INTO sensor_readings (sensor_id, day, ts, value) VALUES (?, ?, ?, ?);

-- name: ReadingsOfDay :many
SELECT ts, value FROM sensor_readings
WHERE sensor_id = ? AND day = ? AND ts >= ? AND ts < ?;

-- name: ReadingStats :one
SELECT count(*) AS n, min(value) AS lo, max(value) AS hi, avg(value) AS mean
FROM sensor_readings WHERE sensor_id = ? AND day = ?;

-- name: ReserveStock :lwt
UPDATE inventory SET reserved = ? WHERE sku = ? IF stock >= ?;

-- name: CreateSession :exec
INSERT INTO sessions (session_token, customer_id) VALUES (?, ?) USING TTL ?;

-- name: SessionCustomer :one
SELECT customer_id FROM sessions WHERE session_token = ?;
