//! The `command` config value: a program line that is either run through the shell or split
//! on spaces (`direct:` prefix), like Ghostty's `config/command.zig`.

/// A command to run in a terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Already split; no shell is involved.
    Direct(Vec<String>),
    /// A line for the shell to expand.
    Shell(String),
}

impl Command {
    /// Parses a config value: `direct:nvim a b` splits on spaces, `shell:` or no prefix keeps
    /// the line for the shell.
    pub fn parse(input: &str) -> Option<Command> {
        let trimmed = input.trim_matches(' ');
        if trimmed.is_empty() {
            return None;
        }
        if let Some((prefix, rest)) = trimmed.split_once(':') {
            let rest = rest.trim_matches(' ');
            match prefix {
                "direct" => {
                    let args: Vec<String> = rest.split(' ').map(str::to_string).collect();
                    return (!rest.is_empty()).then_some(Command::Direct(args));
                }
                "shell" => return (!rest.is_empty()).then(|| Command::Shell(rest.to_string())),
                _ => {}
            }
        }
        Some(Command::Shell(trimmed.to_string()))
    }

    /// The program and arguments to execute. A shell line without anything the shell would
    /// expand is split directly, so the program can be recognised for shell integration;
    /// otherwise it runs as `/bin/sh -c "exec <line>"`.
    pub fn argv(&self) -> Vec<String> {
        match self {
            Command::Direct(args) => args.clone(),
            Command::Shell(line) => split_plain_words(line)
                .unwrap_or_else(|| vec!["/bin/sh".into(), "-c".into(), format!("exec {line}")]),
        }
    }
}

/// Splits `line` into words with quotes and backslashes honoured, or returns `None` when it
/// uses a shell feature (variables, globs, pipes, redirects, `VAR=value` prefixes...).
fn split_plain_words(line: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            c @ ('"' | '\\') => word.push(c),
                            other => {
                                word.push('\\');
                                word.push(other);
                            }
                        },
                        '$' | '`' => return None,
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.push(chars.next()?);
            }
            '|' | '&' | ';' | '<' | '>' | '(' | ')' | '$' | '`' | '*' | '?' | '[' | ']' | '{' | '}' | '!'
            | '\n' => return None,
            '#' | '~' if !in_word => return None,
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    let first = words.first()?;
    let assignment = first.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    });
    (!assignment).then_some(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Option<Vec<String>> {
        split_plain_words(line)
    }

    #[test]
    fn parses_prefixes() {
        assert_eq!(Command::parse("  "), None);
        assert_eq!(Command::parse("direct: nvim a b "), Some(Command::Direct(vec!["nvim".into(), "a".into(), "b".into()])));
        assert_eq!(Command::parse("shell:fish -l"), Some(Command::Shell("fish -l".into())));
        assert_eq!(Command::parse("fish -l"), Some(Command::Shell("fish -l".into())));
        assert_eq!(Command::parse("direct:"), None);
    }

    #[test]
    fn plain_shell_lines_split_into_words() {
        assert_eq!(words("bash --norc -l"), Some(vec!["bash".into(), "--norc".into(), "-l".into()]));
        assert_eq!(words("\"/a b/bash\" -i"), Some(vec!["/a b/bash".into(), "-i".into()]));
        assert_eq!(words("echo 'it''s' x\\ y"), Some(vec!["echo".into(), "its".into(), "x y".into()]));
        assert_eq!(words("a=b#c"), None);
        assert_eq!(words("path/with#hash"), Some(vec!["path/with#hash".into()]));
    }

    #[test]
    fn shell_features_fall_back_to_sh() {
        for line in ["ls | less", "echo $HOME", "FOO=1 vim", "vim *.rs", "cd ~ && ls", "echo 'unterminated"] {
            assert_eq!(
                Command::Shell(line.into()).argv(),
                vec!["/bin/sh".to_string(), "-c".into(), format!("exec {line}")],
                "{line}"
            );
        }
        assert_eq!(Command::Shell("zsh -l".into()).argv(), vec!["zsh".to_string(), "-l".into()]);
    }
}
