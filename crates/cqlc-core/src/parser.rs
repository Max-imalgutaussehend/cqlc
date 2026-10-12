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

/// A parsed statement plus the information needed to match annotations.
#[derive(Debug)]
struct Parsed {
    statement: Statement,
    span: Span,
    cql: String,
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
                out.push(QueryDef {
                    name,
                    kind,
                    statement: p.statement,
                    cql: p.cql,
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
    let ok = match kind {
        QueryKind::One | QueryKind::Many => is_select,
        QueryKind::Exec => !is_select,
    };
    if ok {
        return Ok(());
    }
    let (msg, help) = if is_select {
        (
            "SELECT queries must be annotated `:one` or `:many`",
            "use `:one` for at most one row, `:many` for any number of rows",
        )
    } else {
        (
            "INSERT, UPDATE and DELETE queries must be annotated `:exec`",
            "change the annotation to `:exec`",
        )
    };
    Err(Diagnostic::error(DiagnosticCode::Annotation, p.span, msg).with_help(help))
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
        .with_help("add `-- name: MyQuery :one|:many|:exec` on the line before the statement")
    };
    let c = candidate.ok_or_else(missing)?;
    let body = c.text.trim().trim_start_matches("name:").trim();
    let mut parts = body.split_whitespace();
    let bad = |msg: &str| {
        Diagnostic::error(DiagnosticCode::Annotation, c.span, msg)
            .with_help("expected `-- name: MyQuery :one|:many|:exec`")
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
        other => return Err(bad(&format!("unknown result kind `{other}`"))),
    };
    Ok((
        Ident {
            name: name.to_string(),
            span: c.span,
        },
        kind,
    ))
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    markers: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, tokens: Vec<Token>) -> Self {
        Parser {
            src,
            tokens,
            pos: 0,
            markers: 0,
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
        let name = match &t.kind {
            TokenKind::Ident(s) => s.to_ascii_lowercase(),
            TokenKind::QuotedIdent(s) => s.clone(),
            _ => return Err(self.expected("an identifier")),
        };
        self.bump();
        Ok(Ident { name, span: t.span })
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
        for kw in ["JSON", "DISTINCT"] {
            if self.at_kw(kw) {
                let s = self.peek().span;
                return Err(self.unsupported(s, &format!("`SELECT {kw}`")));
            }
        }
        let selection = if self.at_star() {
            Selection::Star(self.bump().span)
        } else {
            let mut items = Vec::new();
            loop {
                if self.peek().kind == TokenKind::Bind {
                    return Err(Diagnostic::error(
                        DiagnosticCode::UntypedBindMarker,
                        self.peek().span,
                        "bind marker in the selection list has no inferable type",
                    )
                    .with_help(
                        "bind markers are only allowed in WHERE, VALUES, SET, LIMIT and USING",
                    ));
                }
                let column = self.ident()?;
                if matches!(self.peek().kind, TokenKind::LParen) {
                    return Err(self.unsupported(
                        column.span.to(self.peek().span),
                        &format!("function call `{}(...)` in the selection list", column.name),
                    ));
                }
                if matches!(self.peek().kind, TokenKind::Dot | TokenKind::LBracket) {
                    let s = self.peek().span;
                    return Err(self.unsupported(s, "field or element selection"));
                }
                let alias = if self.eat_kw("AS") {
                    Some(self.ident()?)
                } else {
                    None
                };
                items.push(SelectItem { column, alias });
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
        for kw in ["GROUP", "PER"] {
            if self.at_kw(kw) {
                let s = self.peek().span;
                return Err(self.unsupported(s, &format!("`{kw}` clause")));
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
        let limit = if self.eat_kw("LIMIT") {
            Some(self.parse_term()?)
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
            selection,
            table,
            where_clause,
            order_by,
            limit,
            allow_filtering,
            span: start.to(end),
        })
    }

    fn at_star(&self) -> bool {
        self.peek().kind == TokenKind::Star
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

    fn parse_relation(&mut self) -> PResult<Relation> {
        if self.peek().kind == TokenKind::LParen {
            let s = self.peek().span;
            return Err(self.unsupported(s, "tuple relations"));
        }
        let column = self.ident()?;
        if matches!(self.peek().kind, TokenKind::LParen) {
            return Err(self.unsupported(
                column.span,
                &format!("function call `{}(...)` in a relation", column.name),
            ));
        }
        let op_tok = self.peek().clone();
        let op = match &op_tok.kind {
            TokenKind::Eq => RelOp::Eq,
            TokenKind::Lt => RelOp::Lt,
            TokenKind::Le => RelOp::Le,
            TokenKind::Gt => RelOp::Gt,
            TokenKind::Ge => RelOp::Ge,
            TokenKind::Ne => return Err(self.unsupported(op_tok.span, "operator `!=`")),
            TokenKind::Ident(s) if s.eq_ignore_ascii_case("IN") => RelOp::In,
            TokenKind::Ident(s)
                if s.eq_ignore_ascii_case("CONTAINS") || s.eq_ignore_ascii_case("LIKE") =>
            {
                return Err(self.unsupported(
                    op_tok.span,
                    &format!("operator `{}`", s.to_ascii_uppercase()),
                ));
            }
            TokenKind::LBracket => {
                return Err(self.unsupported(op_tok.span, "element access in a relation"));
            }
            _ => return Err(self.expected("a comparison operator")),
        };
        self.bump();
        let rhs = if op == RelOp::In && self.peek().kind == TokenKind::LParen {
            self.bump();
            let mut terms = Vec::new();
            if self.peek().kind != TokenKind::RParen {
                loop {
                    terms.push(self.parse_term()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen)?;
            Rhs::List(terms)
        } else {
            Rhs::Term(self.parse_term()?)
        };
        let end = self.tokens[self.pos - 1].span;
        Ok(Relation {
            span: column.span.to(end),
            column,
            op,
            rhs,
        })
    }

    fn parse_term(&mut self) -> PResult<Term> {
        let t = self.peek().clone();
        let kind = match &t.kind {
            TokenKind::Bind => {
                let i = self.markers;
                self.markers += 1;
                self.bump();
                TermKind::Marker(i)
            }
            TokenKind::String(s) => {
                self.bump();
                TermKind::Literal(Literal::String(s.clone()))
            }
            TokenKind::Number(n) => {
                self.bump();
                TermKind::Literal(number_literal(n, false))
            }
            TokenKind::Minus if matches!(self.peek_n(1), TokenKind::Number(_)) => {
                self.bump();
                let TokenKind::Number(n) = self.bump().kind else {
                    unreachable!()
                };
                TermKind::Literal(number_literal(&n, true))
            }
            TokenKind::Uuid(u) => {
                self.bump();
                TermKind::Literal(Literal::Uuid(u.clone()))
            }
            TokenKind::Blob(b) => {
                self.bump();
                TermKind::Literal(Literal::Blob(b.clone()))
            }
            TokenKind::Ident(s) if s.eq_ignore_ascii_case("true") => {
                self.bump();
                TermKind::Literal(Literal::Bool(true))
            }
            TokenKind::Ident(s) if s.eq_ignore_ascii_case("false") => {
                self.bump();
                TermKind::Literal(Literal::Bool(false))
            }
            TokenKind::Ident(s) if s.eq_ignore_ascii_case("null") => {
                self.bump();
                TermKind::Literal(Literal::Null)
            }
            TokenKind::Colon => {
                return Err(self.unsupported(t.span, "named bind markers (use `?`)"));
            }
            TokenKind::Ident(_)
            | TokenKind::QuotedIdent(_)
            | TokenKind::LParen
            | TokenKind::LBrace
            | TokenKind::LBracket => {
                return Err(self.unsupported(
                    t.span,
                    "this expression (only `?` and plain literals are supported)",
                ));
            }
            _ => return Err(self.expected("a value or `?`")),
        };
        let end = self.tokens[self.pos - 1].span;
        Ok(Term {
            kind,
            span: t.span.to(end),
        })
    }

    fn parse_using(&mut self) -> PResult<Using> {
        let mut using = Using::default();
        if !self.eat_kw("USING") {
            return Ok(using);
        }
        loop {
            if self.eat_kw("TTL") {
                using.ttl = Some(self.parse_term()?);
            } else if self.eat_kw("TIMESTAMP") {
                using.timestamp = Some(self.parse_term()?);
            } else {
                return Err(self.expected("`TTL` or `TIMESTAMP`"));
            }
            if !self.eat_kw("AND") {
                break;
            }
        }
        Ok(using)
    }

    fn reject_conditions(&mut self) -> PResult<()> {
        if self.at_kw("IF") {
            let s = self.peek().span;
            return Err(self.unsupported(s, "lightweight transactions (`IF ...`)"));
        }
        Ok(())
    }

    fn parse_insert(&mut self) -> PResult<Insert> {
        let start = self.expect_kw("INSERT")?;
        self.expect_kw("INTO")?;
        let table = self.table_name()?;
        if self.at_kw("JSON") {
            let s = self.peek().span;
            return Err(self.unsupported(s, "`INSERT ... JSON`"));
        }
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
        let mut values = Vec::new();
        loop {
            values.push(self.parse_term()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen)?;
        self.reject_conditions_or_if_not_exists()?;
        let using = self.parse_using()?;
        let end = self.tokens[self.pos - 1].span;
        Ok(Insert {
            table,
            columns,
            values,
            using,
            span: start.to(end),
        })
    }

    fn reject_conditions_or_if_not_exists(&mut self) -> PResult<()> {
        if self.at_kw("IF") {
            let s = self.peek().span;
            return Err(self.unsupported(s, "lightweight transactions (`IF NOT EXISTS`)"));
        }
        Ok(())
    }

    fn parse_update(&mut self) -> PResult<Update> {
        let start = self.expect_kw("UPDATE")?;
        let table = self.table_name()?;
        let using = self.parse_using()?;
        self.expect_kw("SET")?;
        let mut assignments = Vec::new();
        loop {
            let column = self.ident()?;
            match self.peek().kind {
                TokenKind::LBracket | TokenKind::Dot => {
                    let s = self.peek().span;
                    return Err(self.unsupported(s, "element or field assignment"));
                }
                TokenKind::Plus | TokenKind::Minus => {
                    let s = self.peek().span;
                    return Err(self.unsupported(s, "collection and counter updates (`+=`, `-=`)"));
                }
                _ => {}
            }
            self.expect(&TokenKind::Eq)?;
            let value = self.parse_term()?;
            assignments.push(Assignment { column, value });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect_kw("WHERE")?;
        let where_clause = self.parse_relations()?;
        self.reject_conditions()?;
        let end = self.tokens[self.pos - 1].span;
        Ok(Update {
            table,
            using,
            assignments,
            where_clause,
            span: start.to(end),
        })
    }

    fn parse_delete(&mut self) -> PResult<Delete> {
        let start = self.expect_kw("DELETE")?;
        let mut columns = Vec::new();
        if !self.at_kw("FROM") {
            loop {
                columns.push(self.ident()?);
                if matches!(self.peek().kind, TokenKind::LBracket | TokenKind::Dot) {
                    let s = self.peek().span;
                    return Err(self.unsupported(s, "element or field deletion"));
                }
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
        self.reject_conditions()?;
        let end = self.tokens[self.pos - 1].span;
        Ok(Delete {
            columns,
            table,
            using,
            where_clause,
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
