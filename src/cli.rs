use anyhow::{Context, Result};
use serde_json::Value;
use tokio::process::Command;

/// Profile names from ~/.databrickscfg, in file order.
pub fn list_profiles() -> Vec<String> {
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let path = std::path::Path::new(&home).join(".databrickscfg");
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
                .map(str::to_string)
        })
        .filter(|name| !name.starts_with("__"))
        .collect()
}

pub struct DatabricksCli {
    profile: Option<String>,
}

impl DatabricksCli {
    pub fn new(profile: Option<String>) -> Self {
        Self { profile }
    }

    pub async fn run(&self, args: &[&str]) -> Result<Value> {
        let mut cmd = Command::new("databricks");
        // Run from a neutral directory: inside a bundle folder the CLI
        // resolves typed commands against the bundle's workspace but raw
        // `api` calls against the default profile, splitting the app
        // across two workspaces. Neutral cwd keeps auth consistent.
        cmd.current_dir("/");
        cmd.arg("--output").arg("json");
        if let Some(p) = &self.profile {
            cmd.arg("--profile").arg(p);
        }
        cmd.args(args);

        let out = cmd
            .output()
            .await
            .context("failed to run databricks CLI — is it installed?")?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("databricks CLI error: {}", stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        serde_json::from_str(&stdout).context("failed to parse CLI JSON output")
    }

    /// Runs a command whose stdout is plain text, not JSON —
    /// e.g. `fs cat` on a file inside a volume.
    pub async fn run_raw(&self, args: &[&str]) -> Result<String> {
        let mut cmd = Command::new("databricks");
        cmd.current_dir("/");
        if let Some(p) = &self.profile {
            cmd.arg("--profile").arg(p);
        }
        cmd.args(args);

        let out = cmd
            .output()
            .await
            .context("failed to run databricks CLI — is it installed?")?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("databricks CLI error: {}", stderr.trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// Runs a mutating command where success is all that matters —
    /// start/stop/run-now often print nothing or non-JSON on success.
    pub async fn run_action(&self, args: &[&str]) -> Result<()> {
        let mut cmd = Command::new("databricks");
        cmd.current_dir("/");
        if let Some(p) = &self.profile {
            cmd.arg("--profile").arg(p);
        }
        cmd.args(args);

        let out = cmd
            .output()
            .await
            .context("failed to run databricks CLI — is it installed?")?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("databricks CLI error: {}", stderr.trim());
        }
        Ok(())
    }
}
