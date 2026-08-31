use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use clap::{error::ErrorKind, CommandFactory, Parser};
use syn::visit::Visit;

use super::Cli;

#[derive(Debug)]
struct Guidance {
    source: String,
    line: usize,
    invocation: String,
    parse_full: bool,
}

fn collect_files(dir: &Path, extension: &str, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("guidance directory is readable") {
        let path = entry.expect("guidance entry is readable").path();
        if path.is_dir() {
            collect_files(&path, extension, files);
        } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
            files.push(path);
        }
    }
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for extension in ["md", "mdx", "txt"] {
        collect_files(dir, extension, files);
    }
}

fn source_name(path: &Path) -> String {
    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap_or(path)
        .display()
        .to_string()
}

fn is_command_start(candidate: &str) -> bool {
    let Some(first) = candidate
        .strip_prefix("floo ")
        .and_then(|rest| rest.split_whitespace().next())
    else {
        return false;
    };
    let first =
        first.trim_matches(|character: char| !character.is_alphanumeric() && character != '-');
    first.starts_with("--")
        || matches!(first, "-h" | "-V")
        || Cli::command().get_subcommands().any(|command| {
            command.get_name() == first || command.get_all_aliases().any(|alias| alias == first)
        })
}

/// `/floo …` at a word boundary is a pull request comment command, not CLI
/// syntax. floo's PR previews are driven by `/floo preview on` / `/floo preview
/// off`, and `preview` is a compatibility alias of the `previews` subcommand, so
/// without this the comment command cannot be documented in first-party guidance
/// at all. A slash that follows a path character (`bin/floo deploy`) is still a
/// real invocation and stays validated.
fn is_pr_comment_command(text: &str, start: usize) -> bool {
    let Some(before_slash) = text
        .get(..start)
        .and_then(|prefix| prefix.strip_suffix('/'))
    else {
        return false;
    };
    !before_slash.chars().last().is_some_and(|character| {
        character.is_alphanumeric() || matches!(character, '.' | '-' | '_' | '~')
    })
}

fn push_occurrences(
    text: &str,
    source: &str,
    line: usize,
    parse_full: bool,
    output: &mut Vec<Guidance>,
) {
    let mut remainder = text;
    while let Some(start) = remainder.find("floo ") {
        if is_pr_comment_command(remainder, start) {
            remainder = &remainder[start + "floo ".len()..];
            continue;
        }
        let prefix = &remainder[..start];
        let delimiter = prefix
            .chars()
            .last()
            .filter(|character| matches!(character, '`' | '\''));
        // Preserve the separating whitespace: the suffixes below deliberately
        // include it so words such as "rerun" do not match "run ".
        let normalized_prefix = prefix.to_ascii_lowercase();
        let prose_guidance = [
            "run ",
            "use ",
            "try ",
            "with ",
            "then ",
            "command ",
            "invocation ",
            "rerun ",
            "execute ",
        ]
        .iter()
        .any(|suffix| normalized_prefix.ends_with(suffix));
        let line_command = text.trim_start().starts_with("floo ");
        let path_command = prefix.ends_with('/');
        if delimiter.is_none() && !prose_guidance && !line_command && !path_command {
            remainder = &remainder[start + "floo ".len()..];
            continue;
        }
        let occurrence = &remainder[start..];
        let mut end = occurrence[5..]
            .find("floo ")
            .map_or(occurrence.len(), |offset| offset + 5);
        if let Some(delimiter) = delimiter {
            if let Some(offset) = occurrence[5..].find(delimiter) {
                end = end.min(offset + 5);
            }
        }
        let invocation = occurrence[..end]
            .trim()
            .trim_matches(|character| matches!(character, '`' | '\''))
            .to_string();
        if is_command_start(&invocation) {
            output.push(Guidance {
                source: source.to_string(),
                line,
                invocation,
                parse_full,
            });
        }
        remainder = &occurrence[end..];
    }
}

fn markdown_guidance(path: &Path) -> Vec<Guidance> {
    let source = source_name(path);
    let body = fs::read_to_string(path).expect("guidance source is UTF-8");
    let lines: Vec<&str> = body.lines().collect();
    let mut output = Vec::new();
    let mut in_fence = false;
    let mut index = 0;

    while index < lines.len() {
        let line_number = index + 1;
        let mut line = lines[index].to_string();
        while line.trim_end().ends_with('\\') && index + 1 < lines.len() {
            line.truncate(line.trim_end().len() - 1);
            index += 1;
            line.push(' ');
            line.push_str(lines[index].trim());
        }

        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            index += 1;
            continue;
        }

        let indented_command = trimmed.starts_with("floo ")
            && line.len() > trimmed.len()
            && !line.contains('—')
            && !line.contains(" - ");
        // Scan every line, not only code-formatted spans. A stale command in
        // prose is still user guidance, even when an author forgot backticks.
        // Backtick and quote delimiters keep inline invocations bounded.
        push_occurrences(
            &line,
            &source,
            line_number,
            in_fence || indented_command,
            &mut output,
        );
        index += 1;
    }
    output
}

#[derive(Default)]
struct StringVisitor {
    values: Vec<String>,
}

impl<'ast> Visit<'ast> for StringVisitor {
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.values.push(literal.value());
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if module.ident != "tests" {
            syn::visit::visit_item_mod(self, module);
        }
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if !attribute.path().is_ident("doc") {
            syn::visit::visit_attribute(self, attribute);
        }
    }
}

fn rust_guidance(path: &Path) -> Vec<Guidance> {
    let source = source_name(path);
    let body = fs::read_to_string(path).expect("Rust source is UTF-8");
    let syntax =
        syn::parse_file(&body).unwrap_or_else(|error| panic!("failed to parse {source}: {error}"));
    let mut visitor = StringVisitor::default();
    visitor.visit_file(&syntax);

    let mut output = Vec::new();
    for value in visitor.values {
        for (line, text) in value.lines().enumerate() {
            let trimmed = text.trim();
            let parse_full = trimmed.starts_with("floo ")
                && text.len() > trimmed.len()
                && !text.contains('—')
                && !text.contains(" - ");
            push_occurrences(text, &source, line + 1, parse_full, &mut output);
        }
    }
    output
}

fn command_column(invocation: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    let mut spaces = 0;
    let mut spaces_start = 0;
    for (index, character) in invocation.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = if quote == Some(character) {
                None
            } else if quote.is_none() {
                Some(character)
            } else {
                quote
            };
        }
        if quote.is_none() && character.is_whitespace() {
            if spaces == 0 {
                spaces_start = index;
            }
            spaces += 1;
            let remainder = invocation[index..].trim_start();
            if spaces >= 3
                || remainder.starts_with('#')
                || remainder.starts_with('—')
                || remainder.starts_with("- ")
                || remainder.starts_with('|')
                || remainder.starts_with("&&")
                || remainder.starts_with('>')
            {
                return &invocation[..spaces_start];
            }
        } else {
            spaces = 0;
        }
    }
    invocation
}

fn normalized_argv(invocation: &str) -> Result<Vec<String>, String> {
    fn replace_placeholders(token: &str, previous: Option<&str>) -> String {
        let value = match previous {
            Some("--env") => "dev",
            Some("--role") => "member",
            Some("--category") => "general",
            _ if matches!(token, "<type>" | "<managed>") => "postgres",
            _ => "guidance-value",
        };
        let mut token = token.to_string();
        for (open, close) in [('<', '>'), ('{', '}')] {
            while let Some(start) = token.find(open) {
                let Some(relative_end) = token[start..].find(close) else {
                    break;
                };
                token.replace_range(start..=start + relative_end, value);
            }
        }
        token
    }

    let tokens = shlex::split(command_column(invocation))
        .ok_or_else(|| "invalid shell quoting".to_string())?;
    let mut argv = Vec::new();
    for token in tokens {
        let token = token.trim_matches(|character: char| {
            matches!(
                character,
                '`' | '\'' | '"' | '(' | ')' | '[' | ']' | ',' | '.' | ';' | '*'
            )
        });
        if token.is_empty()
            || matches!(token, "|" | "||" | "&&" | "#" | ">" | ">>")
            || token.starts_with("2>")
        {
            break;
        }
        argv.push(replace_placeholders(token, argv.last().map(String::as_str)));
    }
    if argv.first().map(String::as_str) != Some("floo") {
        return Err("normalized invocation does not start with `floo`".to_string());
    }
    Ok(argv)
}

fn validate_path_and_options(invocation: &str) -> Result<(), String> {
    if matches!(
        command_column(invocation),
        "floo --help" | "floo --version" | "floo -V" | "floo <command>" | "floo <path>"
    ) || invocation.starts_with("floo deploy ...")
    {
        return Ok(());
    }

    let argv = normalized_argv(invocation)?;
    let mut command = Cli::command();
    let mut path = Vec::new();
    let mut options = HashSet::new();
    let mut aliases = HashMap::new();

    let record_options = |command: &clap::Command,
                          options: &mut HashSet<String>,
                          aliases: &mut HashMap<String, String>| {
        for argument in command.get_arguments() {
            if let Some(long) = argument.get_long() {
                options.insert(long.to_string());
                if let Some(argument_aliases) = argument.get_all_aliases() {
                    aliases.extend(
                        argument_aliases
                            .into_iter()
                            .map(|alias| (alias.to_string(), long.to_string())),
                    );
                }
            }
        }
    };
    record_options(&command, &mut options, &mut aliases);
    options.extend(["help".to_string(), "version".to_string()]);

    for token in argv.iter().skip(1) {
        if token == "--" {
            break;
        }
        if let Some(option) = token.strip_prefix("--") {
            let option = option
                .split('=')
                .next()
                .unwrap_or(option)
                .trim_matches(|character: char| matches!(character, '`' | '\'' | '"' | '.' | ','));
            if let Some(canonical) = aliases.get(option) {
                return Err(format!(
                    "`--{option}` is a compatibility alias; use `--{canonical}`"
                ));
            }
            if !options.contains(option) {
                return Err(format!("unknown option `--{option}`"));
            }
            continue;
        }
        let exact_subcommand = {
            command
                .get_subcommands()
                .find(|candidate| candidate.get_name() == token)
                .cloned()
        };
        if let Some(subcommand) = exact_subcommand {
            path.push(token.clone());
            command = subcommand;
            record_options(&command, &mut options, &mut aliases);
            continue;
        }
        if let Some(canonical) = command.get_subcommands().find_map(|candidate| {
            candidate
                .get_all_aliases()
                .any(|alias| alias == token)
                .then(|| candidate.get_name())
        }) {
            return Err(format!(
                "`{token}` is a compatibility alias; use canonical `{canonical}`"
            ));
        }
        if command.has_subcommands() {
            return Err(format!(
                "unknown subcommand `{token}` below `floo {}`",
                path.join(" ")
            ));
        }
    }
    if path.is_empty() {
        return Err("no canonical command path found".to_string());
    }
    Ok(())
}

#[test]
fn first_party_guidance_matches_clap() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut markdown = vec![root.join("README.md")];
    for directory in ["skills", "plugin/skills"] {
        collect_markdown_files(&root.join(directory), &mut markdown);
    }
    if let Some(external_root) = std::env::var_os("FLOO_EXTERNAL_GUIDANCE_DIR") {
        let external_root = PathBuf::from(external_root);
        assert!(
            external_root.is_dir(),
            "FLOO_EXTERNAL_GUIDANCE_DIR is not a directory: {}",
            external_root.display()
        );
        collect_markdown_files(&external_root, &mut markdown);
    }
    markdown.sort();
    markdown.dedup();

    let mut rust = Vec::new();
    collect_files(&root.join("src"), "rs", &mut rust);
    rust.retain(|path| !path.ends_with("guidance_tests.rs"));
    rust.sort();

    let mut guidance = Vec::new();
    for path in markdown {
        guidance.extend(markdown_guidance(&path));
    }
    for path in rust {
        guidance.extend(rust_guidance(&path));
    }

    let mut failures = Vec::new();
    let mut full_examples = 0;
    for item in &guidance {
        if let Err(reason) = validate_path_and_options(&item.invocation) {
            failures.push(format!(
                "{}:{} `{}`: {reason}",
                item.source, item.line, item.invocation
            ));
            continue;
        }
        if item.parse_full {
            let argv = match normalized_argv(&item.invocation) {
                Ok(argv) => argv,
                Err(reason) => {
                    failures.push(format!(
                        "{}:{} `{}`: {reason}",
                        item.source, item.line, item.invocation
                    ));
                    continue;
                }
            };
            match Cli::try_parse_from(argv) {
                Ok(_) => full_examples += 1,
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                    ) =>
                {
                    full_examples += 1;
                }
                Err(error) => failures.push(format!(
                    "{}:{} `{}` parsed with unexpected {:?}",
                    item.source,
                    item.line,
                    item.invocation,
                    error.kind()
                )),
            }
        }
    }

    // These floors are ratchets against a corpus silently emptying, not targets.
    // The complete-example floor dropped from 50 to 15 when docs/offline moved
    // to floo-docs: this validator parses every example against the live Clap
    // tree, and the pages that left are now checked only by hand-maintained
    // regexes in floo-docs/scripts/check_docs.py. That is a real reduction in
    // what is mechanically verified. Raise this floor if the bundled corpus
    // grows; do not lower it to make a failure go away.
    assert!(
        guidance.len() > 100,
        "too few guidance references found: {}",
        guidance.len()
    );
    assert!(
        full_examples > 15,
        "too few complete examples found: {full_examples}"
    );
    assert!(
        failures.is_empty(),
        "first-party guidance contains stale CLI syntax:\n{}",
        failures.join("\n")
    );
}

#[test]
fn guidance_validator_rejects_stale_syntax() {
    let invocation = |suffix: &str| format!("{} {suffix}", "floo");
    assert!(validate_path_and_options(&invocation("apps status my-app")).is_err());
    assert!(validate_path_and_options(&invocation("redeploy --dry-run")).is_err());
    assert!(validate_path_and_options(&invocation("apps show my-app --not-real")).is_err());
    assert!(Cli::try_parse_from(normalized_argv(&invocation("apps show --app")).unwrap()).is_err());

    let mut prose = Vec::new();
    push_occurrences(
        "Before continuing, run floo apps status my-app to inspect it.",
        "negative-fixture.md",
        1,
        false,
        &mut prose,
    );
    assert_eq!(prose.len(), 1);
    assert!(validate_path_and_options(&prose[0].invocation).is_err());
}

#[test]
fn guidance_validator_skips_pr_comment_commands() {
    // `/floo preview on` is a pull request comment command. `preview` is a
    // compatibility alias of `previews`, so treating it as CLI syntax would
    // make the comment command undocumentable in first-party guidance.
    let mut comment = Vec::new();
    push_occurrences(
        "Comment `/floo preview on` on the pull request to opt it in.",
        "comment-fixture.md",
        1,
        false,
        &mut comment,
    );
    assert!(
        comment.is_empty(),
        "pull request comment commands must not be validated as CLI syntax: {comment:?}"
    );

    // A slash that continues a path is still a real invocation.
    let mut path_invocation = Vec::new();
    push_occurrences(
        "Run /usr/local/bin/floo apps status my-app to inspect it.",
        "path-fixture.md",
        1,
        false,
        &mut path_invocation,
    );
    assert_eq!(path_invocation.len(), 1);
    assert!(validate_path_and_options(&path_invocation[0].invocation).is_err());

    // A bare `floo …` next to a slash-delimited sentence is unaffected.
    let mut plain = Vec::new();
    push_occurrences(
        "Deploy/promote, then run floo apps status my-app.",
        "plain-fixture.md",
        1,
        false,
        &mut plain,
    );
    assert_eq!(plain.len(), 1);
}

#[test]
fn installed_and_plugin_routers_match() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let installed = fs::read_to_string(root.join("skills/floo/SKILL.md")).unwrap();
    let plugin = fs::read_to_string(root.join("plugin/skills/floo/SKILL.md")).unwrap();
    assert_eq!(
        installed, plugin,
        "the installed and plugin floo routers must remain one policy"
    );
}
