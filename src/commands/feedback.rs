use std::process;

use crate::errors::ErrorCode;
use crate::output;

pub fn feedback(message: &str, category: &str, app: Option<&str>, context: Option<&str>) {
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
