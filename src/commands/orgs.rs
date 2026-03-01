use std::process;

use crate::output;

pub fn list_members() {
    super::require_auth();
    let client = super::init_client(None);

    let org = match client.get_org_me() {
        Ok(o) => o,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };
    let org_id = super::expect_str_field(&org, "id");

    let result = match client.list_members(org_id) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };

    let members = result
        .get("members")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            output::error(
                "Failed to parse members from API response.",
                "PARSE_ERROR",
                Some("This is a bug. Please report it."),
            );
            process::exit(1);
        });

    if members.is_empty() {
        output::info("No members found.", None);
        return;
    }

    let rows: Vec<Vec<String>> = members
        .iter()
        .map(|m| {
            vec![
                m.get("user_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                m.get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                m.get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
            ]
        })
        .collect();

    output::table(&["USER ID", "EMAIL", "ROLE"], &rows, Some(result));
}

pub fn set_role(user_id: &str, role: &str) {
    super::require_auth();
    let client = super::init_client(None);

    let valid_roles = ["admin", "member", "viewer"];
    if !valid_roles.contains(&role) {
        output::error(
            &format!("Invalid role '{role}'. Must be one of: admin, member, viewer."),
            "INVALID_ROLE",
            None,
        );
        process::exit(1);
    }

    let org = match client.get_org_me() {
        Ok(o) => o,
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    };
    let org_id = super::expect_str_field(&org, "id");

    match client.update_member_role(org_id, user_id, role) {
        Ok(result) => {
            let email = result
                .get("email")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let new_role = result.get("role").and_then(|v| v.as_str()).unwrap_or(role);
            output::success(
                &format!("Updated {email} to role '{new_role}'."),
                Some(result),
            );
        }
        Err(e) => {
            output::error(&e.message, &e.code, None);
            process::exit(1);
        }
    }
}
