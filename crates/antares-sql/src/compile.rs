//! AST → (SQL fragment, binds). The structure of every statement comes from
//! this module; every value a client supplied travels as a bind (§16.2).

pub mod geo;
pub mod q;
pub mod scope;
pub mod temporal;
