//! Persistence errors. `api` maps these onto `AppError`; nothing here ever
//! reaches a client directly.

/// A repository failure.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// The row does not exist, or the caller may not see it. The distinction is
    /// made by the caller, which is what turns an invisible channel into a 404
    /// instead of a 403 (`docs/api/rest-api.md` §3).
    #[error("{0} not found")]
    NotFound(&'static str),

    /// A uniqueness or check constraint rejected the write.
    #[error("conflict: {0}")]
    Conflict(&'static str),

    /// Anything else the database reported.
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}

pub type DbResult<T> = Result<T, DbError>;

impl DbError {
    /// The constraint name PostgreSQL reported, when the failure was a
    /// constraint violation.
    pub fn constraint(&self) -> Option<&str> {
        match self {
            Self::Sqlx(sqlx::Error::Database(e)) => e.constraint(),
            _ => None,
        }
    }

    /// True when the failure was a unique-violation on the given constraint.
    pub fn is_unique_violation(&self, constraint: &str) -> bool {
        matches!(self, Self::Sqlx(sqlx::Error::Database(e))
            if e.code().as_deref() == Some("23505") && e.constraint() == Some(constraint))
    }
}

/// `sqlx::Error::RowNotFound` becomes `NotFound`; everything else stays.
pub fn missing<T>(resource: &'static str, result: Result<T, sqlx::Error>) -> DbResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(sqlx::Error::RowNotFound) => Err(DbError::NotFound(resource)),
        Err(other) => Err(DbError::Sqlx(other)),
    }
}
