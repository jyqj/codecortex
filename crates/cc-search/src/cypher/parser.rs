use super::ast::*;
use cc_model::{CcError, CcResult};

const DEFAULT_VARLEN_MAX_HOPS: usize = 5;

// ── Parser ─────────────────────────────────────────

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> CcResult<Token> {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            Ok(tok)
        } else {
            Err(CcError::Search("unexpected end of query".into()))
        }
    }

    fn expect(&mut self, expected: &Token) -> CcResult<Token> {
        let tok = self.advance()?;
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            Ok(tok)
        } else {
            Err(CcError::Search(format!(
                "expected {:?}, got {:?}",
                expected, tok
            )))
        }
    }

    fn at(&self, expected: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(expected)
    }

    // ── Top-level parse ────────────────────────────

    fn parse(mut self) -> CcResult<CypherQuery> {
        // First MATCH clause (required).  May be preceded by OPTIONAL.
        let mut match_clauses = Vec::new();

        let is_optional = if self.at(&Token::Optional) {
            self.advance()?;
            true
        } else {
            false
        };
        self.expect(&Token::Match)?;
        let mut mc = self.parse_match()?;
        mc.is_optional = is_optional;
        match_clauses.push(mc);

        // Additional MATCH / OPTIONAL MATCH clauses.
        loop {
            let opt = if self.at(&Token::Optional) {
                self.advance()?;
                if !self.at(&Token::Match) {
                    return Err(CcError::Search("expected MATCH after OPTIONAL".into()));
                }
                true
            } else {
                false
            };
            if self.at(&Token::Match) {
                self.advance()?;
                let mut mc = self.parse_match()?;
                mc.is_optional = opt;
                match_clauses.push(mc);
            } else {
                break;
            }
        }

        // Optional WHERE.
        let where_clause = if self.at(&Token::Where) {
            self.advance()?;
            Some(WhereClause {
                expr: self.parse_expr()?,
            })
        } else {
            None
        };

        // RETURN clause (required).
        self.expect(&Token::Return)?;
        let return_clause = self.parse_return()?;

        // Optional ORDER BY.
        let order_by = if self.at(&Token::OrderBy) {
            self.advance()?;
            Some(self.parse_order_by_items(&return_clause)?)
        } else {
            None
        };

        // Optional LIMIT.
        let limit = if self.at(&Token::Limit) {
            self.advance()?;
            match self.advance()? {
                Token::IntLit(n) => Some(n as usize),
                other => {
                    return Err(CcError::Search(format!(
                        "expected integer after LIMIT, got {:?}",
                        other
                    )))
                }
            }
        } else {
            None
        };

        Ok(CypherQuery {
            match_clauses,
            where_clause,
            return_clause,
            order_by,
            limit,
        })
    }

    /// Parse ORDER BY items, supporting both var.prop references and aliases
    /// defined in the RETURN clause.
    fn parse_order_by_items(&mut self, return_clause: &ReturnClause) -> CcResult<Vec<OrderItem>> {
        // Collect known aliases from RETURN items.
        let aliases: std::collections::HashSet<String> = return_clause
            .items
            .iter()
            .filter_map(|item| match item {
                ReturnItem::Prop(_, Some(a)) => Some(a.clone()),
                ReturnItem::Count(_, _, Some(a)) => Some(a.clone()),
                ReturnItem::Aggregate(_, _, _, Some(a)) => Some(a.clone()),
                ReturnItem::Collect(_, _, Some(a)) => Some(a.clone()),
                ReturnItem::Var(_, Some(a)) => Some(a.clone()),
                _ => None,
            })
            .collect();

        let mut items = Vec::new();
        loop {
            // Try to parse as prop_ref (var.prop).  If the next token is an
            // Ident and there is NO dot after it, treat it as an alias reference.
            let expr = if let Token::Ident(ref name) = self.peek().clone() {
                // Lookahead: is the token after the ident a Dot?
                let has_dot = self.tokens.get(self.pos + 1) == Some(&Token::Dot);
                if has_dot {
                    OrderExpr::Prop(self.parse_prop_ref()?)
                } else if aliases.contains(name) {
                    let alias = name.clone();
                    self.advance()?;
                    OrderExpr::Alias(alias)
                } else {
                    // Fall back to trying a prop_ref (will likely error if no dot).
                    OrderExpr::Prop(self.parse_prop_ref()?)
                }
            } else {
                OrderExpr::Prop(self.parse_prop_ref()?)
            };

            let desc = if self.at(&Token::Desc) {
                self.advance()?;
                true
            } else if self.at(&Token::Asc) {
                self.advance()?;
                false
            } else {
                false
            };
            items.push(OrderItem { expr, desc });
            if self.at(&Token::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }
        Ok(items)
    }

    // ── MATCH clause ───────────────────────────────

    fn parse_match(&mut self) -> CcResult<MatchClause> {
        let mut patterns = Vec::new();
        patterns.push(self.parse_path_pattern()?);

        // Support comma-separated patterns.
        while self.at(&Token::Comma) {
            self.advance()?;
            patterns.push(self.parse_path_pattern()?);
        }

        Ok(MatchClause {
            is_optional: false, // caller sets this after parsing
            patterns,
        })
    }

    fn parse_path_pattern(&mut self) -> CcResult<PathPattern> {
        let mut nodes = Vec::new();
        let mut rels = Vec::new();

        // First node.
        nodes.push(self.parse_node_pattern()?);

        // Alternating: rel, node, rel, node...
        loop {
            if !self.at(&Token::Dash) && !self.at(&Token::Lt) {
                break;
            }
            let rel = self.parse_rel_pattern()?;
            rels.push(rel);
            nodes.push(self.parse_node_pattern()?);
        }

        Ok(PathPattern { nodes, rels })
    }

    fn parse_node_pattern(&mut self) -> CcResult<NodePattern> {
        self.expect(&Token::LParen)?;
        let mut np = NodePattern {
            var: None,
            label: None,
            props: Vec::new(),
        };

        // Optional variable name.
        if let Token::Ident(_) = self.peek() {
            if let Token::Ident(name) = self.advance()? {
                np.var = Some(name);
            }
        }

        // Optional :Label.
        if self.at(&Token::Colon) {
            self.advance()?;
            if let Token::Ident(label) = self.advance()? {
                np.label = Some(label);
            } else {
                return Err(CcError::Search("expected label after ':'".into()));
            }
        }

        // Optional inline properties {key: 'value', ...}.
        if self.at(&Token::LBrace) {
            self.advance()?;
            loop {
                if self.at(&Token::RBrace) {
                    break;
                }
                let key = match self.advance()? {
                    Token::Ident(k) => k,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected property name, got {:?}",
                            other
                        )))
                    }
                };
                self.expect(&Token::Colon)?;
                let val = match self.advance()? {
                    Token::StringLit(s) => s,
                    Token::Ident(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected property value, got {:?}",
                            other
                        )))
                    }
                };
                np.props.push((key, val));
                if self.at(&Token::Comma) {
                    self.advance()?;
                }
            }
            self.expect(&Token::RBrace)?;
        }

        self.expect(&Token::RParen)?;
        Ok(np)
    }

    fn parse_rel_pattern(&mut self) -> CcResult<RelPattern> {
        let mut rp = RelPattern {
            var: None,
            rel_type: None,
            direction: RelDirection::Outgoing,
            min_hops: 1,
            max_hops: 1,
        };

        // Determine direction prefix.
        // Patterns: -[...]-> (outgoing), <-[...]- (incoming), -[...]- (both)
        // Also: -> (shorthand outgoing), <- (shorthand incoming)

        let incoming_start = if self.at(&Token::Lt) {
            // Could be <-[...]-  or  <-[...]->
            self.advance()?; // consume <
            self.expect(&Token::Dash)?; // consume -
            true
        } else {
            // Must be - (outgoing or both)
            self.expect(&Token::Dash)?;
            false
        };

        // Check for bracket body: [...]
        if self.at(&Token::LBracket) {
            self.advance()?;

            // Optional variable name.
            if let Token::Ident(_) = self.peek() {
                if let Token::Ident(name) = self.advance()? {
                    rp.var = Some(name);
                }
            }

            // Optional :TYPE.
            if self.at(&Token::Colon) {
                self.advance()?;
                if let Token::Ident(t) = self.advance()? {
                    rp.rel_type = Some(t.to_uppercase());
                } else {
                    return Err(CcError::Search(
                        "expected relationship type after ':'".into(),
                    ));
                }
            }

            // Optional variable-length syntax:
            //   *        => 1..DEFAULT_VARLEN_MAX_HOPS
            //   *N       => N..N
            //   *min..max
            //   *..max   => 1..max
            //   *min..   => min..max(min, DEFAULT_VARLEN_MAX_HOPS)
            if self.at(&Token::Star) {
                self.advance()?;
                match self.peek() {
                    Token::RBracket => {
                        rp.min_hops = 1;
                        rp.max_hops = DEFAULT_VARLEN_MAX_HOPS;
                    }
                    Token::DotDot => {
                        self.advance()?;
                        let max = match self.advance()? {
                            Token::IntLit(n) => n as usize,
                            other => {
                                return Err(CcError::Search(format!(
                                    "expected integer after '*..', got {:?}",
                                    other
                                )))
                            }
                        };
                        rp.min_hops = 1;
                        rp.max_hops = max;
                    }
                    Token::IntLit(_) => {
                        let min = match self.advance()? {
                            Token::IntLit(n) => n as usize,
                            _ => unreachable!(),
                        };
                        if self.at(&Token::DotDot) {
                            self.advance()?;
                            let max = if self.at(&Token::RBracket) {
                                min.max(DEFAULT_VARLEN_MAX_HOPS)
                            } else {
                                match self.advance()? {
                                    Token::IntLit(n) => n as usize,
                                    other => {
                                        return Err(CcError::Search(format!(
                                            "expected integer after '..', got {:?}",
                                            other
                                        )))
                                    }
                                }
                            };
                            rp.min_hops = min;
                            rp.max_hops = max;
                        } else {
                            rp.min_hops = min;
                            rp.max_hops = min;
                        }
                    }
                    other => {
                        return Err(CcError::Search(format!(
                            "expected integer, '..', or ']' after '*', got {:?}",
                            other
                        )))
                    }
                }
                if rp.min_hops == 0 || rp.max_hops < rp.min_hops {
                    return Err(CcError::Search(format!(
                        "invalid variable-length range: *{}..{}",
                        rp.min_hops, rp.max_hops
                    )));
                }
            }

            self.expect(&Token::RBracket)?;
        }

        // Determine direction suffix.
        // After the bracket: - (no arrow) or -> (outgoing end)
        if self.at(&Token::Dash) {
            self.advance()?;
            if self.at(&Token::Gt) {
                self.advance()?;
                // Suffix is ->
                if incoming_start {
                    rp.direction = RelDirection::Both; // <-[...]->
                } else {
                    rp.direction = RelDirection::Outgoing; // -[...]->
                }
            } else {
                // Suffix is just -
                if incoming_start {
                    rp.direction = RelDirection::Incoming; // <-[...]-
                } else {
                    rp.direction = RelDirection::Both; // -[...]-
                }
            }
        } else if self.at(&Token::Arrow) {
            // Token::Arrow is ->
            self.advance()?;
            if incoming_start {
                rp.direction = RelDirection::Both;
            } else {
                rp.direction = RelDirection::Outgoing;
            }
        } else if self.at(&Token::Gt) {
            // After ] we might see > as separate token if -> was split
            self.advance()?;
            if incoming_start {
                rp.direction = RelDirection::Both;
            } else {
                rp.direction = RelDirection::Outgoing;
            }
        } else {
            // No trailing arrow means direction decided by prefix.
            if incoming_start {
                rp.direction = RelDirection::Incoming;
            } else {
                rp.direction = RelDirection::Both;
            }
        }

        Ok(rp)
    }

    // ── WHERE expression parsing ───────────────────

    /// Parse an expression with correct operator precedence: OR binds loosest, AND tighter.
    fn parse_expr(&mut self) -> CcResult<Expr> {
        let mut left = self.parse_and_expr()?;

        while self.at(&Token::Or) {
            self.advance()?;
            let right = self.parse_and_expr()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    /// Parse AND-level expressions (higher precedence than OR).
    fn parse_and_expr(&mut self) -> CcResult<Expr> {
        let mut left = self.parse_expr_atom()?;

        while self.at(&Token::And) {
            self.advance()?;
            let right = self.parse_expr_atom()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_expr_atom(&mut self) -> CcResult<Expr> {
        if self.at(&Token::Not) {
            self.advance()?;
            let inner = self.parse_expr_atom()?;
            return Ok(Expr::Not(Box::new(inner)));
        }

        if self.at(&Token::LParen) {
            self.advance()?;
            let inner = self.parse_expr()?;
            self.expect(&Token::RParen)?;
            return Ok(inner);
        }

        // Check for degree function calls: degree(var), in_degree(var), out_degree(var)
        if let Token::Ident(ref name) = self.peek().clone() {
            let degree_kind = match name.to_lowercase().as_str() {
                "degree" => Some(DegreeKind::Total),
                "in_degree" => Some(DegreeKind::In),
                "out_degree" => Some(DegreeKind::Out),
                _ => None,
            };
            if let Some(kind) = degree_kind {
                // Lookahead: must be followed by '('
                if self.tokens.get(self.pos + 1) == Some(&Token::LParen) {
                    self.advance()?; // consume function name
                    self.expect(&Token::LParen)?;
                    let var = match self.advance()? {
                        Token::Ident(s) => s,
                        other => {
                            return Err(CcError::Search(format!(
                                "expected variable name in degree(), got {:?}",
                                other
                            )))
                        }
                    };
                    self.expect(&Token::RParen)?;
                    let op = self.parse_cmp_op()?;
                    let value = self.parse_value()?;
                    return Ok(Expr::Degree {
                        var,
                        kind,
                        op,
                        value,
                    });
                }
            }
        }

        // Must be a comparison: var.prop OP value
        let left = self.parse_prop_ref()?;

        match self.peek().clone() {
            Token::Eq => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Eq,
                    right,
                })
            }
            Token::Neq => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Neq,
                    right,
                })
            }
            Token::Lt => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Lt,
                    right,
                })
            }
            Token::Gt => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Gt,
                    right,
                })
            }
            Token::Lte => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Lte,
                    right,
                })
            }
            Token::Gte => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Gte,
                    right,
                })
            }
            Token::RegexMatch => {
                self.advance()?;
                let pattern = match self.advance()? {
                    Token::StringLit(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected string after '=~', got {:?}",
                            other
                        )))
                    }
                };
                Ok(Expr::Regex { left, pattern })
            }
            Token::Contains => {
                self.advance()?;
                let value = match self.advance()? {
                    Token::StringLit(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected string after CONTAINS, got {:?}",
                            other
                        )))
                    }
                };
                Ok(Expr::Contains { left, value })
            }
            Token::StartsWith => {
                self.advance()?;
                let value = match self.advance()? {
                    Token::StringLit(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected string after STARTS WITH, got {:?}",
                            other
                        )))
                    }
                };
                Ok(Expr::StartsWith { left, value })
            }
            Token::EndsWith => {
                self.advance()?;
                let value = match self.advance()? {
                    Token::StringLit(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected string after ENDS WITH, got {:?}",
                            other
                        )))
                    }
                };
                Ok(Expr::EndsWith { left, value })
            }
            other => Err(CcError::Search(format!(
                "expected operator after property reference, got {:?}",
                other
            ))),
        }
    }

    fn parse_prop_ref(&mut self) -> CcResult<PropRef> {
        let var = match self.advance()? {
            Token::Ident(s) => s,
            other => {
                return Err(CcError::Search(format!(
                    "expected identifier, got {:?}",
                    other
                )))
            }
        };
        self.expect(&Token::Dot)?;
        let prop = match self.advance()? {
            Token::Ident(s) => s,
            other => {
                return Err(CcError::Search(format!(
                    "expected property name, got {:?}",
                    other
                )))
            }
        };
        Ok(PropRef { var, prop })
    }

    fn parse_value(&mut self) -> CcResult<Value> {
        match self.advance()? {
            Token::StringLit(s) => Ok(Value::String(s)),
            Token::IntLit(n) => Ok(Value::Int(n)),
            Token::FloatLit(f) => Ok(Value::Float(f)),
            Token::Ident(s) if s.eq_ignore_ascii_case("true") => Ok(Value::Bool(true)),
            Token::Ident(s) if s.eq_ignore_ascii_case("false") => Ok(Value::Bool(false)),
            Token::Ident(s) if s.eq_ignore_ascii_case("null") => Ok(Value::Null),
            other => Err(CcError::Search(format!("expected value, got {:?}", other))),
        }
    }

    fn parse_cmp_op(&mut self) -> CcResult<CmpOp> {
        match self.advance()? {
            Token::Eq => Ok(CmpOp::Eq),
            Token::Neq => Ok(CmpOp::Neq),
            Token::Lt => Ok(CmpOp::Lt),
            Token::Gt => Ok(CmpOp::Gt),
            Token::Lte => Ok(CmpOp::Lte),
            Token::Gte => Ok(CmpOp::Gte),
            other => Err(CcError::Search(format!(
                "expected comparison operator, got {:?}",
                other
            ))),
        }
    }

    // ── RETURN clause ──────────────────────────────

    fn parse_return(&mut self) -> CcResult<ReturnClause> {
        // Check for RETURN DISTINCT ...
        let distinct = if self.at(&Token::Distinct) {
            self.advance()?;
            true
        } else {
            false
        };

        let mut items = Vec::new();

        loop {
            let item = self.parse_return_item()?;
            items.push(item);
            if self.at(&Token::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }

        Ok(ReturnClause { distinct, items })
    }

    fn parse_return_item(&mut self) -> CcResult<ReturnItem> {
        // COUNT([DISTINCT] arg)
        if self.at(&Token::Count) {
            self.advance()?;
            self.expect(&Token::LParen)?;
            let distinct = if self.at(&Token::Distinct) {
                self.advance()?;
                true
            } else {
                false
            };
            // arg: * | var | var.prop
            let count_arg = match self.advance()? {
                Token::Star => CountArg::Star,
                Token::Ident(s) => {
                    if self.at(&Token::Dot) {
                        self.advance()?;
                        let prop = match self.advance()? {
                            Token::Ident(p) => p,
                            other => {
                                return Err(CcError::Search(format!(
                                    "expected property name in COUNT(var.prop), got {:?}",
                                    other
                                )))
                            }
                        };
                        CountArg::Prop(PropRef { var: s, prop })
                    } else {
                        CountArg::Var(s)
                    }
                }
                other => {
                    return Err(CcError::Search(format!(
                        "expected identifier or * in COUNT(), got {:?}",
                        other
                    )))
                }
            };
            self.expect(&Token::RParen)?;
            let alias = self.parse_optional_alias()?;
            return Ok(ReturnItem::Count(count_arg, distinct, alias));
        }

        // SUM/AVG/MIN/MAX([DISTINCT] var.prop)
        if self.at(&Token::Sum)
            || self.at(&Token::Avg)
            || self.at(&Token::Min)
            || self.at(&Token::Max)
        {
            let func_name = match self.peek() {
                Token::Sum => "SUM",
                Token::Avg => "AVG",
                Token::Min => "MIN",
                Token::Max => "MAX",
                _ => unreachable!(),
            };
            self.advance()?;
            self.expect(&Token::LParen)?;
            let distinct = if self.at(&Token::Distinct) {
                self.advance()?;
                true
            } else {
                false
            };
            let prop_ref = self.parse_prop_ref()?;
            self.expect(&Token::RParen)?;
            let alias = self.parse_optional_alias()?;
            return Ok(ReturnItem::Aggregate(
                func_name.to_string(),
                prop_ref,
                distinct,
                alias,
            ));
        }

        // COLLECT([DISTINCT] var.prop) or COLLECT([DISTINCT] var)
        if self.at(&Token::Collect) {
            self.advance()?;
            self.expect(&Token::LParen)?;
            let distinct = if self.at(&Token::Distinct) {
                self.advance()?;
                true
            } else {
                false
            };
            let ident = match self.advance()? {
                Token::Ident(s) => s,
                other => {
                    return Err(CcError::Search(format!(
                        "expected identifier in COLLECT(), got {:?}",
                        other
                    )))
                }
            };
            let expr = if self.at(&Token::Dot) {
                self.advance()?;
                let prop = match self.advance()? {
                    Token::Ident(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected property name in COLLECT(var.prop), got {:?}",
                            other
                        )))
                    }
                };
                CollectExpr::Prop(PropRef { var: ident, prop })
            } else {
                CollectExpr::Var(ident)
            };
            self.expect(&Token::RParen)?;
            let alias = self.parse_optional_alias()?;
            return Ok(ReturnItem::Collect(expr, distinct, alias));
        }

        // var.prop or bare var
        let ident = match self.advance()? {
            Token::Ident(s) => s,
            other => {
                return Err(CcError::Search(format!(
                    "expected identifier in RETURN, got {:?}",
                    other
                )))
            }
        };

        if self.at(&Token::Dot) {
            self.advance()?;
            let prop = match self.advance()? {
                Token::Ident(s) => s,
                other => {
                    return Err(CcError::Search(format!(
                        "expected property name after '.', got {:?}",
                        other
                    )))
                }
            };
            let alias = self.parse_optional_alias()?;
            Ok(ReturnItem::Prop(PropRef { var: ident, prop }, alias))
        } else {
            let alias = self.parse_optional_alias()?;
            Ok(ReturnItem::Var(ident, alias))
        }
    }

    fn parse_optional_alias(&mut self) -> CcResult<Option<String>> {
        if self.at(&Token::As) {
            self.advance()?;
            match self.advance()? {
                Token::Ident(s) => Ok(Some(s)),
                other => Err(CcError::Search(format!(
                    "expected alias name after AS, got {:?}",
                    other
                ))),
            }
        } else {
            Ok(None)
        }
    }
}

pub fn parse(tokens: &[Token]) -> CcResult<CypherQuery> {
    let parser = Parser::new(tokens.to_vec());
    parser.parse()
}
