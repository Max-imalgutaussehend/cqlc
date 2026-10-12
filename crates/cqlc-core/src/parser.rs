//! Recursive-descent parser for the supported CQL subset.
//!
//! Anything outside the subset is rejected with `CQL002` instead of being
//! interpreted loosely.

use crate::ast::*;
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::lexer::{Comment, Token, TokenKind, lex};
use crate::span::{FileId, Span};
use crate::types::CqlType;

type PResult<T> = Result<T, Diagnostic>;

/// How deeply expressions and types may nest. Real queries stay far below this;
/// the limit keeps crafted input from overflowing the stack.
const MAX_DEPTH: usize = 64;

/// A parsed statement plus the information needed to match annotations.
#[derive(Debug)]
struct Parsed {
    statement: Statement,
    span: Span,
    cql: String,
    marker_count: usize,
    /// Byte offset just after the previous statement terminator.
    lo: u32,
}

/// Parses a schema file. DML statements are rejected.
pub fn parse_schema(file: FileId, src: &str) -> (Vec<Statement>, Vec<Diagnostic>) {
    let (lexed, mut diags) = lex(file, src);
    let (parsed, mut pd) = Parser::new(src, lexed.tokens).parse_all();
    diags.append(&mut pd);
    let mut out = Vec::new();
    for p in parsed {
        if p.statement.is_ddl() {
            out.push(p.statement);
        } else {
            diags.push(
                Diagnostic::error(
                    DiagnosticCode::Syntax,
                    p.span,
                    "only CREATE statements are allowed in schema files",
                )
                .with_help("move this statement to a file in the queries directory"),
            );
        }
    }
    (out, diags)
}

/// Parses a queries file: every statement needs a `-- name: Foo :kind`
/// annotation directly before it.
pub fn parse_queries(file: FileId, src: &str) -> (Vec<QueryDef>, Vec<Diagnostic>) {
    let (lexed, mut diags) = lex(file, src);
    let comments = lexed.comments;
    let (parsed, mut pd) = Parser::new(src, lexed.tokens).parse_all();
    diags.append(&mut pd);
    let mut out = Vec::new();
    for p in parsed {
        if p.statement.is_ddl() {
            diags.push(
                Diagnostic::error(
                    DiagnosticCode::Syntax,
                    p.span,
                    "DDL statements are not allowed in query files",
                )
                .with_help("move this statement to a file in the schema directory"),
            );
            continue;
        }
        match annotation(&comments, &p) {
            Ok((name, kind)) => {
                if let Err(d) = check_kind(kind, &p) {
                    diags.push(d);
                    continue;
                }
                let allow = allow_list(&comments, &p, &mut diags);
                out.push(QueryDef {
                    name,
                    kind,
                    statement: p.statement,
                    cql: p.cql,
                    marker_count: p.marker_count,
                    allow,
                    span: p.span,
                });
            }
            Err(d) => diags.push(d),
        }
    }
    (out, diags)
}

fn check_kind(kind: QueryKind, p: &Parsed) -> PResult<()> {
    let is_select = matches!(p.statement, Statement::Select(_));
    let conditional = p.statement.is_conditional();
    let (ok, msg, help) = if is_select {
        (
            matches!(kind, QueryKind::One | QueryKind::Many),
            "SELECT queries must be annotated `:one` or `:many`",
            "use `:one` for at most one row, `:many` for any number of rows",
        )
    } else if conditional {
        (
            kind == QueryKind::Lwt,
            "conditional writes (`IF ...`) must be annotated `:lwt`",
            "the generated function returns whether the write was applied",
        )
    } else {
        (
            kind == QueryKind::Exec,
            "INSERT, UPDATE and DELETE queries without `IF` must be annotated `:exec`",
            "change the annotation to `:exec` (`:lwt` is only for statements with `IF`)",
        )
    };
    if ok {
        Ok(())
    } else {
        Err(Diagnostic::error(DiagnosticCode::Annotation, p.span, msg).with_help(help))
    }
}

/// `-- allow: CQL201, CQL202` before a statement silences those warnings.
fn allow_list(
    comments: &[Comment],
    p: &Parsed,
    diags: &mut Vec<Diagnostic>,
) -> Vec<DiagnosticCode> {
    let mut out = Vec::new();
    for c in comments
        .iter()
        .filter(|c| c.span.end <= p.span.start && c.span.start >= p.lo)
    {
        let Some(list) = c.text.trim().strip_prefix("allow:") else {
            continue;
        };
        for item in list.split([',', ' ']).filter(|i| !i.is_empty()) {
            match DiagnosticCode::parse_warning(item) {
                Some(code) => out.push(code),
                None => diags.push(
                    Diagnostic::error(
                        DiagnosticCode::Annotation,
                        c.span,
                        format!("`{item}` is not a warning code that can be allowed"),
                    )
                    .with_help("allowed: CQL201, CQL202, CQL203, CQL204, CQL205"),
                ),
            }
        }
    }
    out
}

fn annotation(comments: &[Comment], p: &Parsed) -> PResult<(Ident, QueryKind)> {
    let candidate = comments.iter().rev().find(|c| {
        c.span.end <= p.span.start && c.span.start >= p.lo && c.text.trim().starts_with("name:")
    });
    let missing = || {
        Diagnostic::error(
            DiagnosticCode::Annotation,
            p.span,
            "query has no `-- name:` annotation",
        )
        .with_help("add `-- name: MyQuery :one|:many|:exec|:lwt` on the line before the statement")
    };
    let c = candidate.ok_or_else(missing)?;
    let body = c.text.trim().trim_start_matches("name:").trim();
    let mut parts = body.split_whitespace();
    let bad = |msg: &str| {
        Diagnostic::error(DiagnosticCode::Annotation, c.span, msg)
            .with_help("expected `-- name: MyQuery :one|:many|:exec|:lwt`")
    };
    let name = parts
        .next()
        .ok_or_else(|| bad("annotation is missing a query name"))?;
    let kind = parts
        .next()
        .ok_or_else(|| bad("annotation is missing a result kind"))?;
    if parts.next().is_some() {
        return Err(bad("annotation has trailing text"));
    }
    let valid = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err(bad(&format!("`{name}` is not a valid query name")));
    }
    let kind = match kind {
        ":one" => QueryKind::One,
        ":many" => QueryKind::Many,
        ":exec" => QueryKind::Exec,
        ":lwt" => QueryKind::Lwt,
        other => return Err(bad(&format!("unknown result kind `{other}`"))),
    };
    Ok((
        Ident {
            name: name.to_string(),
            span: c.span,
            quoted: false,
        },
        kind,
    ))
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    markers: usize,
    /// Current nesting depth of expressions and types.
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, tokens: Vec<Token>) -> Self {
        Parser {
            src,
            tokens,
            pos: 0,
            markers: 0,
            depth: 0,
        }
    }

    fn parse_all(mut self) -> (Vec<Parsed>, Vec<Diagnostic>) {
        let mut out = Vec::new();
        let mut diags = Vec::new();
        let mut lo = 0u32;
        loop {
            while self.eat(&TokenKind::Semi) {
                lo = self.prev_end();
            }
            if self.at_eof() {
                break;
            }
            self.markers = 0;
            let start = self.pos;
            let first = self.peek().span;
            match self.parse_statement() {
                Ok(statement) => {
                    let end_tok = self.pos;
                    let span = first.to(self.tokens[end_tok - 1].span);
                    let terminated = self.eat(&TokenKind::Semi);
                    if !terminated && !self.at_eof() {
                        diags.push(self.expected("`;`"));
                        self.recover();
                    }
                    out.push(Parsed {
                        statement,
                        span,
                        cql: self.normalized(start, end_tok),
                        marker_count: self.markers,
                        lo,
                    });
                }
                Err(d) => {
                    diags.push(d);
                    self.recover();
                }
            }
            lo = self.prev_end();
        }
        (out, diags)
    }

    /// Skip to just after the next `;`.
    fn recover(&mut self) {
        while !self.at_eof() {
            if self.bump().kind == TokenKind::Semi {
                break;
            }
        }
    }

    fn prev_end(&self) -> u32 {
        if self.pos == 0 {
            0
        } else {
            self.tokens[self.pos - 1].span.end
        }
    }

    /// Token texts joined by a single space where the source had any gap.
    fn normalized(&self, from: usize, to: usize) -> String {
        let mut s = String::new();
        for i in from..to {
            let t = &self.tokens[i];
            if i > from && self.tokens[i - 1].span.end < t.span.start {
                s.push(' ');
            }
            s.push_str(&self.src[t.span.range()]);
        }
        s
    }

    // ----- token helpers -------------------------------------------------

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_n(&self, n: usize) -> &TokenKind {
        let i = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }

    fn at_eof(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if &self.peek().kind == kind {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> PResult<Span> {
        if &self.peek().kind == kind {
            Ok(self.bump().span)
        } else {
            Err(self.expected(&kind.describe()))
        }
    }

    fn at_kw(&self, kw: &str) -> bool {
        self.kw_at(0, kw)
    }

    fn kw_at(&self, n: usize, kw: &str) -> bool {
        matches!(self.peek_n(n), TokenKind::Ident(s) if s.eq_ignore_ascii_case(kw))
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.at_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_kw(&mut self, kw: &str) -> PResult<Span> {
        if self.at_kw(kw) {
            Ok(self.bump().span)
        } else {
            Err(self.expected(&format!("`{}`", kw.to_ascii_uppercase())))
        }
    }

    fn expected(&self, what: &str) -> Diagnostic {
        let t = self.peek();
        Diagnostic::error(
            DiagnosticCode::Syntax,
            t.span,
            format!("expected {what}, found {}", t.kind.describe()),
        )
    }

    fn unsupported(&self, span: Span, what: &str) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::Unsupported,
            span,
            format!("{what} is not supported by cqlc yet"),
        )
        .with_help("see docs/SUPPORTED_CQL.md for the supported subset")
    }

    fn ident(&mut self) -> PResult<Ident> {
        let t = self.peek().clone();
        let (name, quoted) = match &t.kind {
            TokenKind::Ident(s) => (s.to_ascii_lowercase(), false),
            TokenKind::QuotedIdent(s) => (s.clone(), true),
            _ => return Err(self.expected("an identifier")),
        };
        self.bump();
        Ok(Ident {
            name,
            span: t.span,
            quoted,
        })
    }

    fn table_name(&mut self) -> PResult<TableName> {
        let first = self.ident()?;
        if self.eat(&TokenKind::Dot) {
            let name = self.ident()?;
            Ok(TableName {
                keyspace: Some(first),
                name,
            })
        } else {
            Ok(TableName {
                keyspace: None,
                name: first,
            })
        }
    }

    // ----- statements ----------------------------------------------------

    fn parse_statement(&mut self) -> PResult<Statement> {
        let t = self.peek().clone();
        let TokenKind::Ident(word) = &t.kind else {
            return Err(self.expected("a statement"));
        };
        match word.to_ascii_uppercase().as_str() {
            "CREATE" => self.parse_create(),
            "SELECT" => self.parse_select().map(Statement::Select),
            "INSERT" => self.parse_insert().map(Statement::Insert),
            "UPDATE" => self.parse_update().map(Statement::Update),
            "DELETE" => self.parse_delete().map(Statement::Delete),
            _ => Err(self.unsupported(t.span, &format!("statement `{word}`"))),
        }
    }

    fn parse_create(&mut self) -> PResult<Statement> {
        let start = self.expect_kw("CREATE")?;
        if self.at_kw("TABLE") {
            self.bump();
            self.parse_create_table(start).map(Statement::CreateTable)
        } else if self.at_kw("TYPE") {
            self.bump();
            self.parse_create_type(start).map(Statement::CreateType)
        } else if self.at_kw("KEYSPACE") {
            self.bump();
            self.eat_if_not_exists()?;
            let name = self.ident()?;
            // Replication options do not influence types; skip them.
            while !matches!(self.peek().kind, TokenKind::Semi | TokenKind::Eof) {
                self.bump();
            }
            Ok(Statement::CreateKeyspace(CreateKeyspace { name }))
        } else {
            let t = self.peek().clone();
            Err(self.unsupported(
                t.span,
                &format!("`CREATE {}`", t.kind.describe().trim_matches('`')),
            ))
        }
    }

    fn eat_if_not_exists(&mut self) -> PResult<()> {
        if self.eat_kw("IF") {
            self.expect_kw("NOT")?;
            self.expect_kw("EXISTS")?;
        }
        Ok(())
    }

    fn parse_create_table(&mut self, start: Span) -> PResult<CreateTable> {
        self.eat_if_not_exists()?;
        let name = self.table_name()?;
        self.expect(&TokenKind::LParen)?;
        let mut columns = Vec::new();
        let mut primary_key = None;
        loop {
            if self.at_kw("PRIMARY") && self.kw_at(1, "KEY") {
                let pk_start = self.bump().span;
                self.bump();
                if primary_key.is_some() {
                    return Err(Diagnostic::error(
                        DiagnosticCode::PrimaryKey,
                        pk_start,
                        "table has more than one PRIMARY KEY clause",
                    ));
                }
                primary_key = Some(self.parse_primary_key(pk_start)?);
            } else {
                columns.push(self.parse_column_def()?);
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        let end = self.expect(&TokenKind::RParen)?;
        let mut clustering_order = Vec::new();
        if self.eat_kw("WITH") {
            loop {
                if self.at_kw("CLUSTERING") {
                    self.bump();
                    self.expect_kw("ORDER")?;
                    self.expect_kw("BY")?;
                    self.expect(&TokenKind::LParen)?;
                    loop {
                        let col = self.ident()?;
                        let order = self.parse_order()?.unwrap_or(Order::Asc);
                        clustering_order.push((col, order));
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                } else {
                    self.skip_table_option()?;
                }
                if !self.eat_kw("AND") {
                    break;
                }
            }
        }
        Ok(CreateTable {
            name,
            columns,
            primary_key,
            clustering_order,
            span: start.to(end),
        })
    }

    /// Skips one `WITH` option (e.g. `comment = '...'`) up to `AND`/`;`.
    fn skip_table_option(&mut self) -> PResult<()> {
        let mut depth = 0usize;
        let mut consumed = false;
        loop {
            match &self.peek().kind {
                TokenKind::Eof | TokenKind::Semi => break,
                TokenKind::Ident(s) if depth == 0 && s.eq_ignore_ascii_case("AND") => break,
                TokenKind::LParen | TokenKind::LBrace | TokenKind::LBracket => depth += 1,
                TokenKind::RParen | TokenKind::RBrace | TokenKind::RBracket => {
                    depth = depth.saturating_sub(1)
                }
                _ => {}
            }
            consumed = true;
            self.bump();
        }
        if consumed {
            Ok(())
        } else {
            Err(self.expected("a table option"))
        }
    }

    fn parse_order(&mut self) -> PResult<Option<Order>> {
        if self.eat_kw("ASC") {
            Ok(Some(Order::Asc))
        } else if self.eat_kw("DESC") {
            Ok(Some(Order::Desc))
        } else {
            Ok(None)
        }
    }

    fn parse_primary_key(&mut self, start: Span) -> PResult<PrimaryKeyDef> {
        self.expect(&TokenKind::LParen)?;
        let mut partition = Vec::new();
        if self.eat(&TokenKind::LParen) {
            loop {
                partition.push(self.ident()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::RParen)?;
        } else {
            partition.push(self.ident()?);
        }
        let mut clustering = Vec::new();
        while self.eat(&TokenKind::Comma) {
            clustering.push(self.ident()?);
        }
        let end = self.expect(&TokenKind::RParen)?;
        Ok(PrimaryKeyDef {
            partition,
            clustering,
            span: start.to(end),
        })
    }

    fn parse_column_def(&mut self) -> PResult<ColumnDef> {
        let name = self.ident()?;
        let ty_start = self.peek().span;
        let ty = self.parse_type()?;
        let ty_span = ty_start.to(self.tokens[self.pos - 1].span);
        let mut is_static = false;
        let mut inline_primary_key = false;
        loop {
            if self.at_kw("STATIC") {
                self.bump();
                is_static = true;
            } else if self.at_kw("PRIMARY") && self.kw_at(1, "KEY") {
                self.bump();
                self.bump();
                inline_primary_key = true;
            } else {
                break;
            }
        }
        Ok(ColumnDef {
            name,
            ty,
            ty_span,
            is_static,
            inline_primary_key,
        })
    }

    fn parse_type(&mut self) -> PResult<CqlType> {
        self.nested("type", Self::parse_type_inner)
    }

    fn parse_type_inner(&mut self) -> PResult<CqlType> {
        let id = self.ident()?;
        let generic = |p: &mut Self| -> PResult<()> { p.expect(&TokenKind::Lt).map(|_| ()) };
        let close = |p: &mut Self| -> PResult<()> { p.expect(&TokenKind::Gt).map(|_| ()) };
        let ty = match id.name.as_str() {
            "list" => {
                generic(self)?;
                let t = self.parse_type()?;
                close(self)?;
                CqlType::List(Box::new(t))
            }
            "set" => {
                generic(self)?;
                let t = self.parse_type()?;
                close(self)?;
                CqlType::Set(Box::new(t))
            }
            "frozen" => {
                generic(self)?;
                let t = self.parse_type()?;
                close(self)?;
                CqlType::Frozen(Box::new(t))
            }
            "map" => {
                generic(self)?;
                let k = self.parse_type()?;
                self.expect(&TokenKind::Comma)?;
                let v = self.parse_type()?;
                close(self)?;
                CqlType::Map(Box::new(k), Box::new(v))
            }
            "tuple" => {
                generic(self)?;
                let mut ts = vec![self.parse_type()?];
                while self.eat(&TokenKind::Comma) {
                    ts.push(self.parse_type()?);
                }
                close(self)?;
                CqlType::Tuple(ts)
            }
            other => match CqlType::from_primitive(other) {
                Some(t) => t,
                None => {
                    // `ks.udt` -> keep the type name only.
                    let mut name = id.name.clone();
                    if self.eat(&TokenKind::Dot) {
                        name = self.ident()?.name;
                    }
                    CqlType::Udt(name)
                }
            },
        };
        Ok(ty)
    }

    fn parse_create_type(&mut self, start: Span) -> PResult<CreateType> {
        self.eat_if_not_exists()?;
        let name = self.table_name()?;
        self.expect(&TokenKind::LParen)?;
        let mut fields = Vec::new();
        loop {
            let f = self.ident()?;
            let t = self.parse_type()?;
            fields.push((f, t));
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        let end = self.expect(&TokenKind::RParen)?;
        Ok(CreateType {
            name,
            fields,
            span: start.to(end),
        })
    }

    // ----- DML -----------------------------------------------------------

    fn parse_select(&mut self) -> PResult<Select> {
        let start = self.expect_kw("SELECT")?;
        let mode = if self.eat_kw("JSON") {
            SelectMode::Json
        } else if self.eat_kw("DISTINCT") {
            SelectMode::Distinct
        } else {
            SelectMode::Normal
        };
        let selection = if self.peek().kind == TokenKind::Star {
            Selection::Star(self.bump().span)
        } else {
            let mut items = Vec::new();
            loop {
                let expr = self.parse_expr()?;
                let alias = if self.eat_kw("AS") {
                    Some(self.ident()?)
                } else {
                    None
                };
                items.push(SelectItem { expr, alias });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            Selection::Items(items)
        };
        self.expect_kw("FROM")?;
        let table = self.table_name()?;
        let where_clause = if self.eat_kw("WHERE") {
            self.parse_relations()?
        } else {
            Vec::new()
        };
        let mut group_by = Vec::new();
        if self.eat_kw("GROUP") {
            self.expect_kw("BY")?;
            loop {
                group_by.push(self.ident()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        let mut order_by = Vec::new();
        if self.eat_kw("ORDER") {
            self.expect_kw("BY")?;
            loop {
                let col = self.ident()?;
                let order = self.parse_order()?.unwrap_or(Order::Asc);
                order_by.push((col, order));
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        let per_partition_limit = if self.eat_kw("PER") {
            self.expect_kw("PARTITION")?;
            self.expect_kw("LIMIT")?;
            Some(self.parse_expr()?)
        } else {
            None
        };
        let limit = if self.eat_kw("LIMIT") {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let mut allow_filtering = false;
        if self.eat_kw("ALLOW") {
            self.expect_kw("FILTERING")?;
            allow_filtering = true;
        }
        let end = self.tokens[self.pos - 1].span;
        Ok(Select {
            mode,
            selection,
            table,
            where_clause,
            group_by,
            order_by,
            per_partition_limit,
            limit,
            allow_filtering,
            span: start.to(end),
        })
    }

    fn parse_relations(&mut self) -> PResult<Vec<Relation>> {
        let mut rels = Vec::new();
        loop {
            rels.push(self.parse_relation()?);
            if !self.eat_kw("AND") {
                break;
            }
        }
        Ok(rels)
    }

    fn parse_ident_list_in_parens(&mut self) -> PResult<Vec<Ident>> {
        self.expect(&TokenKind::LParen)?;
        let mut ids = vec![self.ident()?];
        while self.eat(&TokenKind::Comma) {
            ids.push(self.ident()?);
        }
        self.expect(&TokenKind::RParen)?;
        Ok(ids)
    }

    fn parse_relation(&mut self) -> PResult<Relation> {
        let start = self.peek().span;
        let lhs = if self.peek().kind == TokenKind::LParen {
            RelLhs::Tuple(self.parse_ident_list_in_parens()?)
        } else if self.at_kw("TOKEN") && *self.peek_n(1) == TokenKind::LParen {
            self.bump();
            RelLhs::Token(self.parse_ident_list_in_parens()?)
        } else {
            let column = self.ident()?;
            match self.peek().kind {
                TokenKind::LParen => {
                    return Err(self.unsupported(
                        column.span,
                        &format!(
                            "function call `{}(...)` on the left side of a relation",
                            column.name
                        ),
                    ));
                }
                TokenKind::LBracket => {
                    self.bump();
                    let key = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket)?;
                    RelLhs::Element(column, key)
                }
                TokenKind::Dot => {
                    self.bump();
                    RelLhs::Field(column, self.ident()?)
                }
                _ => RelLhs::Column(column),
            }
        };
        let op_tok = self.peek().clone();
        let op = match &op_tok.kind {
            TokenKind::Eq => RelOp::Eq,
            TokenKind::Ne => RelOp::Ne,
            TokenKind::Lt => RelOp::Lt,
            TokenKind::Le => RelOp::Le,
            TokenKind::Gt => RelOp::Gt,
            TokenKind::Ge => RelOp::Ge,
            TokenKind::Ident(s) if s.eq_ignore_ascii_case("IN") => RelOp::In,
            TokenKind::Ident(s) if s.eq_ignore_ascii_case("CONTAINS") => {
                if matches!(self.peek_n(1), TokenKind::Ident(k) if k.eq_ignore_ascii_case("KEY")) {
                    self.bump();
                    RelOp::ContainsKey
                } else {
                    RelOp::Contains
                }
            }
            TokenKind::Ident(s) if s.eq_ignore_ascii_case("LIKE") => {
                return Err(self.unsupported(op_tok.span, "operator `LIKE`"));
            }
            _ => return Err(self.expected("a comparison operator")),
        };
        self.bump();
        let rhs = if op == RelOp::In && self.peek().kind == TokenKind::LParen {
            self.bump();
            let mut items = Vec::new();
            if self.peek().kind != TokenKind::RParen {
                loop {
                    items.push(self.parse_expr()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen)?;
            Rhs::InList(items)
        } else {
            Rhs::Expr(self.parse_expr()?)
        };
        let end = self.tokens[self.pos - 1].span;
        Ok(Relation {
            lhs,
            op,
            rhs,
            span: start.to(end),
        })
    }

    // ----- expressions ---------------------------------------------------

    /// Runs `f` one nesting level deeper, failing instead of overflowing the stack.
    fn nested<T>(&mut self, what: &str, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        if self.depth >= MAX_DEPTH {
            return Err(Diagnostic::error(
                DiagnosticCode::Syntax,
                self.peek().span,
                format!("{what} is nested too deeply (limit {MAX_DEPTH})"),
            ));
        }
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        result
    }

    fn parse_expr(&mut self) -> PResult<Expr> {
        self.nested("expression", Self::parse_expr_inner)
    }

    fn parse_expr_inner(&mut self) -> PResult<Expr> {
        let mut e = self.parse_primary()?;
        loop {
            match self.peek().kind {
                TokenKind::Dot => {
                    self.bump();
                    let f = self.ident()?;
                    let span = e.span.to(f.span);
                    e = Expr {
                        kind: ExprKind::Field(Box::new(e), f),
                        span,
                    };
                }
                TokenKind::LBracket => {
                    self.bump();
                    let idx = self.parse_expr()?;
                    let end = self.expect(&TokenKind::RBracket)?;
                    let span = e.span.to(end);
                    e = Expr {
                        kind: ExprKind::Index(Box::new(e), Box::new(idx)),
                        span,
                    };
                }
                _ => return Ok(e),
            }
        }
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let t = self.peek().clone();
        let lit = |p: &mut Self, l: Literal| {
            p.bump();
            Ok(Expr {
                kind: ExprKind::Literal(l),
                span: t.span,
            })
        };
        match &t.kind {
            TokenKind::Bind => {
                let i = self.markers;
                self.markers += 1;
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Marker(i),
                    span: t.span,
                })
            }
            TokenKind::String(s) => lit(self, Literal::String(s.clone())),
            TokenKind::Number(n) => lit(self, number_literal(n, false)),
            TokenKind::Uuid(u) => lit(self, Literal::Uuid(u.clone())),
            TokenKind::Blob(b) => lit(self, Literal::Blob(b.clone())),
            TokenKind::Minus if matches!(self.peek_n(1), TokenKind::Number(_)) => {
                self.bump();
                let n = self.bump();
                let TokenKind::Number(n_text) = n.kind else {
                    unreachable!()
                };
                Ok(Expr {
                    kind: ExprKind::Literal(number_literal(&n_text, true)),
                    span: t.span.to(n.span),
                })
            }
            TokenKind::Colon => Err(self.unsupported(t.span, "named bind markers (use `?`)")),
            TokenKind::LBracket => {
                self.bump();
                let items = self.parse_expr_list(&TokenKind::RBracket)?;
                let end = self.expect(&TokenKind::RBracket)?;
                Ok(Expr {
                    kind: ExprKind::List(items),
                    span: t.span.to(end),
                })
            }
            TokenKind::LParen => {
                self.bump();
                let mut items = vec![self.parse_expr()?];
                let mut tuple = false;
                while self.eat(&TokenKind::Comma) {
                    tuple = true;
                    items.push(self.parse_expr()?);
                }
                let end = self.expect(&TokenKind::RParen)?;
                if tuple {
                    Ok(Expr {
                        kind: ExprKind::Tuple(items),
                        span: t.span.to(end),
                    })
                } else {
                    Ok(items.remove(0))
                }
            }
            TokenKind::LBrace => self.parse_braces(),
            TokenKind::Ident(word) if word.eq_ignore_ascii_case("true") => {
                lit(self, Literal::Bool(true))
            }
            TokenKind::Ident(word) if word.eq_ignore_ascii_case("false") => {
                lit(self, Literal::Bool(false))
            }
            TokenKind::Ident(word) if word.eq_ignore_ascii_case("null") => lit(self, Literal::Null),
            TokenKind::Ident(_) | TokenKind::QuotedIdent(_) => {
                let name = self.ident()?;
                if self.peek().kind == TokenKind::LParen {
                    self.parse_call(name)
                } else {
                    Ok(Expr {
                        span: name.span,
                        kind: ExprKind::Column(name),
                    })
                }
            }
            _ => Err(self.expected("an expression")),
        }
    }

    fn parse_expr_list(&mut self, close: &TokenKind) -> PResult<Vec<Expr>> {
        let mut items = Vec::new();
        if &self.peek().kind != close {
            loop {
                items.push(self.parse_expr()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        Ok(items)
    }

    fn parse_call(&mut self, name: Ident) -> PResult<Expr> {
        self.expect(&TokenKind::LParen)?;
        if name.name == "cast" {
            let inner = self.parse_expr()?;
            self.expect_kw("AS")?;
            let ty = self.parse_type()?;
            let end = self.expect(&TokenKind::RParen)?;
            return Ok(Expr {
                span: name.span.to(end),
                kind: ExprKind::Cast(Box::new(inner), ty),
            });
        }
        let mut star = false;
        let args = if self.peek().kind == TokenKind::Star {
            self.bump();
            star = true;
            Vec::new()
        } else {
            self.parse_expr_list(&TokenKind::RParen)?
        };
        let end = self.expect(&TokenKind::RParen)?;
        Ok(Expr {
            span: name.span.to(end),
            kind: ExprKind::Call { name, args, star },
        })
    }

    /// `{}`, `{a, b}`, `{k: v}` or `{field: v}`.
    fn parse_braces(&mut self) -> PResult<Expr> {
        let open = self.expect(&TokenKind::LBrace)?;
        if self.peek().kind == TokenKind::RBrace {
            let end = self.bump().span;
            return Ok(Expr {
                kind: ExprKind::EmptyBraces,
                span: open.to(end),
            });
        }
        let udt = matches!(
            self.peek().kind,
            TokenKind::Ident(_) | TokenKind::QuotedIdent(_)
        ) && *self.peek_n(1) == TokenKind::Colon;
        if udt {
            let mut fields = Vec::new();
            loop {
                let f = self.ident()?;
                self.expect(&TokenKind::Colon)?;
                fields.push((f, self.parse_expr()?));
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            let end = self.expect(&TokenKind::RBrace)?;
            return Ok(Expr {
                kind: ExprKind::Udt(fields),
                span: open.to(end),
            });
        }
        let first = self.parse_expr()?;
        if self.eat(&TokenKind::Colon) {
            let mut pairs = vec![(first, self.parse_expr()?)];
            while self.eat(&TokenKind::Comma) {
                let k = self.parse_expr()?;
                self.expect(&TokenKind::Colon)?;
                pairs.push((k, self.parse_expr()?));
            }
            let end = self.expect(&TokenKind::RBrace)?;
            return Ok(Expr {
                kind: ExprKind::Map(pairs),
                span: open.to(end),
            });
        }
        let mut items = vec![first];
        while self.eat(&TokenKind::Comma) {
            items.push(self.parse_expr()?);
        }
        let end = self.expect(&TokenKind::RBrace)?;
        Ok(Expr {
            kind: ExprKind::Set(items),
            span: open.to(end),
        })
    }

    // ----- writes --------------------------------------------------------

    fn parse_using(&mut self) -> PResult<Using> {
        let mut using = Using::default();
        if !self.eat_kw("USING") {
            return Ok(using);
        }
        loop {
            if self.eat_kw("TTL") {
                using.ttl = Some(self.parse_expr()?);
            } else if self.eat_kw("TIMESTAMP") {
                using.timestamp = Some(self.parse_expr()?);
            } else {
                return Err(self.expected("`TTL` or `TIMESTAMP`"));
            }
            if !self.eat_kw("AND") {
                break;
            }
        }
        Ok(using)
    }

    fn parse_if(&mut self) -> PResult<Option<IfClause>> {
        if !self.eat_kw("IF") {
            return Ok(None);
        }
        if self.eat_kw("EXISTS") {
            return Ok(Some(IfClause::Exists));
        }
        if self.at_kw("NOT") {
            let s = self.peek().span;
            return Err(
                self.unsupported(s, "`IF NOT EXISTS` on UPDATE/DELETE (only INSERT has it)")
            );
        }
        Ok(Some(IfClause::Conditions(self.parse_relations()?)))
    }

    fn parse_target(&mut self) -> PResult<Target> {
        let column = self.ident()?;
        let mut key = None;
        let mut field = None;
        if self.eat(&TokenKind::LBracket) {
            key = Some(self.parse_expr()?);
            self.expect(&TokenKind::RBracket)?;
        } else if self.eat(&TokenKind::Dot) {
            field = Some(self.ident()?);
        }
        Ok(Target { column, key, field })
    }

    fn parse_insert(&mut self) -> PResult<Insert> {
        let start = self.expect_kw("INSERT")?;
        self.expect_kw("INTO")?;
        let table = self.table_name()?;
        let source = if self.eat_kw("JSON") {
            let value = self.parse_expr()?;
            if self.at_kw("DEFAULT") {
                let s = self.peek().span;
                return Err(self.unsupported(s, "`DEFAULT NULL|UNSET` on `INSERT ... JSON`"));
            }
            InsertSource::Json(value)
        } else {
            self.expect(&TokenKind::LParen)?;
            let mut columns = Vec::new();
            loop {
                columns.push(self.ident()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::RParen)?;
            self.expect_kw("VALUES")?;
            self.expect(&TokenKind::LParen)?;
            let values = self.parse_expr_list(&TokenKind::RParen)?;
            self.expect(&TokenKind::RParen)?;
            InsertSource::Values { columns, values }
        };
        let mut if_not_exists = false;
        if self.eat_kw("IF") {
            self.expect_kw("NOT")?;
            self.expect_kw("EXISTS")?;
            if_not_exists = true;
        }
        let using = self.parse_using()?;
        let end = self.tokens[self.pos - 1].span;
        Ok(Insert {
            table,
            source,
            if_not_exists,
            using,
            span: start.to(end),
        })
    }

    fn parse_update(&mut self) -> PResult<Update> {
        let start = self.expect_kw("UPDATE")?;
        let table = self.table_name()?;
        let using = self.parse_using()?;
        self.expect_kw("SET")?;
        let mut assignments = Vec::new();
        loop {
            assignments.push(self.parse_assignment()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect_kw("WHERE")?;
        let where_clause = self.parse_relations()?;
        let if_clause = self.parse_if()?;
        let end = self.tokens[self.pos - 1].span;
        Ok(Update {
            table,
            using,
            assignments,
            where_clause,
            if_clause,
            span: start.to(end),
        })
    }

    fn parse_assignment(&mut self) -> PResult<Assignment> {
        let target = self.parse_target()?;
        // `col += v` / `col -= v`
        let compound = match self.peek().kind {
            TokenKind::Plus if *self.peek_n(1) == TokenKind::Eq => Some(AssignOp::Add),
            TokenKind::Minus if *self.peek_n(1) == TokenKind::Eq => Some(AssignOp::Sub),
            _ => None,
        };
        if let Some(op) = compound {
            self.bump();
            self.bump();
            let value = self.parse_expr()?;
            return Ok(Assignment { target, op, value });
        }
        self.expect(&TokenKind::Eq)?;
        let first = self.parse_expr()?;
        let arith = match self.peek().kind {
            TokenKind::Plus => Some(AssignOp::Add),
            TokenKind::Minus => Some(AssignOp::Sub),
            _ => None,
        };
        let Some(op) = arith else {
            return Ok(Assignment {
                target,
                op: AssignOp::Set,
                value: first,
            });
        };
        let op_span = self.bump().span;
        let second = self.parse_expr()?;
        let is_target = |e: &Expr| {
            matches!(&e.kind, ExprKind::Column(c) if c.name == target.column.name)
                && target.key.is_none()
                && target.field.is_none()
        };
        if is_target(&first) {
            Ok(Assignment {
                target,
                op,
                value: second,
            })
        } else if op == AssignOp::Add && is_target(&second) {
            Ok(Assignment {
                target,
                op: AssignOp::Prepend,
                value: first,
            })
        } else {
            Err(self.unsupported(
                op_span,
                "arithmetic in assignments other than `col = col + v`, `col = col - v` and `col = v + col`",
            ))
        }
    }

    fn parse_delete(&mut self) -> PResult<Delete> {
        let start = self.expect_kw("DELETE")?;
        let mut targets = Vec::new();
        if !self.at_kw("FROM") {
            loop {
                targets.push(self.parse_target()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect_kw("FROM")?;
        let table = self.table_name()?;
        let using = self.parse_using()?;
        self.expect_kw("WHERE")?;
        let where_clause = self.parse_relations()?;
        let if_clause = self.parse_if()?;
        let end = self.tokens[self.pos - 1].span;
        Ok(Delete {
            targets,
            table,
            using,
            where_clause,
            if_clause,
            span: start.to(end),
        })
    }
}

fn number_literal(n: &str, negative: bool) -> Literal {
    let text = if negative {
        format!("-{n}")
    } else {
        n.to_string()
    };
    if n.contains(['.', 'e', 'E']) {
        Literal::Float(text)
    } else {
        Literal::Integer(text)
    }
}
