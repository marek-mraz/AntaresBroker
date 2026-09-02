// SPDX-License-Identifier: EUPL-1.2
//! NGSI-LD error model: the Table 5.5.2-1 error-type vocabulary (variant
//! names verbatim) with the Table 6.3.2-1 HTTP status mapping. Error type
//! URI base is https (V1.9.1). errors/Conflict entered with 5.9.2.4
//! (registration-vs-entity/registration proxied-mode conflicts, 409).

use serde::Serialize;
use thiserror::Error;

/// Base of every NGSI-LD error type URI (Table 5.5.2-1, https in V1.9.1).
pub const ERROR_TYPE_BASE: &str = "https://uri.etsi.org/ngsi-ld/errors/";

/// NGSI-LD error types of Table 5.5.2-1; the payload is the `detail` text.
#[derive(Debug, Error)]
pub enum NgsiError {
    /// The referred element already exists (409).
    #[error("{0}")]
    AlreadyExists(String),
    /// The request or its content is incorrect (400).
    #[error("{0}")]
    BadRequestData(String),
    /// Registration-vs-entity or proxied-mode registration conflict (409).
    #[error("{0}")]
    Conflict(String),
    /// The request is not valid (400).
    #[error("{0}")]
    InvalidRequest(String),
    /// An unexpected internal error (500).
    #[error("{0}")]
    InternalError(String),
    /// A remote JSON-LD @context could not be retrieved (504).
    #[error("{0}")]
    LdContextNotAvailable(String),
    /// Multi-tenancy is not supported by this broker (501).
    #[error("{0}")]
    NoMultiTenantSupport(String),
    /// The tenant named in `NGSILD-Tenant` does not exist (404).
    #[error("{0}")]
    NonexistentTenant(String),
    /// The operation is not supported (422).
    #[error("{0}")]
    OperationNotSupported(String),
    /// The referred resource has not been found (404).
    #[error("{0}")]
    ResourceNotFound(String),
    /// The query is too complex to be processed (403).
    #[error("{0}")]
    TooComplexQuery(String),
    /// The query would return too many results (403).
    #[error("{0}")]
    TooManyResults(String),
}

/// The `InternalError` detail a storage driver sets when its connection
/// pool ran out of time handing over a connection. It is not a Table
/// 6.3.2-1 error type: the HTTP binding answers it 503 with `Retry-After`
/// (6.3.2 "implementations shall support the standard specific errors of
/// HTTP bindings, such as the following", an open list). Both the driver
/// that raises it and the binding that recognises it name this constant,
/// so the two ends cannot drift.
pub const DB_OVERLOADED: &str = "database overloaded";

impl NgsiError {
    /// HTTP status per Table 6.3.2-1.
    pub fn status(&self) -> u16 {
        match self {
            Self::AlreadyExists(_) | Self::Conflict(_) => 409,
            Self::BadRequestData(_) | Self::InvalidRequest(_) => 400,
            Self::InternalError(_) => 500,
            // 6.3.2 Table 6.3.2-1 (V1.9.1): LdContextNotAvailable → 504.
            // The suite's V1.8-era 503 expectations are fixed in the
            // local suite fork.
            Self::LdContextNotAvailable(_) => 504,
            Self::NoMultiTenantSupport(_) => 501,
            Self::NonexistentTenant(_) | Self::ResourceNotFound(_) => 404,
            Self::OperationNotSupported(_) => 422,
            Self::TooComplexQuery(_) | Self::TooManyResults(_) => 403,
        }
    }

    /// Spec error name == variant name.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::AlreadyExists(_) => "AlreadyExists",
            Self::BadRequestData(_) => "BadRequestData",
            Self::Conflict(_) => "Conflict",
            Self::InvalidRequest(_) => "InvalidRequest",
            Self::InternalError(_) => "InternalError",
            Self::LdContextNotAvailable(_) => "LdContextNotAvailable",
            Self::NoMultiTenantSupport(_) => "NoMultiTenantSupport",
            Self::NonexistentTenant(_) => "NonexistentTenant",
            Self::OperationNotSupported(_) => "OperationNotSupported",
            Self::ResourceNotFound(_) => "ResourceNotFound",
            Self::TooComplexQuery(_) => "TooComplexQuery",
            Self::TooManyResults(_) => "TooManyResults",
        }
    }

    /// Renders this error as the RFC 7807 body of 6.3.6.
    pub fn to_problem_details(&self) -> ProblemDetails {
        ProblemDetails {
            r#type: format!("{ERROR_TYPE_BASE}{}", self.kind()),
            title: self.kind().to_owned(),
            status: self.status(),
            detail: self.to_string(),
        }
    }
}

/// RFC 7807 body (always `application/json`, fully-qualified names — 6.3.6).
#[derive(Debug, Serialize)]
pub struct ProblemDetails {
    /// Error type URI: `ERROR_TYPE_BASE` + error name.
    pub r#type: String,
    /// Short summary — the error name.
    pub title: String,
    /// HTTP status per Table 6.3.2-1.
    pub status: u16,
    /// Human-readable explanation of this occurrence.
    pub detail: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 6.3.2 Table 6.3.2-1 — every row of the error-type → HTTP status
    /// mapping (V1.9.1, PDF p.269), plus the project's Conflict extension.
    #[test]
    fn status_mapping_matches_table_6_3_2_1() {
        assert_eq!(NgsiError::AlreadyExists(String::new()).status(), 409);
        assert_eq!(NgsiError::BadRequestData(String::new()).status(), 400);
        assert_eq!(NgsiError::InternalError(String::new()).status(), 500);
        assert_eq!(NgsiError::InvalidRequest(String::new()).status(), 400);
        assert_eq!(
            NgsiError::LdContextNotAvailable(String::new()).status(),
            504
        );
        assert_eq!(NgsiError::NoMultiTenantSupport(String::new()).status(), 501);
        assert_eq!(NgsiError::NonexistentTenant(String::new()).status(), 404);
        assert_eq!(
            NgsiError::OperationNotSupported(String::new()).status(),
            422
        );
        assert_eq!(NgsiError::ResourceNotFound(String::new()).status(), 404);
        assert_eq!(NgsiError::TooComplexQuery(String::new()).status(), 403);
        assert_eq!(NgsiError::TooManyResults(String::new()).status(), 403);
        assert_eq!(NgsiError::Conflict(String::new()).status(), 409);
    }

    /// 5.5.2 Table 5.5.2-1 — the error names are the wire contract: clients
    /// branch on the type URI, so every variant's name is pinned here, not
    /// just the one the round-trip test happens to use.
    #[test]
    fn every_error_type_uri_matches_table_5_5_2_1() {
        for (e, name) in [
            (NgsiError::AlreadyExists(String::new()), "AlreadyExists"),
            (NgsiError::BadRequestData(String::new()), "BadRequestData"),
            (NgsiError::Conflict(String::new()), "Conflict"),
            (NgsiError::InternalError(String::new()), "InternalError"),
            (NgsiError::InvalidRequest(String::new()), "InvalidRequest"),
            (
                NgsiError::LdContextNotAvailable(String::new()),
                "LdContextNotAvailable",
            ),
            (
                NgsiError::NoMultiTenantSupport(String::new()),
                "NoMultiTenantSupport",
            ),
            (
                NgsiError::NonexistentTenant(String::new()),
                "NonexistentTenant",
            ),
            (
                NgsiError::OperationNotSupported(String::new()),
                "OperationNotSupported",
            ),
            (
                NgsiError::ResourceNotFound(String::new()),
                "ResourceNotFound",
            ),
            (NgsiError::TooComplexQuery(String::new()), "TooComplexQuery"),
            (NgsiError::TooManyResults(String::new()), "TooManyResults"),
        ] {
            assert_eq!(e.kind(), name);
            let pd = e.to_problem_details();
            assert_eq!(pd.r#type, format!("{ERROR_TYPE_BASE}{name}"));
            assert_eq!(pd.title, name);
            assert!(
                !pd.r#type.starts_with("http://"),
                "the V1.9.1 base is https"
            );
        }
    }

    /// 6.3.6 / RFC 7807: the member names are `type`, `title`, `status` and
    /// `detail` — the Rust raw identifier must not leak as `r#type`.
    #[test]
    fn problem_details_serializes_rfc_7807_member_names() {
        let body =
            serde_json::to_value(NgsiError::TooManyResults("too many".into()).to_problem_details())
                .expect("serialize");
        let obj = body.as_object().expect("object");
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["detail", "status", "title", "type"]);
        assert_eq!(obj["status"], 403);
    }

    #[test]
    fn problem_details_uses_https_base() {
        let pd = NgsiError::ResourceNotFound("nope".into()).to_problem_details();
        assert_eq!(
            pd.r#type,
            "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound"
        );
        assert_eq!(pd.status, 404);
    }
}
