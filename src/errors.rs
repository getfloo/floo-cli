use thiserror::Error;

#[derive(Error, Debug)]
#[error("{message}")]
pub struct FlooError {
    pub code: String,
    pub message: String,
    pub suggestion: Option<String>,
}

impl FlooError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            suggestion: None,
        }
    }

    pub fn with_suggestion(
        code: impl Into<String>,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            suggestion: Some(suggestion.into()),
        }
    }
}

#[derive(Error, Debug)]
#[error("{message}")]
pub struct FlooApiError {
    pub status_code: u16,
    pub code: String,
    pub message: String,
}

impl FlooApiError {
    pub fn new(status_code: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status_code,
            code: code.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_floo_error_display() {
        let err = FlooError::new("TEST", "something failed");
        assert_eq!(err.to_string(), "something failed");
        assert_eq!(err.code, "TEST");
        assert!(err.suggestion.is_none());
    }

    #[test]
    fn test_floo_error_with_suggestion() {
        let err = FlooError::with_suggestion("CODE", "msg", "try this");
        assert_eq!(err.suggestion.as_deref(), Some("try this"));
    }

    #[test]
    fn test_floo_api_error() {
        let err = FlooApiError::new(404, "NOT_FOUND", "App not found.");
        assert_eq!(err.status_code, 404);
        assert_eq!(err.code, "NOT_FOUND");
        assert_eq!(err.to_string(), "App not found.");
    }
}
