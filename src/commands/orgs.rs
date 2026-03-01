use std::process;

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
