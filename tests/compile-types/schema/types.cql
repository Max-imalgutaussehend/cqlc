CREATE TABLE all_types (
    id uuid,
    seq int,
    a_ascii ascii,
    a_bigint bigint,
    a_blob blob,
    a_boolean boolean,
    a_date date,
    a_decimal decimal,
    a_double double,
    a_duration duration,
    a_float float,
    a_inet inet,
    a_smallint smallint,
    a_text text,
    a_time time,
    a_timestamp timestamp,
    a_timeuuid timeuuid,
    a_tinyint tinyint,
    a_varint varint,
    a_list list<int>,
    a_set set<uuid>,
    a_map map<text, double>,
    a_tuple tuple<int, text>,
    a_frozen frozen<list<text>>,
    "type" text,
    PRIMARY KEY ((id), seq)
);

CREATE TABLE counters (
    id uuid PRIMARY KEY,
    hits counter
);
