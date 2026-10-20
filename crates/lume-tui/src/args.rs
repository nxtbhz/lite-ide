use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Help,
    Version,
    BenchStartup(CliTargets),
    Run(CliTargets),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliTargets {
    pub workspace: Option<PathBuf>,
    pub files: Vec<PathBuf>,
}

/// Matches a simple wildcard pattern containing `*` and `?` against text.
pub fn matches_pattern(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut p_i, mut t_i) = (0, 0);
    let (mut star_p, mut star_t) = (None, 0);

    while t_i < t.len() {
        if p_i < p.len() && (p[p_i] == '?' || p[p_i] == t[t_i]) {
            p_i += 1;
            t_i += 1;
        } else if p_i < p.len() && p[p_i] == '*' {
            star_p = Some(p_i);
            p_i += 1;
            star_t = t_i;
        } else if let Some(sp) = star_p {
            p_i = sp + 1;
            star_t += 1;
            t_i = star_t;
        } else {
            return false;
        }
    }

    while p_i < p.len() && p[p_i] == '*' {
        p_i += 1;
    }

    p_i == p.len()
}

/// Expands glob patterns with `*` and `?` in path components without external dependencies.
/// If pattern does not contain wildcard characters, returns the path directly.
pub fn expand_glob(pattern: &str) -> Vec<PathBuf> {
    if !pattern.contains('*') && !pattern.contains('?') {
        return vec![PathBuf::from(pattern)];
    }

    let path = Path::new(pattern);
    let mut current_paths = vec![PathBuf::new()];

    for component in path.iter() {
        let comp_str = component.to_string_lossy();
        if comp_str.contains('*') || comp_str.contains('?') {
            let mut next_paths = Vec::new();
            for base in current_paths {
                let dir_to_read = if base.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    &base
                };

                if let Ok(entries) = std::fs::read_dir(dir_to_read) {
                    let mut matched = Vec::new();
                    for entry in entries.flatten() {
                        let file_name = entry.file_name();
                        let name_str = file_name.to_string_lossy();
                        // Ignore hidden files unless pattern explicitly starts with '.'
                        if name_str.starts_with('.') && !comp_str.starts_with('.') {
                            continue;
                        }
                        if matches_pattern(&comp_str, &name_str) {
                            if base.as_os_str().is_empty() {
                                matched.push(PathBuf::from(file_name));
                            } else {
                                matched.push(base.join(file_name));
                            }
                        }
                    }
                    matched.sort();
                    next_paths.extend(matched);
                }
            }
            current_paths = next_paths;
        } else {
            for base in &mut current_paths {
                if base.as_os_str().is_empty() {
                    *base = PathBuf::from(component);
                } else {
                    *base = base.join(component);
                }
            }
        }
    }

    current_paths
}

/// Parses command line arguments into a high-level CliCommand.
pub fn parse_cli_args<I, T>(args: I) -> CliCommand
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let all_args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();

    let mut flags = Vec::new();
    let mut raw_targets = Vec::new();

    for arg in all_args.iter().skip(1) {
        if arg.starts_with('-') && arg != "-" {
            flags.push(arg.as_str());
        } else {
            raw_targets.push(arg.as_str());
        }
    }

    if flags.contains(&"-h") || flags.contains(&"--help") {
        return CliCommand::Help;
    }

    if flags.contains(&"-v") || flags.contains(&"--version") {
        return CliCommand::Version;
    }

    let is_bench = flags.contains(&"--bench-startup");

    let mut expanded_paths = Vec::new();
    for target in raw_targets {
        let expanded = expand_glob(target);
        if expanded.is_empty() {
            if !target.contains('*') && !target.contains('?') {
                expanded_paths.push(PathBuf::from(target));
            }
        } else {
            expanded_paths.extend(expanded);
        }
    }

    let mut workspace = None;
    let mut files = Vec::new();

    for path in expanded_paths {
        let is_explicit_dir = path.is_dir()
            || path.to_string_lossy().ends_with('/')
            || path.to_string_lossy().ends_with('\\');

        if is_explicit_dir {
            if workspace.is_none() {
                workspace = Some(path);
            }
        } else {
            files.push(path);
        }
    }

    let targets = CliTargets { workspace, files };

    if is_bench {
        CliCommand::BenchStartup(targets)
    } else {
        CliCommand::Run(targets)
    }
}

pub fn print_help() {
    println!("lume 0.1.0");
    println!("Fast, lightweight terminal code editor.\n");
    println!("USAGE:");
    println!("    lume [OPTIONS] [PATH]...\n");
    println!("ARGS:");
    println!("    [PATH]...    File(s) or directory to open (e.g. `lume .`, `lume file.txt`, `lume src/*.rs`)\n");
    println!("OPTIONS:");
    println!("    -h, --help             Print help information");
    println!("    -v, --version          Print version information");
    println!("        --bench-startup    Benchmark cold startup time and exit");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_pattern() {
        assert!(matches_pattern("*.rs", "main.rs"));
        assert!(matches_pattern("*.rs", "lib.rs"));
        assert!(!matches_pattern("*.rs", "main.c"));
        assert!(!matches_pattern("*.rs", "main.rs.bak"));

        assert!(matches_pattern("file.*", "file.txt"));
        assert!(matches_pattern("file.*", "file."));
        assert!(!matches_pattern("file.*", "file"));
        assert!(!matches_pattern("file.*", "profile.txt"));

        assert!(matches_pattern("test_?_file", "test_1_file"));
        assert!(matches_pattern("test_?_file", "test_a_file"));
        assert!(!matches_pattern("test_?_file", "test_12_file"));

        assert!(matches_pattern("*", "anything"));
        assert!(matches_pattern("*", ""));
        assert!(matches_pattern("", ""));
        assert!(!matches_pattern("", "something"));

        assert!(matches_pattern("a*b*c", "abc"));
        assert!(matches_pattern("a*b*c", "a_middle_b_end_c"));
        assert!(!matches_pattern("a*b*c", "ab"));
    }

    #[test]
    fn test_expand_glob_in_repo() {
        let results = expand_glob("src/*.rs");
        assert!(!results.is_empty());
        assert!(results.iter().any(|p| p.ends_with("main.rs")));
        assert!(results.iter().any(|p| p.ends_with("args.rs")));

        let non_wildcard = expand_glob("Cargo.toml");
        assert_eq!(non_wildcard, vec![PathBuf::from("Cargo.toml")]);

        let nonexistent = expand_glob("nonexistent_glob_dir_xyz_123/*.foo");
        assert!(nonexistent.is_empty());
    }

    #[test]
    fn test_parse_cli_args() {
        // No args
        let cmd = parse_cli_args(["lume"]);
        assert_eq!(
            cmd,
            CliCommand::Run(CliTargets {
                workspace: None,
                files: vec![]
            })
        );

        // Help
        assert_eq!(parse_cli_args(["lume", "--help"]), CliCommand::Help);
        assert_eq!(parse_cli_args(["lume", "-h"]), CliCommand::Help);

        // Version
        assert_eq!(parse_cli_args(["lume", "--version"]), CliCommand::Version);
        assert_eq!(parse_cli_args(["lume", "-v"]), CliCommand::Version);

        // Bench startup
        assert_eq!(
            parse_cli_args(["lume", "--bench-startup"]),
            CliCommand::BenchStartup(CliTargets {
                workspace: None,
                files: vec![]
            })
        );

        // Directory path `.`
        let cmd = parse_cli_args(["lume", "."]);
        match cmd {
            CliCommand::Run(targets) => {
                assert_eq!(targets.workspace, Some(PathBuf::from(".")));
                assert!(targets.files.is_empty());
            }
            _ => panic!("Expected Run command"),
        }

        // Single file
        let cmd = parse_cli_args(["lume", "Cargo.toml"]);
        match cmd {
            CliCommand::Run(targets) => {
                assert_eq!(targets.workspace, None);
                assert_eq!(targets.files, vec![PathBuf::from("Cargo.toml")]);
            }
            _ => panic!("Expected Run command"),
        }

        // Multiple files
        let cmd = parse_cli_args(["lume", "Cargo.toml", "Cargo.lock"]);
        match cmd {
            CliCommand::Run(targets) => {
                assert_eq!(targets.workspace, None);
                assert_eq!(
                    targets.files,
                    vec![PathBuf::from("Cargo.toml"), PathBuf::from("Cargo.lock")]
                );
            }
            _ => panic!("Expected Run command"),
        }

        // Directory + file
        let cmd = parse_cli_args(["lume", ".", "Cargo.toml"]);
        match cmd {
            CliCommand::Run(targets) => {
                assert_eq!(targets.workspace, Some(PathBuf::from(".")));
                assert_eq!(targets.files, vec![PathBuf::from("Cargo.toml")]);
            }
            _ => panic!("Expected Run command"),
        }
    }
}
