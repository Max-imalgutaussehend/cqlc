//! End-to-end demo of generated cqlc code.
//!
//! Needs a Cassandra-compatible node (`CQLC_NODE`, default `127.0.0.1:9042`):
//! it creates a keyspace, applies `schema/*.cql`, runs every query and
//! asserts on the results.

#[rustfmt::skip]
mod queries;

use chrono::{DateTime, TimeZone, Utc};
use queries::*;
use scylla::client::session_builder::SessionBuilder;
use uuid::Uuid;

const SCHEMA: [&str; 2] = [
    include_str!("../schema/events.cql"),
    include_str!("../schema/users.cql"),
];

fn ts(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).unwrap()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let node = std::env::var("CQLC_NODE").unwrap_or_else(|_| "127.0.0.1:9042".into());
    let session = SessionBuilder::new().known_node(node).build().await?;
    session
        .query_unpaged(
            "CREATE KEYSPACE IF NOT EXISTS cqlc_example WITH replication = \
             {'class': 'NetworkTopologyStrategy', 'replication_factor': 1}",
            &[],
        )
        .await?;
    session.use_keyspace("cqlc_example", false).await?;
    for file in SCHEMA {
        for stmt in file.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let stmt = stmt.replace("CREATE TABLE", "CREATE TABLE IF NOT EXISTS");
            session.query_unpaged(stmt, &[]).await?;
        }
    }

    // :exec with a parameter struct; `None` binds NULL.
    let id = Uuid::new_v4();
    insert_user(
        &session,
        InsertUserParams {
            user_id: id,
            username: Some("ada".into()),
            email: None,
            created_at: Some(ts(1_700_000_000)),
        },
    )
    .await?;

    // :one returns Option; absent cells are `None`.
    let user = get_user_by_id(&session, id).await?.expect("user exists");
    assert_eq!(user.user_id, id);
    assert_eq!(user.username.as_deref(), Some("ada"));
    assert_eq!(user.email, None);
    assert_eq!(user.created_at, Some(ts(1_700_000_000)));

    update_email(
        &session,
        UpdateEmailParams {
            email: Some("ada@example.com".into()),
            user_id: id,
        },
    )
    .await?;
    let user = get_user_by_id(&session, id).await?.expect("user exists");
    assert_eq!(user.email.as_deref(), Some("ada@example.com"));

    delete_user(&session, id).await?;
    assert!(get_user_by_id(&session, id).await?.is_none());

    // :many on a clustering table; CLUSTERING ORDER BY created_at DESC.
    let status = format!("active-{id}");
    for (i, name) in ["first", "second", "third"].iter().enumerate() {
        insert_user_by_status(
            &session,
            InsertUserByStatusParams {
                status: status.clone(),
                created_at: ts(1_700_000_000 + i as i64),
                user_id: Uuid::new_v4(),
                username: Some((*name).into()),
            },
        )
        .await?;
    }
    let rows = get_users_by_status(&session, status).await?;
    let names: Vec<_> = rows
        .iter()
        .map(|r| r.username.as_deref().unwrap())
        .collect();
    assert_eq!(names, ["third", "second", "first"]);

    // Range restriction + LIMIT, aliased column.
    let tenant = Uuid::new_v4();
    for i in 0..5 {
        session
            .query_unpaged(
                "INSERT INTO events (tenant_id, event_time, event_id, payload) VALUES (?, ?, ?, ?)",
                (
                    tenant,
                    ts(1_700_000_000 + i),
                    Uuid::new_v4(),
                    format!("event {i}"),
                ),
            )
            .await?;
    }
    let events = list_events_since(
        &session,
        ListEventsSinceParams {
            tenant_id: tenant,
            event_time: ts(1_700_000_002),
            limit: 2,
        },
    )
    .await?;
    let bodies: Vec<_> = events.iter().map(|e| e.body.as_deref().unwrap()).collect();
    assert_eq!(bodies, ["event 2", "event 3"]);

    println!("all generated queries behaved as expected");
    Ok(())
}
