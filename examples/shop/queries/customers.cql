-- name: CreateCustomer :exec
INSERT INTO customers (customer_id, name, email, vip, birthday, created_at, tags, phones)
VALUES (?, ?, ?, ?, ?, toTimestamp(now()), {'new'}, ['none']);

-- name: CreateCustomerFromJson :exec
INSERT INTO customers JSON ?;

-- name: CreateCustomerIfAbsent :lwt
INSERT INTO customers (customer_id, name, email) VALUES (?, ?, ?) IF NOT EXISTS;

-- name: GetCustomer :one
SELECT customer_id, name, email, vip, birthday, created_at, primary_address, addresses, tags, phones
FROM customers WHERE customer_id = ?;

-- name: GetCustomerJson :one
SELECT JSON name, email FROM customers WHERE customer_id = ?;

-- name: CustomerMeta :one
SELECT writetime(email) AS email_written_at, ttl(name) AS name_ttl, token(customer_id) AS tok,
       toUnixTimestamp(created_at) AS created_unix, primary_address.city AS city,
       cast(vip AS text) AS vip_text
FROM customers WHERE customer_id = ?;

-- allow: CQL201
-- name: CountCustomers :one
SELECT count(*) AS total FROM customers;

-- name: SetPrimaryAddress :exec
UPDATE customers SET primary_address = ? WHERE customer_id = ?;

-- name: AddTag :exec
UPDATE customers SET tags = tags + ? WHERE customer_id = ?;

-- name: RemoveTag :exec
UPDATE customers SET tags = tags - ? WHERE customer_id = ?;

-- name: PrependPhone :exec
UPDATE customers SET phones = ? + phones WHERE customer_id = ?;

-- name: SetAddress :exec
UPDATE customers SET addresses[?] = ? WHERE customer_id = ?;

-- name: ClearAddress :exec
DELETE addresses[?] FROM customers WHERE customer_id = ?;

-- name: ClearCollections :exec
DELETE tags, phones FROM customers WHERE customer_id = ?;

-- name: RenameCustomer :exec
UPDATE customers USING TTL ? SET name = ? WHERE customer_id = ?;

-- name: CustomersByIds :many
SELECT customer_id, name FROM customers WHERE customer_id IN ?;

-- allow: CQL201, CQL202
-- name: CustomersByTag :many
SELECT customer_id, name FROM customers WHERE tags CONTAINS ? ALLOW FILTERING;

-- name: ScanByToken :many
SELECT customer_id FROM customers WHERE token(customer_id) > token(?) LIMIT ?;

-- name: DeleteCustomerIfExists :lwt
DELETE FROM customers WHERE customer_id = ? IF EXISTS;
