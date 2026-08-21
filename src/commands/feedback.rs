use std::process;

use crate::errors::ErrorCode;
use crate::output;

pub const MAX_FEEDBACK_CONTEXT_LENGTH: usize = 10_000;

fn validate_context_length(context: Option<&str>) -> Result<(), usize> {
    let actual_length = context.map(str::chars).map(Iterator::count).unwrap_or(0);
    if actual_length > MAX_FEEDBACK_CONTEXT_LENGTH {
        return Err(actual_length);
    }
    Ok(())
}

pub fn feedback(message: &str, category: &str, app: Option<&str>, context: Option<&str>) {
    if let Err(actual_length) = validate_context_length(context) {
        output::error(
            &format!(
                "Feedback context is {actual_length} characters; the maximum is {MAX_FEEDBACK_CONTEXT_LENGTH}."
            ),
            &ErrorCode::Other("FEEDBACK_CONTEXT_TOO_LONG".to_string()),
            None,
        );
        process::exit(1);
    }

    super::require_auth();
    let client = super::init_client(None);

    let source = if output::is_json_mode() {
        "agent"
    } else {
        "cli"
    };

    match client.submit_feedback(category, message, source, context, app) {
        Ok(()) => output::success("Feedback submitted. Thanks!", None),
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_length_validation_counts_unicode_characters() {
        let at_limit = "é".repeat(MAX_FEEDBACK_CONTEXT_LENGTH);
        let over_limit = format!("{at_limit}é");

        assert_eq!(validate_context_length(Some(&at_limit)), Ok(()));
        assert_eq!(
            validate_context_length(Some(&over_limit)),
            Err(MAX_FEEDBACK_CONTEXT_LENGTH + 1)
        );
    }
}
