use std::process;

use crate::errors::ErrorCode;
use crate::output;

pub fn list() {
    super::require_auth();
    let client = super::init_client(None);

    let result = match client.list_base_images() {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success("Base images", Some(output::to_value(&result)));
        return;
    }

    let rows: Vec<Vec<String>> = result
        .images
        .iter()
        .map(|img| {
            vec![
                img.name.clone(),
                img.tag.clone(),
                img.public_uri.clone(),
                img.mirror_uri.clone().unwrap_or_else(|| "-".to_owned()),
            ]
        })
        .collect();

    output::table(
        &["Name", "Tag", "Public URI", "Deploy Mirror URI"],
        &rows,
        None,
    );
}
