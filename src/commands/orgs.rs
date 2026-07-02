use std::process;

use crate::config::{load_config, save_config};
use crate::errors::ErrorCode;
use crate::output;

pub fn list_members() {
    super::require_auth();
    let client = super::init_client(None);

    let org = match client.get_org_me() {
        Ok(o) => o,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let result = match client.list_members(&org.id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if result.members.is_empty() {
        output::info("No members found.", None);
        return;
    }

    let rows: Vec<Vec<String>> = result
        .members
        .iter()
        .map(|m| vec![m.user_id.clone(), m.email.clone(), m.role.clone()])
        .collect();

    output::table(
        &["USER ID", "EMAIL", "ROLE"],
        &rows,
        Some(output::to_value(&result)),
    );
}

pub fn set_role(user_id: &str, role: &str) {
    super::require_auth();
    let client = super::init_client(None);

    let valid_roles = ["admin", "member", "viewer"];
    if !valid_roles.contains(&role) {
        output::error(
            &format!("Invalid role '{role}'. Must be one of: admin, member, viewer."),
            &ErrorCode::InvalidRole,
            None,
        );
        process::exit(1);
    }

    let org = match client.get_org_me() {
        Ok(o) => o,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    match client.update_member_role(&org.id, user_id, role) {
        Ok(result) => {
            output::success(
                &format!("Updated {} to role '{}'.", result.email, result.role),
                Some(output::to_value(&result)),
            );
        }
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    }
}

pub fn invite(email: &str, role: &str) {
    // `role` is constrained to admin/member/viewer by clap's value_parser, so
    // no manual validation is needed here.
    super::require_auth();
    let client = super::init_client(None);

    let org = match client.get_org_me() {
        Ok(o) => o,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };
    let org_name = org.name.as_deref().unwrap_or("your org");

    match client.create_org_invite(&org.id, email, role) {
        Ok(result) => {
            if output::is_json_mode() {
                output::success("", Some(output::to_value(&result)));
            } else {
                output::success(
                    &format!("Invited {email} to {org_name} as {}.", result.role),
                    None,
                );
                output::info(&format!("  Invite link: {}", result.invite_url), None);
                output::info("  An invite email was also sent.", None);
            }
        }
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    }
}

pub fn switch(org_slug: &str) {
    super::require_auth();
    let client = super::init_client(None);

    let orgs = match client.list_orgs() {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    let target = orgs
        .orgs
        .iter()
        .find(|o| o.slug.as_deref() == Some(org_slug) || o.id == org_slug);

    let target = match target {
        Some(o) => o,
        None => {
            let available: Vec<&str> = orgs.orgs.iter().filter_map(|o| o.slug.as_deref()).collect();
            output::error(
                &format!("Organization '{org_slug}' not found."),
                &ErrorCode::from_api("ORG_NOT_FOUND"),
                Some(&format!(
                    "Available orgs: {}",
                    if available.is_empty() {
                        "(none)".to_string()
                    } else {
                        available.join(", ")
                    }
                )),
            );
            process::exit(1);
        }
    };

    let mut config = load_config();
    config.default_org = Some(target.id.clone());
    if let Err(e) = save_config(&config) {
        output::error(
            &format!("Failed to save config: {e}"),
            &ErrorCode::ConfigError,
            None,
        );
        process::exit(1);
    }

    let display = target.display_name().unwrap_or(&target.id);
    output::success(
        &format!("Switched to org '{display}'."),
        Some(output::to_value(target)),
    );
}
