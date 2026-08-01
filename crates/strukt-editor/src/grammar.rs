use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GrammarDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub extensions: &'static [&'static str],
    pub exact_file_names: &'static [&'static str],
    pub iced_token: &'static str,
}

const RUST: GrammarDescriptor = grammar("rust", "Rust", &["rs"], &[], "Rust");
const JAVASCRIPT: GrammarDescriptor = grammar(
    "javascript",
    "JavaScript",
    &["js", "jsx", "mjs", "cjs"],
    &[],
    "JavaScript",
);
const TYPESCRIPT: GrammarDescriptor = grammar(
    "typescript",
    "TypeScript",
    &["ts", "tsx", "mts", "cts"],
    &[],
    "TypeScript",
);
const PYTHON: GrammarDescriptor = grammar("python", "Python", &["py", "pyi"], &[], "Python");
const JSON: GrammarDescriptor = grammar(
    "json",
    "JSON",
    &["json", "jsonc"],
    &["package-lock.json", "tsconfig.json"],
    "JSON",
);
const TOML: GrammarDescriptor = grammar("toml", "TOML", &["toml"], &["Cargo.lock"], "TOML");
const MARKDOWN: GrammarDescriptor = grammar(
    "markdown",
    "Markdown",
    &["md", "markdown", "mdx"],
    &["README", "CHANGELOG", "CONTRIBUTING", "LICENSE"],
    "Markdown",
);
const SHELL: GrammarDescriptor = grammar(
    "shell",
    "Shell",
    &["sh", "bash", "zsh", "fish"],
    &[".bashrc", ".bash_profile", ".zshrc", ".profile"],
    "Bourne Again Shell (bash)",
);
const YAML: GrammarDescriptor = grammar(
    "yaml",
    "YAML",
    &["yaml", "yml"],
    &["docker-compose.yml", "docker-compose.yaml"],
    "YAML",
);
const HTML: GrammarDescriptor = grammar("html", "HTML", &["html", "htm"], &[], "HTML");
const CSS: GrammarDescriptor = grammar("css", "CSS", &["css", "scss", "sass", "less"], &[], "CSS");
pub const PLAIN_TEXT_GRAMMAR: GrammarDescriptor =
    grammar("plain-text", "Plain Text", &["txt"], &[], "Plain Text");

const GRAMMARS: [GrammarDescriptor; 12] = [
    RUST,
    JAVASCRIPT,
    TYPESCRIPT,
    PYTHON,
    JSON,
    TOML,
    MARKDOWN,
    SHELL,
    YAML,
    HTML,
    CSS,
    PLAIN_TEXT_GRAMMAR,
];

const fn grammar(
    id: &'static str,
    display_name: &'static str,
    extensions: &'static [&'static str],
    exact_file_names: &'static [&'static str],
    iced_token: &'static str,
) -> GrammarDescriptor {
    GrammarDescriptor {
        id,
        display_name,
        extensions,
        exact_file_names,
        iced_token,
    }
}

pub struct GrammarRegistry;

impl GrammarRegistry {
    #[must_use]
    pub const fn all() -> &'static [GrammarDescriptor] {
        &GRAMMARS
    }

    #[must_use]
    pub fn by_id(id: &str) -> Option<&'static GrammarDescriptor> {
        GRAMMARS.iter().find(|grammar| grammar.id == id)
    }

    #[must_use]
    pub fn detect(path: &Path, override_id: Option<&str>) -> &'static GrammarDescriptor {
        if let Some(id) = override_id {
            return Self::by_id(id).unwrap_or(&PLAIN_TEXT_GRAMMAR);
        }
        if let Some(name) = path.file_name().and_then(|name| name.to_str())
            && let Some(grammar) = GRAMMARS
                .iter()
                .find(|grammar| grammar.exact_file_names.contains(&name))
        {
            return grammar;
        }
        if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
            return GRAMMARS
                .iter()
                .find(|grammar| {
                    grammar
                        .extensions
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
                })
                .unwrap_or(&PLAIN_TEXT_GRAMMAR);
        }
        &PLAIN_TEXT_GRAMMAR
    }
}
