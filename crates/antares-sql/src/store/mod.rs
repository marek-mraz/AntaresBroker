//! The storage seam: one folder per backend (`mem/`, `pg/`), `AnyStore`
//! as the dispatcher between them, and the backend-neutral filter types.

pub mod any;
pub mod mem;
#[cfg(feature = "postgres")]
pub mod pg;
pub use antares_store::filter;
pub use antares_store::{ChangeHook, Kind};
pub use mem::{Store, MAX_CACHED_CONTEXTS};
