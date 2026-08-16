//! AST → (SQL fragment, binds). The structure of every statement comes from
//! this module; every value a client supplied travels as a bind.

pub mod geo;
pub mod q;
pub mod qprefilter;
pub mod scope;
pub mod temporal;
