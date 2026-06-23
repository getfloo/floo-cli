use std::process;

use crate::errors::ErrorCode;
use crate::output;

/// List the current user's email notification preferences.
pub fn list() {
    super::require_auth();
    let client = super::init_client(None);

    let result = match client.get_notification_preferences() {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let rows: Vec<Vec<String>> = result
        .preferences
        .iter()
        .map(|p| {
            vec![
                p.category.clone(),
                if p.enabled {
                    "on".to_string()
                } else {
                    "off".to_string()
                },
                p.label.clone(),
            ]
        })
        .collect();

    output::table(
        &["CATEGORY", "EMAILS", "WHAT"],
        &rows,
        Some(output::to_value(&result)),
    );
}

/// Turn one category of email on or off for the current user.
pub fn set(category: &str, value: &str) {
    super::require_auth();
    let client = super::init_client(None);

    let enabled = match value {
        "on" => true,
        "off" => false,
        _ => {
            output::error(
                &format!("Invalid value '{value}'. Use 'on' or 'off'."),
                &ErrorCode::InvalidFormat,
                None,
            );
            process::exit(1);
        }
    };

    match client.set_notification_preference(category, enabled) {
        Ok(result) => {
            let label = result
                .preferences
                .iter()
                .find(|p| p.category == category)
                .map(|p| p.label.clone())
                .unwrap_or_else(|| category.to_string());
            output::success(
                &format!("{label} emails turned {value}."),
                Some(output::to_value(&result)),
            );
        }
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    }
}
