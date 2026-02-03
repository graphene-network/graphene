use crate::unikraft::{Runtime, UnikraftError, ValidatedDockerfile};
use dockerfile_parser::{Dockerfile, Instruction};
use std::collections::HashSet;

/// Validator for Dockerfiles to ensure unikernel compatibility
pub struct DockerfileValidator {
    forbidden_commands: HashSet<&'static str>,
    allowed_run_patterns: Vec<&'static str>,
}

impl Default for DockerfileValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerfileValidator {
    pub fn new() -> Self {
        let forbidden_commands: HashSet<&'static str> = [
            "USER",
            "VOLUME",
            "SHELL",
            "ADD",
            "STOPSIGNAL",
            "EXPOSE",
            "HEALTHCHECK",
        ]
        .into_iter()
        .collect();

        let allowed_run_patterns = vec![
            "npm install",
            "npm ci",
            "npm run build",
            "yarn install",
            "yarn build",
        ];

        Self {
            forbidden_commands,
            allowed_run_patterns,
        }
    }

    /// Validate a Dockerfile and return parsed metadata
    pub fn validate(&self, content: &str) -> Result<ValidatedDockerfile, UnikraftError> {
        let dockerfile = Dockerfile::parse(content).map_err(|e| {
            UnikraftError::DockerfileParseError(format!("Failed to parse Dockerfile: {}", e))
        })?;

        let mut from_image: Option<String> = None;
        let mut entrypoint: Vec<String> = Vec::new();

        for instruction in dockerfile.instructions {
            match &instruction {
                Instruction::From(from) => {
                    from_image = Some(from.image_parsed.image.clone());
                }
                Instruction::Run(run) => {
                    let cmd = if let Some(shell) = run.as_shell() {
                        shell.to_string()
                    } else if let Some(exec) = run.as_exec() {
                        exec.as_str_vec().join(" ")
                    } else {
                        continue;
                    };
                    self.validate_run_command(&cmd)?;
                }
                Instruction::Cmd(cmd) => {
                    if cmd.as_shell().is_some() {
                        return Err(UnikraftError::UnsupportedCommand {
                            command: "CMD".to_string(),
                            reason: "Shell form is not supported. Use exec form: [\"executable\", \"arg1\"]".to_string(),
                        });
                    }
                    if let Some(exec) = cmd.as_exec() {
                        entrypoint = exec.as_str_vec().iter().map(|s| s.to_string()).collect();
                    }
                }
                Instruction::Entrypoint(ep) => {
                    if ep.as_shell().is_some() {
                        return Err(UnikraftError::UnsupportedCommand {
                            command: "ENTRYPOINT".to_string(),
                            reason: "Shell form is not supported. Use exec form: [\"executable\", \"arg1\"]".to_string(),
                        });
                    }
                    if let Some(exec) = ep.as_exec() {
                        entrypoint = exec.as_str_vec().iter().map(|s| s.to_string()).collect();
                    }
                }
                Instruction::Copy(copy) => {
                    // ADD is forbidden, but COPY with --from is allowed for multi-stage builds
                    // Check if it's using ADD-like features we don't support
                    if copy.flags.iter().any(|f| {
                        let flag_str = format!("{:?}", f);
                        flag_str.contains("chown") || flag_str.contains("chmod")
                    }) {
                        return Err(UnikraftError::UnsupportedCommand {
                            command: "COPY".to_string(),
                            reason: "--chown and --chmod flags are not supported in unikernels"
                                .to_string(),
                        });
                    }
                }
                Instruction::Misc(misc) => {
                    let cmd_upper = misc.instruction.content.to_uppercase();
                    if self.forbidden_commands.contains(cmd_upper.as_str()) {
                        return Err(UnikraftError::UnsupportedCommand {
                            command: cmd_upper,
                            reason: "This command is not compatible with unikernels".to_string(),
                        });
                    }
                }
                // Allowed instructions that don't need special handling
                Instruction::Arg(_) | Instruction::Label(_) | Instruction::Env(_) => {}
            }
        }

        let from_image = from_image.ok_or_else(|| {
            UnikraftError::DockerfileParseError("Missing FROM instruction".to_string())
        })?;

        let runtime = self.validate_base_image(&from_image)?;

        Ok(ValidatedDockerfile {
            content: content.to_string(),
            runtime,
            entrypoint,
        })
    }

    fn validate_base_image(&self, image: &str) -> Result<Runtime, UnikraftError> {
        match image {
            "graphene/node" => Ok(Runtime::Node20),
            _ => Err(UnikraftError::UnsupportedBaseImage(format!(
                "Base image '{}' is not supported. Use one of: graphene/node:20",
                image
            ))),
        }
    }

    fn validate_run_command(&self, cmd: &str) -> Result<(), UnikraftError> {
        let cmd_lower = cmd.to_lowercase();

        let is_allowed = self
            .allowed_run_patterns
            .iter()
            .any(|pattern| cmd_lower.contains(&pattern.to_lowercase()));

        if !is_allowed {
            return Err(UnikraftError::InvalidRunCommand(format!(
                "RUN command '{}' is not allowed. Allowed patterns: {:?}",
                cmd, self.allowed_run_patterns
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_allowed_dockerfile() {
        let dockerfile = r#"
FROM graphene/node:20
WORKDIR /app
COPY package.json .
RUN npm install
COPY . .
CMD ["node", "index.js"]
"#;

        let validator = DockerfileValidator::new();
        let result = validator.validate(dockerfile);
        assert!(result.is_ok(), "Expected Ok, got {:?}", result);

        let validated = result.unwrap();
        assert_eq!(validated.runtime, Runtime::Node20);
        assert_eq!(validated.entrypoint, vec!["node", "index.js"]);
    }

    #[test]
    fn test_reject_forbidden_command() {
        let dockerfile = r#"
FROM graphene/node:20
USER node
CMD ["node", "index.js"]
"#;

        let validator = DockerfileValidator::new();
        let result = validator.validate(dockerfile);

        assert!(matches!(
            result,
            Err(UnikraftError::UnsupportedCommand { command, .. }) if command == "USER"
        ));
    }

    #[test]
    fn test_reject_shell_form() {
        let dockerfile = r#"
FROM graphene/node:20
CMD node index.js
"#;

        let validator = DockerfileValidator::new();
        let result = validator.validate(dockerfile);

        assert!(matches!(
            result,
            Err(UnikraftError::UnsupportedCommand { command, reason })
                if command == "CMD" && reason.contains("Shell form")
        ));
    }

    #[test]
    fn test_reject_unsupported_base_image() {
        let dockerfile = r#"
FROM ubuntu:22.04
CMD ["bash"]
"#;

        let validator = DockerfileValidator::new();
        let result = validator.validate(dockerfile);

        assert!(matches!(
            result,
            Err(UnikraftError::UnsupportedBaseImage(_))
        ));
    }

    #[test]
    fn test_reject_invalid_run_command() {
        let dockerfile = r#"
FROM graphene/node:20
RUN apt-get update
CMD ["node", "index.js"]
"#;

        let validator = DockerfileValidator::new();
        let result = validator.validate(dockerfile);

        assert!(matches!(result, Err(UnikraftError::InvalidRunCommand(_))));
    }

    #[test]
    fn test_parse_line_continuation() {
        let dockerfile = r#"
FROM graphene/node:20
RUN npm install \
    --production
CMD ["node", "index.js"]
"#;

        let validator = DockerfileValidator::new();
        let result = validator.validate(dockerfile);
        assert!(result.is_ok(), "Expected Ok, got {:?}", result);
    }

    #[test]
    fn test_env_variables() {
        let dockerfile = r#"
FROM graphene/node:20
ENV NODE_ENV=production
ENV PORT 3000
RUN npm install
CMD ["node", "index.js"]
"#;

        let validator = DockerfileValidator::new();
        let result = validator.validate(dockerfile);
        assert!(result.is_ok(), "Expected Ok, got {:?}", result);
    }

    #[test]
    fn test_arg_with_default() {
        let dockerfile = r#"
FROM graphene/node:20
ARG NODE_ENV=production
ARG VERSION
RUN npm install
CMD ["node", "index.js"]
"#;

        let validator = DockerfileValidator::new();
        let result = validator.validate(dockerfile);
        assert!(result.is_ok(), "Expected Ok, got {:?}", result);
    }

    #[test]
    fn test_entrypoint_exec_form() {
        let dockerfile = r#"
FROM graphene/node:20
ENTRYPOINT ["node"]
CMD ["index.js"]
"#;

        let validator = DockerfileValidator::new();
        let result = validator.validate(dockerfile);
        assert!(result.is_ok(), "Expected Ok, got {:?}", result);

        // ENTRYPOINT takes precedence over CMD for the entrypoint field
        let validated = result.unwrap();
        assert_eq!(validated.entrypoint, vec!["index.js"]);
    }

    #[test]
    fn test_entrypoint_shell_form_rejected() {
        let dockerfile = r#"
FROM graphene/node:20
ENTRYPOINT node index.js
"#;

        let validator = DockerfileValidator::new();
        let result = validator.validate(dockerfile);

        assert!(matches!(
            result,
            Err(UnikraftError::UnsupportedCommand { command, reason })
                if command == "ENTRYPOINT" && reason.contains("Shell form")
        ));
    }
}
