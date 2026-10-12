-- name: GetAll :one
SELECT * FROM all_types WHERE id = ? AND seq = ?;

-- name: InsertScalars :exec
INSERT INTO all_types (
    id, seq, a_ascii, a_bigint, a_blob, a_boolean, a_date, a_decimal, a_double,
    a_duration, a_float, a_inet, a_smallint, a_text, a_time, a_timestamp
) VALUES (
    ?, ?, ?, ?, ?, ?, ?, ?, ?,
    ?, ?, ?, ?, ?, ?, ?
);

-- name: InsertRest :exec
INSERT INTO all_types (
    id, seq, a_timeuuid, a_tinyint, a_varint, a_list, a_set, a_map, a_tuple, a_frozen
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?);

-- name: ListInRange :many
SELECT seq, a_text AS label FROM all_types
WHERE id = ? AND seq >= ? AND seq < ?
ORDER BY seq DESC
LIMIT ?;

-- name: ListByIds :many
SELECT seq FROM all_types WHERE id IN ? AND seq = 1;

-- name: SetType :exec
UPDATE all_types USING TTL ? SET "type" = ? WHERE id = ? AND seq = ?;

-- name: DeleteRange :exec
DELETE FROM all_types WHERE id = ? AND seq > ?;

-- name: GetHits :one
SELECT hits FROM counters WHERE id = ?;
