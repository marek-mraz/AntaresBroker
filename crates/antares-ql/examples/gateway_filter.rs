// SPDX-License-Identifier: EUPL-1.2
//! A gateway in front of the broker rewrites an incoming `q=` with the
//! broker's own query engine: parse, AND in an authorization predicate,
//! render the result as the query string to forward — and show the SQL the
//! broker would run for it.
//!
//!     cargo run -p antares-ql --example gateway_filter -- 'speed>25|heading<90'

use antares_ql::{parse_q, sql::compile_q, CmpOp, QNode, QPath, QValue};

fn main() {
    let incoming = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "speed>25|heading<90".to_owned());
    let node = match parse_q(&incoming) {
        Ok(n) => n,
        Err(e) => {
            // the same error the broker would answer with (Table 6.3.2-1)
            eprintln!("refused: {} {}", e.status(), e.to_problem_details().detail);
            std::process::exit(1);
        }
    };
    // authz: the caller may only see entities of its own tenant-owner
    let restriction = QNode::Cmp {
        path: QPath::dotted(vec!["owner".into()]),
        op: CmpOp::Eq,
        value: QValue::Str("urn:ngsi-ld:Org:tenant-a".into()),
    };
    let rewritten = QNode::And(vec![restriction, node]);
    println!("forward q={rewritten}");
    println!(
        "ast   = {}",
        serde_json::to_string(&rewritten).expect("the AST serializes")
    );
    // the broker lowers the same tree to one bind-parameter jsonpath per
    // leaf (nothing a client typed reaches SQL text); a shape outside the
    // exact subset yields None and is filtered in memory instead
    let expand = |term: &str| format!("https://example.org/vocab/{term}");
    match compile_q(&rewritten, "entity", 1, &expand) {
        Some(c) => println!("sql   = {}\nbinds = {:?}", c.sql, c.binds),
        None => println!("sql   = (outside the pushdown subset: in-memory filter)"),
    }
}
