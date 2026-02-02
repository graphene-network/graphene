use crate::unikraft::{Runtime, UnikraftError, ValidatedDockerfile};
use std::collections::HashSet;

/// Dockerfile instruction types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    From(String),
    Copy {
        src: String,
        dest: String,
    },
    Workdir(String),
    Env {
        key: String,
        value: String,
    },
    Cmd(Vec<String>),
    Entrypoint(Vec<String>),
    Arg {
        name: String,
        default: Option<String>,
    },
    Run(String),
    Label {
        key: String,
        value: String,
    },
}

/// Parser for Dockerfiles
pub struct DockerfileParser;

impl DockerfileParser {
    /// Parse a Dockerfile into a list of instructions
    pub fn parse(content: &str) -> Result<Vec<Instruction>, UnikraftError> {
        let mut instructions = Vec::new();
        let mut current_line = String::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Handle line continuations
            if let Some(stripped) = trimmed.strip_suffix('\\') {
                current_line.push_str(stripped);
                current_line.push(' ');
                continue;
            }

            current_line.push_str(trimmed);
            let instruction = Self::parse_instruction(&current_line)?;
            instructions.push(instruction);
            current_line.clear();
        }

        // Handle any remaining content
        if !current_line.is_empty() {
            let instruction = Self::parse_instruction(&current_line)?;
            instructions.push(instruction);
        }

        Ok(instructions)
    }

    fn parse_instruction(line: &str) -> Result<Instruction, UnikraftError> {
        let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
        if parts.is_empty() {
            return Err(UnikraftError::DockerfileParseError(
                "Empty instruction".to_string(),
            ));
        }

        let command = parts[0].to_uppercase();
        let args = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match command.as_str() {
            "FROM" => Ok(Instruction::From(args.to_string())),
            "COPY" => Self::parse_copy(args),
            "WORKDIR" => Ok(Instruction::Workdir(args.to_string())),
            "ENV" => Self::parse_env(args),
            "CMD" => Self::parse_cmd_or_entrypoint(args).map(Instruction::Cmd),
            "ENTRYPOINT" => Self::parse_cmd_or_entrypoint(args).map(Instruction::Entrypoint),
            "ARG" => Self::parse_arg(args),
            "RUN" => Ok(Instruction::Run(args.to_string())),
            "LABEL" => Self::parse_label(args),
            _ => Err(UnikraftError::DockerfileParseError(format!(
                "Unknown instruction: {}",
                command
            ))),
        }
    }

    fn parse_copy(args: &str) -> Result<Instruction, UnikraftError> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(UnikraftError::DockerfileParseError(
                "COPY requires source and destination".to_string(),
            ));
        }
        // Handle multiple sources - last one is destination
        let dest = parts.last().unwrap().to_string();
        let src = parts[..parts.len() - 1].join(" ");
        Ok(Instruction::Copy { src, dest })
    }

    fn parse_env(args: &str) -> Result<Instruction, UnikraftError> {
        // ENV can be "KEY=VALUE" or "KEY VALUE"
        if let Some(eq_pos) = args.find('=') {
            let key = args[..eq_pos].to_string();
            let value = args[eq_pos + 1..].to_string();
            Ok(Instruction::Env { key, value })
        } else {
            let parts: Vec<&str> = args.splitn(2, char::is_whitespace).collect();
            if parts.len() < 2 {
                return Err(UnikraftError::DockerfileParseError(
                    "ENV requires key and value".to_string(),
                ));
            }
            Ok(Instruction::Env {
                key: parts[0].to_string(),
                value: parts[1].to_string(),
            })
        }
    }

    fn parse_cmd_or_entrypoint(args: &str) -> Result<Vec<String>, UnikraftError> {
        let trimmed = args.trim();

        // JSON array format: ["executable", "param1", "param2"]
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let inner = &trimmed[1..trimmed.len() - 1];
            let parts: Vec<String> = inner
                .split(',')
                .map(|s| {
                    let s = s.trim();
                    // Remove quotes
                    if (s.starts_with('"') && s.ends_with('"'))
                        || (s.starts_with('\'') && s.ends_with('\''))
                    {
                        s[1..s.len() - 1].to_string()
                    } else {
                        s.to_string()
                    }
                })
                .collect();
            Ok(parts)
        } else {
            // Shell form - this should be rejected by validator but we parse it
            Ok(vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                trimmed.to_string(),
            ])
        }
    }

    fn parse_arg(args: &str) -> Result<Instruction, UnikraftError> {
        if let Some(eq_pos) = args.find('=') {
            Ok(Instruction::Arg {
                name: args[..eq_pos].to_string(),
                default: Some(args[eq_pos + 1..].to_string()),
            })
        } else {
            Ok(Instruction::Arg {
                name: args.to_string(),
                default: None,
            })
        }
    }

    fn parse_label(args: &str) -> Result<Instruction, UnikraftError> {
        if let Some(eq_pos) = args.find('=') {
            let key = args[..eq_pos].to_string();
            let value = args[eq_pos + 1..].trim_matches('"').to_string();
            Ok(Instruction::Label { key, value })
        } else {
            Err(UnikraftError::DockerfileParseError(
                "LABEL requires key=value format".to_string(),
            ))
        }
    }
}

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
        // Check for forbidden commands in raw content BEFORE parsing
        // This catches commands that the parser doesn't recognize
        let uppercase_content = content.to_uppercase();
        for forbidden in &self.forbidden_commands {
            // Check for command at start of line with space after
            let pattern_space = format!("{} ", forbidden);
            let pattern_newline_space = format!("\n{} ", forbidden);
            // Check if content starts with the command or has it after a newline
            if uppercase_content.starts_with(&pattern_space)
                || uppercase_content.contains(&pattern_newline_space)
            {
                return Err(UnikraftError::UnsupportedCommand {
                    command: forbidden.to_string(),
                    reason: "This command is not compatible with unikernels".to_string(),
                });
            }
        }

        let instructions = DockerfileParser::parse(content)?;

        // Extract and validate FROM
        let from_image = instructions
            .iter()
            .find_map(|i| match i {
                Instruction::From(img) => Some(img.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                UnikraftError::DockerfileParseError("Missing FROM instruction".to_string())
            })?;

        let runtime = self.validate_base_image(&from_image)?;

        // Validate RUN commands
        for instruction in &instructions {
            if let Instruction::Run(cmd) = instruction {
                self.validate_run_command(cmd)?;
            }
        }

        // Extract entrypoint (prefer ENTRYPOINT over CMD)
        let entrypoint = instructions
            .iter()
            .rev()
            .find_map(|i| match i {
                Instruction::Entrypoint(args) if !args.is_empty() => Some(args.clone()),
                Instruction::Cmd(args) if !args.is_empty() => Some(args.clone()),
                _ => None,
            })
            .unwrap_or_default();

        // Validate CMD/ENTRYPOINT is in exec form (not shell form)
        for instruction in &instructions {
            match instruction {
                Instruction::Cmd(args) | Instruction::Entrypoint(args) => {
                    if args.len() >= 3 && args[0] == "/bin/sh" && args[1] == "-c" {
                        return Err(UnikraftError::UnsupportedCommand {
                            command: "CMD/ENTRYPOINT".to_string(),
                            reason: "Shell form is not supported. Use exec form: [\"executable\", \"arg1\"]".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }

        Ok(ValidatedDockerfile {
            content: content.to_string(),
            runtime,
            entrypoint,
        })
    }

    fn validate_base_image(&self, image: &str) -> Result<Runtime, UnikraftError> {
        // Strip any tag or digest for comparison
        let image_name = image.split(':').next().unwrap_or(image);
        let image_name = image_name.split('@').next().unwrap_or(image_name);

        match image_name {
            "graphene/node" => Ok(Runtime::Node20),
            _ => Err(UnikraftError::UnsupportedBaseImage(format!(
                "Base image '{}' is not supported. Use one of: graphene/node:20",
                image
            ))),
        }
    }

    fn validate_run_command(&self, cmd: &str) -> Result<(), UnikraftError> {
        let cmd_lower = cmd.to_lowercase();

        // Check if the command matches any allowed pattern
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
    fn test_parse_simple_dockerfile() {
        let dockerfile = r#"
FROM graphene/node:20
WORKDIR /app
COPY package.json .
RUN npm install
COPY . .
CMD ["node", "index.js"]
"#;

        let instructions = DockerfileParser::parse(dockerfile).unwrap();
        assert_eq!(instructions.len(), 6);

        assert!(matches!(&instructions[0], Instruction::From(img) if img == "graphene/node:20"));
        assert!(matches!(&instructions[1], Instruction::Workdir(dir) if dir == "/app"));
        assert!(
            matches!(&instructions[5], Instruction::Cmd(args) if args == &["node", "index.js"])
        );
    }

    #[test]
    fn test_parse_env_formats() {
        let dockerfile = r#"
FROM graphene/node:20
ENV NODE_ENV=production
ENV PORT 3000
CMD ["node", "index.js"]
"#;

        let instructions = DockerfileParser::parse(dockerfile).unwrap();

        assert!(matches!(
            &instructions[1],
            Instruction::Env { key, value } if key == "NODE_ENV" && value == "production"
        ));
        assert!(matches!(
            &instructions[2],
            Instruction::Env { key, value } if key == "PORT" && value == "3000"
        ));
    }

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
        assert!(result.is_ok());

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
                if command == "CMD/ENTRYPOINT" && reason.contains("Shell form")
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

        let instructions = DockerfileParser::parse(dockerfile).unwrap();
        assert!(matches!(
            &instructions[1],
            Instruction::Run(cmd) if cmd.contains("npm install") && cmd.contains("--production")
        ));
    }

    #[test]
    fn test_parse_arg_with_default() {
        let dockerfile = r#"
FROM graphene/node:20
ARG NODE_ENV=production
ARG VERSION
CMD ["node", "index.js"]
"#;

        let instructions = DockerfileParser::parse(dockerfile).unwrap();

        assert!(matches!(
            &instructions[1],
            Instruction::Arg { name, default: Some(val) } if name == "NODE_ENV" && val == "production"
        ));
        assert!(matches!(
            &instructions[2],
            Instruction::Arg { name, default: None } if name == "VERSION"
        ));
    }
}
