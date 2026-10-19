use crossbeam_channel::{unbounded, Receiver, Sender};
use std::ops::Range;
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    C,
    Rust,
    Python,
}

impl Language {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Option<Self> {
        let ext = path.as_ref().extension()?.to_str()?;
        Self::from_extension(ext)
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "c" | "h" | "cpp" | "hpp" | "cc" | "cxx" => Some(Language::C),
            "rs" => Some(Language::Rust),
            "py" => Some(Language::Python),
            _ => None,
        }
    }

    fn tree_sitter_language(&self) -> tree_sitter::Language {
        match self {
            Language::C => tree_sitter_c::LANGUAGE.into(),
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenType {
    Keyword,
    Function,
    Type,
    String,
    Number,
    Comment,
    Operator,
    Macro,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighlightToken {
    pub start_byte: usize,
    pub end_byte: usize,
    pub token_type: TokenType,
}

pub struct SyntaxHighlighter {
    language: Language,
    parser: Parser,
    tree: Option<Tree>,
}

impl SyntaxHighlighter {
    pub fn new(language: Language) -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&language.tree_sitter_language())
            .map_err(|e| format!("Tree-sitter language error: {:?}", e))?;

        Ok(Self {
            language,
            parser,
            tree: None,
        })
    }

    pub fn update(&mut self, text: &str) {
        self.tree = self.parser.parse(text, None);
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn highlight_range(
        &self,
        start_byte: usize,
        end_byte: usize,
        text: &str,
    ) -> Vec<HighlightToken> {
        let mut tokens = Vec::new();
        if let Some(ref tree) = self.tree {
            let root = tree.root_node();
            self.collect_tokens(root, start_byte, end_byte, text, &mut tokens);
        }
        tokens
    }

    fn collect_tokens(
        &self,
        node: Node,
        start_byte: usize,
        end_byte: usize,
        text: &str,
        tokens: &mut Vec<HighlightToken>,
    ) {
        let node_start = node.start_byte();
        let node_end = node.end_byte();

        if node_end <= start_byte || node_start >= end_byte {
            return;
        }

        let kind = node.kind();
        let token_type = self.classify_node(kind, node, text);

        if let Some(t_type) = token_type {
            // Leaf node or token to color
            if node.child_count() == 0
                || t_type == TokenType::Comment
                || t_type == TokenType::String
                || t_type == TokenType::Macro
            {
                tokens.push(HighlightToken {
                    start_byte: node_start,
                    end_byte: node_end,
                    token_type: t_type,
                });
                return;
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_tokens(child, start_byte, end_byte, text, tokens);
        }
    }

    fn classify_node(&self, kind: &str, node: Node, text: &str) -> Option<TokenType> {
        match self.language {
            Language::C => classify_c_node(kind, node, text),
            Language::Rust => classify_rust_node(kind, node, text),
            Language::Python => classify_python_node(kind, node, text),
        }
    }
}

fn classify_c_node(kind: &str, node: Node, text: &str) -> Option<TokenType> {
    match kind {
        "primitive_type" | "type_identifier" | "sized_type_specifier" => Some(TokenType::Type),
        "string_literal" | "system_lib_string" | "char_literal" => Some(TokenType::String),
        "number_literal" => Some(TokenType::Number),
        "comment" => Some(TokenType::Comment),
        "preproc_include"
        | "preproc_def"
        | "preproc_function_def"
        | "preproc_ifdef"
        | "preproc_if" => Some(TokenType::Macro),
        "identifier" => {
            if let Some(parent) = node.parent() {
                if parent.kind() == "call_expression" || parent.kind() == "function_declarator" {
                    return Some(TokenType::Function);
                }
            }
            None
        }
        _ => {
            let slice = &text[node.start_byte()..node.end_byte().min(text.len())];
            match slice {
                "if" | "else" | "for" | "while" | "do" | "return" | "break" | "continue"
                | "switch" | "case" | "default" | "goto" | "struct" | "union" | "enum"
                | "typedef" | "static" | "extern" | "const" | "volatile" | "inline" | "sizeof" => {
                    Some(TokenType::Keyword)
                }
                _ => None,
            }
        }
    }
}

fn classify_rust_node(kind: &str, node: Node, text: &str) -> Option<TokenType> {
    match kind {
        "primitive_type" | "type_identifier" => Some(TokenType::Type),
        "string_literal" | "raw_string_literal" | "char_literal" => Some(TokenType::String),
        "integer_literal" | "float_literal" | "boolean_literal" => Some(TokenType::Number),
        "line_comment" | "block_comment" => Some(TokenType::Comment),
        "identifier" => {
            if let Some(parent) = node.parent() {
                if parent.kind() == "macro_invocation" {
                    return Some(TokenType::Macro);
                }
                if parent.kind() == "call_expression" || parent.kind() == "function_item" {
                    return Some(TokenType::Function);
                }
            }
            None
        }
        _ => {
            let slice = &text[node.start_byte()..node.end_byte().min(text.len())];
            match slice {
                "fn" | "let" | "mut" | "pub" | "use" | "mod" | "struct" | "enum" | "impl"
                | "trait" | "match" | "if" | "else" | "for" | "while" | "loop" | "return"
                | "break" | "continue" | "const" | "static" | "async" | "await" | "where"
                | "as" | "in" | "ref" | "type" | "unsafe" | "self" | "Self" => {
                    Some(TokenType::Keyword)
                }
                _ => None,
            }
        }
    }
}

fn classify_python_node(kind: &str, node: Node, text: &str) -> Option<TokenType> {
    match kind {
        "string" => Some(TokenType::String),
        "integer" | "float" => Some(TokenType::Number),
        "comment" => Some(TokenType::Comment),
        "identifier" => {
            if let Some(parent) = node.parent() {
                if parent.kind() == "call" || parent.kind() == "function_definition" {
                    return Some(TokenType::Function);
                }
            }
            None
        }
        _ => {
            let slice = &text[node.start_byte()..node.end_byte().min(text.len())];
            match slice {
                "def" | "class" | "import" | "from" | "as" | "if" | "elif" | "else" | "for"
                | "while" | "return" | "yield" | "break" | "continue" | "try" | "except"
                | "finally" | "raise" | "with" | "pass" | "lambda" | "in" | "is" | "not"
                | "and" | "or" | "None" | "True" | "False" => Some(TokenType::Keyword),
                _ => None,
            }
        }
    }
}

// Background syntax worker
pub struct SyntaxRequest {
    pub buffer_id: usize,
    pub revision: u64,
    pub text: String,
    pub language: Language,
    pub visible_range: Range<usize>,
}

pub struct SyntaxResponse {
    pub buffer_id: usize,
    pub revision: u64,
    pub tokens: Vec<HighlightToken>,
}

pub struct SyntaxWorker {
    req_tx: Sender<SyntaxRequest>,
    res_rx: Receiver<SyntaxResponse>,
}

impl SyntaxWorker {
    pub fn spawn() -> Self {
        let (req_tx, req_rx) = unbounded::<SyntaxRequest>();
        let (res_tx, res_rx) = unbounded::<SyntaxResponse>();

        std::thread::spawn(move || {
            let mut highlighters: std::collections::HashMap<Language, SyntaxHighlighter> =
                std::collections::HashMap::new();

            while let Ok(req) = req_rx.recv() {
                let highlighter = highlighters
                    .entry(req.language)
                    .or_insert_with(|| SyntaxHighlighter::new(req.language).unwrap());

                highlighter.update(&req.text);
                let tokens = highlighter.highlight_range(
                    req.visible_range.start,
                    req.visible_range.end,
                    &req.text,
                );

                let _ = res_tx.send(SyntaxResponse {
                    buffer_id: req.buffer_id,
                    revision: req.revision,
                    tokens,
                });
            }
        });

        Self { req_tx, res_rx }
    }

    pub fn send_request(&self, req: SyntaxRequest) {
        let _ = self.req_tx.send(req);
    }

    pub fn try_recv_response(&self) -> Option<SyntaxResponse> {
        self.res_rx.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_highlighting() {
        let code =
            "#include <stdio.h>\nint main() {\n    // comment\n    int x = 42;\n    return 0;\n}";
        let mut highlighter = SyntaxHighlighter::new(Language::C).unwrap();
        highlighter.update(code);
        let tokens = highlighter.highlight_range(0, code.len(), code);

        assert!(tokens.iter().any(|t| t.token_type == TokenType::Macro));
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Type));
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Function));
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Comment));
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Number));
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Keyword));
    }

    #[test]
    fn test_rust_highlighting() {
        let code = "fn main() {\n    // test comment\n    let val: u32 = 123;\n    println!(\"hello\");\n}";
        let mut highlighter = SyntaxHighlighter::new(Language::Rust).unwrap();
        highlighter.update(code);
        let tokens = highlighter.highlight_range(0, code.len(), code);

        assert!(tokens.iter().any(|t| t.token_type == TokenType::Keyword));
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Function));
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Comment));
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Type));
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Number));
        assert!(tokens.iter().any(|t| t.token_type == TokenType::String));
    }

    #[test]
    fn test_syntax_worker_background() {
        let worker = SyntaxWorker::spawn();
        worker.send_request(SyntaxRequest {
            buffer_id: 1,
            revision: 1,
            text: "fn add(a: i32, b: i32) -> i32 { a + b }".to_string(),
            language: Language::Rust,
            visible_range: 0..40,
        });

        let mut received = None;
        for _ in 0..50 {
            if let Some(res) = worker.try_recv_response() {
                received = Some(res);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        assert!(received.is_some());
        let res = received.unwrap();
        assert_eq!(res.buffer_id, 1);
        assert!(!res.tokens.is_empty());
    }
}
