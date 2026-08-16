//! NGSI-LD error model: the Table 5.5.2-1 error-type vocabulary (variant
//! names verbatim) with the Table 6.3.2-1 HTTP status mapping. Error type
//! URI base is https (V1.9.1). errors/Conflict entered with 5.9.2.4
//! (registration-vs-entity/registration proxied-mode conflicts, 409).

use serde::Serialize;
use thiserror::Error;

pub const ERROR_TYPE_BASE: &str = "https://uri.etsi.org/ngsi-ld/errors/";

#[derive(Debug, Error)]
pub enum NgsiError {
    #[error("{0}")]
    AlreadyExists(String),
    #[error("{0}")]
    BadRequestData(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    InvalidRequest(String),
    #[error("{0}")]
    InternalError(String),
    #[error("{0}")]
    LdContextNotAvailable(String),
    #[error("{0}")]
    NoMultiTenantSupport(String),
    #[error("{0}")]
    NonexistentTenant(String),
    #[error("{0}")]
    OperationNotSupported(String),
    #[error("{0}")]
    ResourceNotFound(String),
    #[error("{0}")]
    TooComplexQuery(String),
    #[error("{0}")]
    TooManyResults(String),
}

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
    pub r#type: String,
    pub title: String,
    pub status: u16,
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
