use anyhow::{Context, Result};
use serde_json::Value;
use tokio::process::Command;

pub struct DatabricksCli {
    profile: Option<String>,
}

impl DatabricksCli {
    pub fn new(profile: Option<String>) -> Self {
        Self { profile }
    }

    pub async fn run(&self, args: &[&str]) -> Result<Value> {
        let mut cmd = Command::new("databricks");
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

    /// Runs a mutating command where success is all that matters —
    /// start/stop/run-now often print nothing or non-JSON on success.
    pub async fn run_action(&self, args: &[&str]) -> Result<()> {
        let mut cmd = Command::new("databricks");
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
