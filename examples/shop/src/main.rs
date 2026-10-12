//! Acceptance test for cqlc: every kind of query in `queries/*.cql` is run
//! against a real Cassandra-compatible node (`CQLC_NODE`, default
//! `127.0.0.1:9042`) and its result is asserted.

#![allow(dead_code)]

#[rustfmt::skip]
mod queries;

use chrono::{Duration, NaiveDate, TimeZone, Utc};
use futures::TryStreamExt;
use queries::*;
use scylla::client::session_builder::SessionBuilder;
use scylla::value::{Counter, CqlDecimal};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use uuid::Uuid;

const SCHEMA: &str = include_str!("../schema/shop.cql");

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn dec(unscaled: i64, scale: i32) -> CqlDecimal {
    CqlDecimal::from_signed_be_bytes_slice_and_exponent(&unscaled.to_be_bytes(), scale)
}

fn dec_f64(d: &CqlDecimal) -> f64 {
    let (bytes, scale) = d.as_signed_be_bytes_slice_and_exponent();
    let mut v: i128 = if bytes.first().is_some_and(|b| b & 0x80 != 0) {
        -1
    } else {
        0
    };
    for b in bytes {
        v = (v << 8) | *b as i128;
    }
    v as f64 / 10f64.powi(scale)
}

fn address(city: &str) -> Address {
    Address {
        street: Some("Main St 1".into()),
        city: Some(city.into()),
        zip: Some(10115),
    }
}

#[tokio::main]
async fn main() -> Res {
    let node = std::env::var("CQLC_NODE").unwrap_or_else(|_| "127.0.0.1:9042".into());
    let session = SessionBuilder::new().known_node(node).build().await?;
    session
        .query_unpaged("DROP KEYSPACE IF EXISTS cqlc_shop", &[])
        .await?;
    session
        .query_unpaged(
            "CREATE KEYSPACE cqlc_shop WITH replication = {'class': 'NetworkTopologyStrategy', 'replication_factor': 1}",
            &[],
        )
        .await?;
    session.use_keyspace("cqlc_shop", false).await?;
    for stmt in SCHEMA.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        session.query_unpaged(stmt, &[]).await?;
    }
    session
        .query_unpaged(
            "INSERT INTO inventory (sku, stock, reserved) VALUES ('sku-1', 10, 0)",
            &[],
        )
        .await?;

    // Prepared once, shared everywhere.
    let q = Queries::new(Arc::new(session)).await?;

    customers(&q).await?;
    orders(&q).await?;
    misc(&q).await?;
    println!("all {} demo queries behaved as expected", 36);
    Ok(())
}

async fn customers(q: &Queries) -> Res {
    let ada = Uuid::new_v4();
    let bob = Uuid::new_v4();
    let grace = Uuid::new_v4();

    // INSERT with literals, set/list literals and toTimestamp(now()).
    q.create_customer(CreateCustomerParams {
        customer_id: ada,
        name: Some("Ada".into()),
        email: Some("ada@example.com".into()),
        vip: Some(true),
        birthday: NaiveDate::from_ymd_opt(1815, 12, 10),
    })
    .await?;
    let c = q.get_customer(ada).await?.expect("ada exists");
    assert_eq!(c.name.as_deref(), Some("Ada"));
    assert_eq!(c.tags, Some(BTreeSet::from(["new".to_string()])));
    assert_eq!(c.phones, Some(vec!["none".to_string()]));
    let age = Utc::now() - c.created_at.expect("created_at set by toTimestamp(now())");
    assert!(age < Duration::minutes(5) && age > Duration::minutes(-5));
    assert_eq!(c.birthday, NaiveDate::from_ymd_opt(1815, 12, 10));

    // INSERT ... JSON ?
    q.create_customer_from_json(format!(r#"{{"customer_id": "{grace}", "name": "Grace"}}"#))
        .await?;
    assert_eq!(
        q.get_customer(grace).await?.unwrap().name.as_deref(),
        Some("Grace")
    );

    // LWT: IF NOT EXISTS
    let first = q
        .create_customer_if_absent(CreateCustomerIfAbsentParams {
            customer_id: bob,
            name: Some("Bob".into()),
            email: None,
        })
        .await?;
    assert!(first.applied);
    let second = q
        .create_customer_if_absent(CreateCustomerIfAbsentParams {
            customer_id: bob,
            name: Some("Robert".into()),
            email: None,
        })
        .await?;
    assert!(!second.applied);
    assert_eq!(
        second.name.as_deref(),
        Some("Bob"),
        "the existing row is reported back"
    );
    assert_eq!(second.customer_id, Some(bob));

    // SELECT JSON
    let json = q.get_customer_json(ada).await?.unwrap().json;
    assert!(
        json.contains("\"name\": \"Ada\"") && json.contains("ada@example.com"),
        "{json}"
    );

    // writetime / ttl / token / toUnixTimestamp / UDT field / cast
    let meta = q.customer_meta(ada).await?.unwrap();
    assert!(meta.email_written_at.is_some());
    assert_eq!(meta.name_ttl, None);
    assert!(meta.created_unix.unwrap() > 1_600_000_000_000);
    assert_eq!(meta.city, None);
    assert_eq!(meta.vip_text.as_deref(), Some("true"));

    // UDT column and UDT field selection
    q.set_primary_address(SetPrimaryAddressParams {
        primary_address: Some(address("Berlin")),
        customer_id: ada,
    })
    .await?;
    assert_eq!(
        q.customer_meta(ada).await?.unwrap().city.as_deref(),
        Some("Berlin")
    );
    assert_eq!(
        q.get_customer(ada).await?.unwrap().primary_address,
        Some(address("Berlin"))
    );

    // Collection updates: set +/-, list prepend, map element set/delete
    q.add_tag(AddTagParams {
        tags: BTreeSet::from(["vip".to_string(), "beta".to_string()]),
        customer_id: ada,
    })
    .await?;
    q.remove_tag(RemoveTagParams {
        tags: BTreeSet::from(["beta".to_string()]),
        customer_id: ada,
    })
    .await?;
    assert_eq!(
        q.get_customer(ada).await?.unwrap().tags,
        Some(BTreeSet::from(["new".to_string(), "vip".to_string()]))
    );
    q.prepend_phone(PrependPhoneParams {
        phones: vec!["+49".to_string()],
        customer_id: ada,
    })
    .await?;
    assert_eq!(
        q.get_customer(ada).await?.unwrap().phones,
        Some(vec!["+49".to_string(), "none".to_string()])
    );
    q.set_address(SetAddressParams {
        addresses_key: "home".into(),
        addresses_value: address("Paris"),
        customer_id: ada,
    })
    .await?;
    assert_eq!(
        q.get_customer(ada).await?.unwrap().addresses,
        Some(BTreeMap::from([("home".to_string(), address("Paris"))]))
    );
    q.clear_address(ClearAddressParams {
        addresses_key: "home".into(),
        customer_id: ada,
    })
    .await?;
    assert_eq!(q.get_customer(ada).await?.unwrap().addresses, None);

    // CONTAINS (+ ALLOW FILTERING), IN ?, IN as stream
    let by_tag = q.customers_by_tag("vip".into()).await?;
    assert_eq!(
        by_tag.iter().map(|r| r.customer_id).collect::<Vec<_>>(),
        [ada]
    );
    let by_ids = q.customers_by_ids(vec![ada, bob, grace]).await?;
    assert_eq!(by_ids.len(), 3);
    let streamed: Vec<_> = q
        .stream_customers_by_ids(vec![ada, bob, grace])
        .await?
        .try_collect()
        .await?;
    assert_eq!(streamed.len(), 3);

    // token(...) > token(?)
    let scanned = q
        .scan_by_token(ScanByTokenParams {
            customer_id: ada,
            limit: 100,
        })
        .await?;
    assert!(scanned.len() <= 3);
    assert!(q.count_customers().await?.unwrap().total >= 3);

    // DELETE ... collections and USING TTL
    q.clear_collections(ada).await?;
    let c = q.get_customer(ada).await?.unwrap();
    assert_eq!((c.tags, c.phones), (None, None));
    q.rename_customer(RenameCustomerParams {
        ttl: 100,
        name: Some("Ada L.".into()),
        customer_id: ada,
    })
    .await?;
    let ttl = q
        .customer_meta(ada)
        .await?
        .unwrap()
        .name_ttl
        .expect("ttl is set");
    assert!((1..=100).contains(&ttl));

    // LWT: IF EXISTS
    assert!(q.delete_customer_if_exists(grace).await?.applied);
    assert!(!q.delete_customer_if_exists(grace).await?.applied);
    assert!(q.get_customer(grace).await?.is_none());
    println!("customers: ok");
    Ok(())
}

async fn orders(q: &Queries) -> Res {
    let cust = Uuid::new_v4();
    for cents in [1250, 300, 9900] {
        q.place_order(PlaceOrderParams {
            customer_id: cust,
            total: Some(dec(cents, 2)),
        })
        .await?;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // minTimeuuid/maxTimeuuid, toTimestamp(column), ORDER BY, LIMIT ?
    let now = Utc::now();
    let params = OrdersSinceParams {
        customer_id: cust,
        order_time_gt: now - Duration::hours(1),
        order_time_lt: now + Duration::hours(1),
        limit: 10,
    };
    let rows = q.orders_since(params.clone()).await?;
    assert_eq!(rows.len(), 3);
    assert!(
        rows.windows(2).all(|w| w[0].placed_at >= w[1].placed_at),
        "newest first"
    );
    assert!(rows.iter().all(|r| r.status.as_deref() == Some("new")));
    let totals: Vec<f64> = rows
        .iter()
        .map(|r| dec_f64(r.total.as_ref().unwrap()))
        .collect();
    assert_eq!(totals, [99.0, 3.0, 12.5]);
    let limited = q
        .orders_since(OrdersSinceParams {
            limit: 2,
            ..params.clone()
        })
        .await?;
    assert_eq!(limited.len(), 2);
    let streamed: Vec<_> = q.stream_orders_since(params).await?.try_collect().await?;
    assert_eq!(streamed.len(), 3);

    // Tuple relation: (order_time, order_id) < (?, ?) pages through the clustering order.
    let newest = &rows[0];
    let page = q
        .orders_page(OrdersPageParams {
            customer_id: cust,
            order_time: newest.order_time,
            order_id: newest.order_id,
            limit: 10,
        })
        .await?;
    assert_eq!(page.len(), 2);

    // Aggregates
    assert_eq!(q.count_orders(cust).await?.unwrap().n, 3);
    let t = q.totals(cust).await?.unwrap();
    assert_eq!(dec_f64(&t.lowest.unwrap()), 3.0);
    assert_eq!(dec_f64(&t.highest.unwrap()), 99.0);
    assert_eq!(dec_f64(&t.sum_total), 114.5);

    // DISTINCT, PER PARTITION LIMIT, GROUP BY
    let customers: Vec<_> = q.customers_with_orders().await?;
    assert!(customers.iter().any(|r| r.customer_id == cust));
    let latest = q.latest_order_per_customer().await?;
    assert_eq!(latest.iter().filter(|r| r.customer_id == cust).count(), 1);
    let per = q.orders_per_customer().await?;
    assert_eq!(per.iter().find(|r| r.customer_id == cust).unwrap().n, 3);

    // LWT with IF <column> = ?
    let ship = |status: &str| ShipOrderParams {
        customer_id: cust,
        order_time: newest.order_time,
        order_id: newest.order_id,
        status: status.into(),
    };
    assert!(q.ship_order(ship("new")).await?.applied);
    let again = q.ship_order(ship("new")).await?;
    assert!(!again.applied);
    assert_eq!(again.status.as_deref(), Some("shipped"));
    println!("orders: ok");
    Ok(())
}

async fn misc(q: &Queries) -> Res {
    // Counters
    let product = Uuid::new_v4();
    q.count_view(CountViewParams {
        views: Counter(5),
        product_id: product,
    })
    .await?;
    q.count_view(CountViewParams {
        views: Counter(3),
        product_id: product,
    })
    .await?;
    assert_eq!(
        q.view_count(product).await?.unwrap().views,
        Some(Counter(8))
    );

    // Time series with a composite partition key
    let day = NaiveDate::from_ymd_opt(2024, 5, 1).unwrap();
    let t0 = Utc.with_ymd_and_hms(2024, 5, 1, 0, 0, 0).unwrap();
    for (h, v) in [(0, 1.0), (1, 2.0), (2, 6.0), (3, 3.0)] {
        q.add_reading(AddReadingParams {
            sensor_id: "s1".into(),
            day,
            ts: t0 + Duration::hours(h),
            value: Some(v),
        })
        .await?;
    }
    let window = q
        .readings_of_day(ReadingsOfDayParams {
            sensor_id: "s1".into(),
            day,
            ts_gte: t0 + Duration::hours(1),
            ts_lt: t0 + Duration::hours(3),
        })
        .await?;
    assert_eq!(
        window.iter().map(|r| r.value.unwrap()).collect::<Vec<_>>(),
        [2.0, 6.0]
    );
    let stats = q
        .reading_stats(ReadingStatsParams {
            sensor_id: "s1".into(),
            day,
        })
        .await?
        .unwrap();
    assert_eq!((stats.n, stats.lo, stats.hi), (4, Some(1.0), Some(6.0)));
    assert_eq!(stats.mean, 3.0);

    // LWT with a range condition
    let reserve = |stock: i32| ReserveStockParams {
        reserved: Some(2),
        sku: "sku-1".into(),
        stock,
    };
    assert!(q.reserve_stock(reserve(5)).await?.applied);
    let denied = q.reserve_stock(reserve(50)).await?;
    assert!(!denied.applied);
    assert_eq!(denied.stock, Some(10));

    // INSERT ... USING TTL ?
    let customer = Uuid::new_v4();
    q.create_session(CreateSessionParams {
        session_token: "tok".into(),
        customer_id: Some(customer),
        ttl: 60,
    })
    .await?;
    assert_eq!(
        q.session_customer("tok".into()).await?.unwrap().customer_id,
        Some(customer)
    );
    println!("misc: ok");
    Ok(())
}
