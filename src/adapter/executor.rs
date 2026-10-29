use super::*;
use crate::config::{Host, Model};
use std::process::Command;

#[derive(Clone, Default)]
pub(crate) struct NativeOptions {
    args: Vec<String>,
    pub model: Option<Model>,
}

impl std::fmt::Debug for NativeOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeOptions")
            .field("args", &self.args.len())
            .field("model", &self.model.is_some())
            .finish()
    }
}

impl NativeOptions {
    pub fn new(host: Host, args: &[String], model: Option<&Model>) -> Result<Self> {
        if args.len() > 16 || !args.len().is_multiple_of(2) {
            return Err(ErrorCode::ConfigInvalid.into());
        }
        for pair in args.as_chunks::<2>().0 {
            let (flag, value) = (pair[0].as_str(), pair[1].as_str());
            let allowed = match host {
                Host::Codex if matches!(flag, "-c" | "--config") => {
                    let table = value
                        .parse::<toml::Table>()
                        .map_err(|_| ErrorCode::ConfigInvalid)?;
                    table.len() == 1
                        && table.iter().all(|(key, value)| {
                            let value = value.as_str().unwrap_or("");
                            match key.as_str() {
                                "model_reasoning_effort" => matches!(
                                    value,
                                    "none" | "minimal" | "low" | "medium" | "high" | "xhigh"
                                ),
                                "model_verbosity" => matches!(value, "low" | "medium" | "high"),
                                _ => false,
                            }
                        })
                }
                Host::ClaudeCode => {
                    flag == "--effort" && matches!(value, "low" | "medium" | "high" | "max")
                }
                Host::Pi => {
                    flag == "--thinking"
                        && matches!(
                            value,
                            "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
                        )
                }
                Host::Hermes => {
                    flag == "--max-iterations" && value.parse::<u32>().is_ok_and(|n| n > 0)
                }
                _ => false,
            };
            if !allowed || value.len() > 256 {
                return Err(ErrorCode::ConfigInvalid.into());
            }
        }
        Ok(Self {
            args: args.to_vec(),
            model: model.cloned(),
        })
    }

    pub fn apply(&self, host: Host, command: &mut Command) -> Result<()> {
        command
            .args(&self.args)
            .env_remove("AKASHA_EXECUTOR_MODEL")
            .env_remove("AKASHA_EXECUTOR_KEY");
        let Some(model) = &self.model else {
            return Ok(());
        };
        let credential = model
            .credential_ref
            .as_ref()
            .map(|reference| {
                let name = reference
                    .strip_prefix("env:")
                    .ok_or(ErrorCode::ConfigInvalid)?;
                let secret = std::env::var(name).map_err(|_| ErrorCode::AuthRequired)?;
                if secret.is_empty() {
                    return Err(ErrorCode::AuthRequired.into());
                }
                Ok::<_, AdapterError>((name, secret))
            })
            .transpose()?;
        let string = |s: &str| serde_json::to_string(s).map_err(|_| ErrorCode::ConfigInvalid);
        match host {
            Host::Codex => {
                for setting in [
                    format!("model={}", string(&model.model)?),
                    "model_provider=\"akasha_executor\"".into(),
                    "model_providers.akasha_executor.name=\"Akasha executor\"".into(),
                    format!(
                        "model_providers.akasha_executor.base_url={}",
                        string(model.endpoint.as_str().trim_end_matches('/'))?
                    ),
                    "model_providers.akasha_executor.wire_api=\"responses\"".into(),
                    "model_providers.akasha_executor.requires_openai_auth=false".into(),
                    "model_providers.akasha_executor.supports_websockets=false".into(),
                ] {
                    command.args(["-c", &setting]);
                }
                if let Some((name, _)) = credential {
                    command.args([
                        "-c",
                        &format!("model_providers.akasha_executor.env_key={}", string(name)?),
                    ]);
                }
            }
            Host::ClaudeCode => {
                let (_, secret) = credential.ok_or(ErrorCode::AuthRequired)?;
                let endpoint = model.endpoint.as_str().trim_end_matches('/');
                command
                    .args(["--model", &model.model])
                    .env(
                        "ANTHROPIC_BASE_URL",
                        endpoint.strip_suffix("/v1").unwrap_or(endpoint),
                    )
                    .env("ANTHROPIC_API_KEY", secret)
                    .env_remove("ANTHROPIC_AUTH_TOKEN")
                    .env_remove("CLAUDE_CODE_OAUTH_TOKEN");
                for variable in [
                    "CLAUDE_CODE_USE_BEDROCK",
                    "CLAUDE_CODE_USE_VERTEX",
                    "CLAUDE_CODE_USE_FOUNDRY",
                ] {
                    command.env_remove(variable);
                }
            }
            Host::Hermes | Host::Pi => {
                let (_, secret) = credential.ok_or(ErrorCode::AuthRequired)?;
                command
                    .env(
                        "AKASHA_EXECUTOR_MODEL",
                        serde_json::to_string(model).map_err(|_| ErrorCode::ConfigInvalid)?,
                    )
                    .env("AKASHA_EXECUTOR_KEY", secret);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_model_credentials_are_explicit_and_inherited_overrides_are_removed() {
        for host in [Host::Codex, Host::ClaudeCode, Host::Hermes, Host::Pi] {
            let mut command = Command::new("unused");
            command
                .env("AKASHA_EXECUTOR_MODEL", "inherited")
                .env("AKASHA_EXECUTOR_KEY", "inherited");
            NativeOptions::default().apply(host, &mut command).unwrap();
            assert!(command.get_envs().all(|(_, value)| value.is_none()));
            let model: Model = serde_json::from_value(serde_json::json!({
                "kind":"text", "endpoint":"http://127.0.0.1:9/v1", "model":"fixture", "local":true,
                "credential_ref":"env:AKASHA_TEST_UNAVAILABLE_CREDENTIAL_16"
            }))
            .unwrap();
            assert!(std::env::var_os("AKASHA_TEST_UNAVAILABLE_CREDENTIAL_16").is_none());
            assert_eq!(
                NativeOptions::new(host, &[], Some(&model))
                    .unwrap()
                    .apply(host, &mut command)
                    .unwrap_err()
                    .code,
                ErrorCode::AuthRequired
            );
            let model = Model {
                credential_ref: None,
                ..model
            };
            let result = NativeOptions::new(host, &[], Some(&model))
                .unwrap()
                .apply(host, &mut command);
            if host == Host::Codex {
                assert!(result.is_ok());
            } else {
                assert_eq!(result.unwrap_err().code, ErrorCode::AuthRequired);
            }
        }
    }

    #[test]
    fn native_arguments_cannot_replace_tool_or_profile_restrictions() {
        for (host, flag, value) in [
            (Host::Codex, "-c", "features.shell_tool=true"),
            (
                Host::Codex,
                "-c",
                "model_reasoning_effort=\"low\"\nweb_search=\"live\"",
            ),
            (Host::ClaudeCode, "--tools", "Bash"),
            (Host::Pi, "-e", "/untrusted"),
            (Host::Hermes, "--request", "/other-prompt"),
        ] {
            assert_eq!(
                NativeOptions::new(host, &[flag.into(), value.into()], None)
                    .unwrap_err()
                    .code,
                ErrorCode::ConfigInvalid
            );
        }
        for (host, flag, value) in [
            (Host::Codex, "-c", "model_reasoning_effort=\"high\""),
            (Host::ClaudeCode, "--effort", "high"),
            (Host::Pi, "--thinking", "low"),
            (Host::Hermes, "--max-iterations", "20"),
        ] {
            assert!(NativeOptions::new(host, &[flag.into(), value.into()], None).is_ok());
        }
    }
}
