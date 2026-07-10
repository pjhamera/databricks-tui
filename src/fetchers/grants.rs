use crate::cli::DatabricksCli;
use crate::shape::{DetailData, Status};
use serde_json::Value;

/// Fetches who-can-do-what for an object: Unity Catalog effective grants
/// (including inherited ones) or workspace object ACLs.
pub async fn fetch(cli: &DatabricksCli, uc: bool, object_type: &str, id: &str) -> DetailData {
    let result = if uc {
        cli.run(&["grants", "get-effective", object_type, id]).await
    } else {
        cli.run(&["permissions", "get", object_type, id]).await
    };
    let json = match result {
        Ok(v) => v,
        Err(e) => {
            return DetailData {
                summary: Vec::new(),
                activity: Vec::new(),
                raw: format!("{e:#}"),
            }
        }
    };
    let raw = serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string());
    let summary = vec![
        ("Object".to_string(), id.to_string()),
        ("Type".to_string(), object_type.to_string()),
    ];
    let activity = if uc {
        uc_grants(&json)
    } else {
        workspace_acl(&json)
    };

    DetailData {
        summary,
        activity,
        raw,
    }
}

fn uc_grants(j: &Value) -> Vec<(Status, String)> {
    j["privilege_assignments"]
        .as_array()
        .map(|assignments| {
            assignments
                .iter()
                .map(|a| {
                    let principal = a["principal"].as_str().unwrap_or("?");
                    let privileges: Vec<String> = a["privileges"]
                        .as_array()
                        .map(|ps| {
                            ps.iter()
                                .map(|p| {
                                    let name = p["privilege"].as_str().unwrap_or("?");
                                    match p["inherited_from_name"].as_str() {
                                        Some(from) => format!("{name} (from {from})"),
                                        None => name.to_string(),
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let owner = privileges.iter().any(|p| p.starts_with("ALL_PRIVILEGES"));
                    let status = if owner {
                        Status::Success
                    } else {
                        Status::Unknown(String::new())
                    };
                    (status, format!("{principal}  ·  {}", privileges.join(", ")))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn workspace_acl(j: &Value) -> Vec<(Status, String)> {
    j["access_control_list"]
        .as_array()
        .map(|acl| {
            acl.iter()
                .map(|entry| {
                    let principal = entry["user_name"]
                        .as_str()
                        .or_else(|| entry["group_name"].as_str())
                        .or_else(|| entry["service_principal_name"].as_str())
                        .unwrap_or("?");
                    let perms: Vec<String> = entry["all_permissions"]
                        .as_array()
                        .map(|ps| {
                            ps.iter()
                                .map(|p| {
                                    let level = p["permission_level"].as_str().unwrap_or("?");
                                    if p["inherited"].as_bool().unwrap_or(false) {
                                        format!("{level} (inherited)")
                                    } else {
                                        level.to_string()
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let owner = perms.iter().any(|p| p.starts_with("IS_OWNER"));
                    let status = if owner {
                        Status::Success
                    } else {
                        Status::Unknown(String::new())
                    };
                    (status, format!("{principal}  ·  {}", perms.join(", ")))
                })
                .collect()
        })
        .unwrap_or_default()
}
