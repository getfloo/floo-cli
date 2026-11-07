use std::process;

use crate::errors::ErrorCode;
use crate::output;

pub fn promote(app: Option<&str>, tag: Option<&str>) {
    super::require_auth();
    let client = super::init_client(None);

    let (app_id, name) = super::resolve_app_from_config(&client, app);
    let app_id = app_id.as_str();
    let name = name.as_str();

    let _spinner = output::Spinner::new(&format!("Promoting {name} to prod..."));

    let result = match client.promote_app(app_id, tag) {
        Ok(r) => r,
        Err(e) => {
            // PROMOTE_USE_GITHUB_RELEASE carries the exact `gh release create` command.
            let api_hint = e
                .extra
                .as_ref()
                .and_then(|extra| extra.get("hint"))
                .and_then(|hint| hint.as_str());
            let suggestion = api_hint.or(match e.code.as_str() {
                "NO_DEV_DEPLOY" => {
                    Some("Deploy to dev first: run floo deploy, or push to the connected branch")
                }
                "RELEASE_TAG_EXISTS" => Some("Use a different tag with --tag <tag>"),
                _ => None,
            });
            output::error(&e.message, &ErrorCode::from_api(&e.code), suggestion);
            process::exit(1);
        }
    };

    let result_tag = &result.tag;
    let release_url = &result.release_url;

    if output::is_json_mode() {
        output::success(
            &format!("Promoted {name} \u{2192} prod ({result_tag})"),
            Some(output::to_value(&result)),
        );
    } else {
        output::success(
            &format!("Promoted {name} \u{2192} prod ({result_tag})"),
            Some(serde_json::json!({
                "app": name,
                "tag": result_tag,
                "release_url": release_url,
            })),
        );
        if let Some(release_url) = release_url {
            output::dim_line(&format!("Release: {release_url}"));
        }
        output::dim_line("Prod deploy in progress.");
    }
}
