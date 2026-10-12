-- name: ListEventsSince :many
SELECT event_id, event_time, payload AS body
FROM events
WHERE tenant_id = ? AND event_time >= ?
LIMIT ?;
