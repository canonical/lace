// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Julian Andres Klode <julian.klode@canonical.com>
//! GRUB configuration file parser.
//!
//! This module provides a parser for GRUB2 configuration files, extracting
//! menu entries with their kernel, initrd, and command line parameters.
//!
//! # Implementation
//!
//! The parser uses a token-based approach with a simple tokenizer that recognizes:
//! - Words (including quoted strings with single `'` or double `"` quotes)
//! - Braces (`{` and `}`)
//! - Separators (newlines `;`)
//! - Comments (lines starting with `#`)
//!
//! # Limitations
//!
//! This parser is intentionally simplified and does not implement full GRUB2
//! script parsing. Key limitations:
//!
//! - **No variable expansion**: Variables like `$root` or `${prefix}` are treated
//!   as literal strings and not expanded.
//! - **No command substitution**: Backticks and `$()` are not evaluated.
//! - **No conditionals**: `if/then/else/fi` blocks are skipped over but not evaluated.
//! - **No loops**: `for` and `while` constructs are ignored.
//! - **Limited quote handling**: Escape sequences in single quotes are not processed;
//!   only double quotes support backslash escapes.
//! - **No function definitions**: Custom GRUB functions are not recognized.
//! - **Brace matching only**: Nested braces are counted to find block boundaries,
//!   but the contents are not fully parsed unless they contain `menuentry` or `submenu`.
//!
//! The parser focuses on extracting the essential information needed for boot:
//! menu entry titles, kernel paths, initrd paths, and kernel command lines. This
//! is sufficient for reading standard GRUB configurations from cloud images and
//! Linux distributions.

#[cfg(feature = "std")]
use std::string::String;
#[cfg(feature = "std")]
use std::vec::Vec;

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Represents a GRUB menu entry.
///
/// A menu entry can be either a bootable entry (with kernel and initrd paths)
/// or a submenu containing other entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// A bootable menu entry with kernel, initrd, and command line
    Entry {
        /// Title of the menu entry
        title: String,
        /// Path to the Linux kernel image
        linux: Option<String>,
        /// Path to the initrd image
        initrd: Option<String>,
        /// Kernel command line arguments
        cmdline: Option<String>,
    },
    /// A submenu containing other menu entries
    Submenu {
        /// Title of the submenu
        title: String,
        /// Entries within this submenu
        entries: Vec<MenuEntry>,
    },
}

impl MenuEntry {
    /// Create a new bootable menu entry with the given title.
    pub fn new_entry(title: String) -> Self {
        Self::Entry {
            title,
            linux: None,
            initrd: None,
            cmdline: None,
        }
    }

    /// Create a new submenu with the given title.
    pub fn new_submenu(title: String) -> Self {
        Self::Submenu {
            title,
            entries: Vec::new(),
        }
    }

    /// Get the title of this menu entry (works for both Entry and Submenu).
    pub fn title(&self) -> &str {
        match self {
            MenuEntry::Entry { title, .. } => title,
            MenuEntry::Submenu { title, .. } => title,
        }
    }
}

/// Token types recognized by the GRUB configuration tokenizer.
///
/// The tokenizer produces a stream of these tokens from the input text.
/// This simplified token set is sufficient for parsing menu entries while
/// ignoring complex GRUB scripting constructs.
#[derive(Debug, PartialEq, Eq, Clone)]
enum Token {
    /// A word or quoted string (quotes are removed during tokenization)
    Word(String),
    /// Opening brace `{`
    OpenBrace,
    /// Closing brace `}`
    CloseBrace,
    /// Line separator (newline or semicolon)
    Separator,
}

/// Simple tokenizer for GRUB configuration files.
///
/// The tokenizer processes the input character-by-character, handling:
/// - Whitespace (space, tab, carriage return) - skipped
/// - Newlines and semicolons - converted to `Separator` tokens
/// - Single-quoted strings - quotes removed, contents taken literally
/// - Double-quoted strings - quotes removed, backslash escapes processed
/// - Comments (`#` to end of line) - skipped
/// - Braces - converted to `OpenBrace`/`CloseBrace` tokens
/// - Other characters - accumulated into `Word` tokens
///
/// Note: This is not a full GRUB2 lexer. Variable expansion, command
/// substitution, and other shell-like features are not implemented.
struct Tokenizer<'a> {
    chars: core::iter::Peekable<core::str::Chars<'a>>,
}

impl<'a> Tokenizer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            chars: input.chars().peekable(),
        }
    }

    fn next_token(&mut self) -> Option<Token> {
        loop {
            let c = self.chars.peek()?;
            match c {
                ' ' | '\t' | '\r' => {
                    self.chars.next();
                }
                '\n' | ';' => {
                    self.chars.next();
                    return Some(Token::Separator);
                }
                '{' => {
                    self.chars.next();
                    return Some(Token::OpenBrace);
                }
                '}' => {
                    self.chars.next();
                    return Some(Token::CloseBrace);
                }
                '#' => {
                    // Comment, skip until newline
                    while let Some(&c) = self.chars.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.chars.next();
                    }
                }
                _ => {
                    return Some(self.parse_word());
                }
            }
        }
    }

    fn parse_word(&mut self) -> Token {
        let mut word = String::new();
        while let Some(&c) = self.chars.peek() {
            match c {
                ' ' | '\t' | '\r' | '\n' | ';' | '{' | '}' | '#' => break,
                '\'' => {
                    self.chars.next();
                    for c in self.chars.by_ref() {
                        if c == '\'' {
                            break;
                        }
                        word.push(c);
                    }
                }
                '"' => {
                    self.chars.next();
                    while let Some(c) = self.chars.next() {
                        if c == '"' {
                            break;
                        }
                        if c == '\\' {
                            if let Some(next_c) = self.chars.next() {
                                word.push(next_c);
                            }
                        } else {
                            word.push(c);
                        }
                    }
                }
                _ => {
                    self.chars.next();
                    word.push(c);
                }
            }
        }
        Token::Word(word)
    }
}

/// Parse a GRUB configuration file.
///
/// # Example
///
/// ```
/// use lace_util::grub::{parse_grub_cfg, MenuEntry};
///
/// let config = r#"
/// menuentry 'Ubuntu' {
///     linux /boot/vmlinuz root=/dev/sda1
///     initrd /boot/initrd.img
/// }
/// "#;
///
/// let entries = parse_grub_cfg(config);
/// assert_eq!(entries.len(), 1);
/// assert_eq!(entries[0].title(), "Ubuntu");
/// match &entries[0] {
///     MenuEntry::Entry { linux, cmdline, .. } => {
///         assert_eq!(linux, &Some("/boot/vmlinuz".to_string()));
///         assert_eq!(cmdline, &Some("root=/dev/sda1".to_string()));
///     }
///     _ => panic!("Expected Entry variant"),
/// }
/// ```
pub fn parse_grub_cfg(content: &str) -> Vec<MenuEntry> {
    let mut parser = GrubParser::new(content);
    parser.parse()
}

struct GrubParser<'a> {
    tokenizer: Tokenizer<'a>,
    current_token: Option<Token>,
}

impl<'a> GrubParser<'a> {
    fn new(content: &'a str) -> Self {
        let mut tokenizer = Tokenizer::new(content);
        let current_token = tokenizer.next_token();
        Self {
            tokenizer,
            current_token,
        }
    }

    fn advance(&mut self) {
        self.current_token = self.tokenizer.next_token();
    }

    fn parse(&mut self) -> Vec<MenuEntry> {
        let mut entries = Vec::new();

        while let Some(token) = &self.current_token {
            match token {
                Token::Word(w) => match w.as_str() {
                    "menuentry" => {
                        if let Some(entry) = self.parse_menuentry() {
                            entries.push(entry);
                        }
                    }
                    "submenu" => {
                        if let Some(entry) = self.parse_submenu() {
                            entries.push(entry);
                        }
                    }
                    _ => {
                        self.advance();
                    }
                },
                _ => {
                    self.advance();
                }
            }
        }

        entries
    }

    fn parse_menuentry(&mut self) -> Option<MenuEntry> {
        // Consume 'menuentry'
        self.advance();

        // Expect title
        let title = match &self.current_token {
            Some(Token::Word(t)) => t.clone(),
            _ => return None,
        };
        self.advance();

        let mut linux = None;
        let mut initrd = None;
        let mut cmdline = None;

        // Skip arguments until '{'
        while let Some(token) = &self.current_token {
            if matches!(token, Token::OpenBrace) {
                break;
            }
            self.advance();
        }

        self.current_token.as_ref()?;

        // Consume '{'
        self.advance();

        // Parse body
        let mut brace_depth = 1;
        while let Some(token) = &self.current_token {
            match token {
                Token::OpenBrace => {
                    brace_depth += 1;
                    self.advance();
                }
                Token::CloseBrace => {
                    brace_depth -= 1;
                    self.advance();
                    if brace_depth == 0 {
                        break;
                    }
                }
                Token::Word(w) if brace_depth == 1 => match w.as_str() {
                    "linux" => {
                        self.parse_linux_command(&mut linux, &mut cmdline);
                    }
                    "initrd" => {
                        self.parse_initrd_command(&mut initrd);
                    }
                    _ => {
                        self.advance();
                    }
                },
                _ => {
                    self.advance();
                }
            }
        }

        Some(MenuEntry::Entry {
            title,
            linux,
            initrd,
            cmdline,
        })
    }

    fn parse_submenu(&mut self) -> Option<MenuEntry> {
        // Consume 'submenu'
        self.advance();

        // Expect title
        let title = match &self.current_token {
            Some(Token::Word(t)) => t.clone(),
            _ => return None,
        };
        self.advance();

        let mut entries = Vec::new();

        // Skip arguments until '{'
        while let Some(token) = &self.current_token {
            if matches!(token, Token::OpenBrace) {
                break;
            }
            self.advance();
        }

        self.current_token.as_ref()?;

        // Consume '{'
        self.advance();

        // Parse body
        let mut brace_depth = 1;
        while let Some(token) = &self.current_token {
            match token {
                Token::OpenBrace => {
                    brace_depth += 1;
                    self.advance();
                }
                Token::CloseBrace => {
                    brace_depth -= 1;
                    self.advance();
                    if brace_depth == 0 {
                        break;
                    }
                }
                Token::Word(w) if brace_depth == 1 => match w.as_str() {
                    "menuentry" => {
                        if let Some(entry) = self.parse_menuentry() {
                            entries.push(entry);
                        }
                    }
                    "submenu" => {
                        if let Some(entry) = self.parse_submenu() {
                            entries.push(entry);
                        }
                    }
                    _ => {
                        self.advance();
                    }
                },
                _ => {
                    self.advance();
                }
            }
        }

        Some(MenuEntry::Submenu { title, entries })
    }

    fn parse_linux_command(&mut self, linux: &mut Option<String>, cmdline: &mut Option<String>) {
        self.advance(); // Consume 'linux'

        // Kernel path
        if let Some(Token::Word(path)) = &self.current_token {
            *linux = Some(path.clone());
            self.advance();
        } else {
            return;
        }

        // Cmdline (consume remaining words on this line)
        let mut cmdline_parts = Vec::new();
        while let Some(Token::Word(w)) = &self.current_token {
            cmdline_parts.push(w.clone());
            self.advance();
        }

        if !cmdline_parts.is_empty() {
            *cmdline = Some(cmdline_parts.join(" "));
        }
    }

    fn parse_initrd_command(&mut self, initrd: &mut Option<String>) {
        self.advance(); // Consume 'initrd'

        // Initrd path
        if let Some(Token::Word(path)) = &self.current_token {
            *initrd = Some(path.clone());
            self.advance();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_menuentry() {
        let config = r#"
menuentry 'Ubuntu' {
    linux /boot/vmlinuz root=/dev/sda1
    initrd /boot/initrd.img
}
"#;

        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry {
                title,
                linux,
                initrd,
                cmdline,
            } => {
                assert_eq!(title, "Ubuntu");
                assert_eq!(linux, &Some("/boot/vmlinuz".to_string()));
                assert_eq!(initrd, &Some("/boot/initrd.img".to_string()));
                assert_eq!(cmdline, &Some("root=/dev/sda1".to_string()));
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_parse_multiple_entries() {
        let config = r#"
menuentry 'Ubuntu' {
    linux /boot/vmlinuz-6.8 root=UUID=1234
    initrd /boot/initrd.img-6.8
}

menuentry 'Ubuntu (recovery mode)' {
    linux /boot/vmlinuz-6.8 root=UUID=1234 single
    initrd /boot/initrd.img-6.8
}
"#;

        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title(), "Ubuntu");
        assert_eq!(entries[1].title(), "Ubuntu (recovery mode)");
        match &entries[1] {
            MenuEntry::Entry { cmdline, .. } => {
                assert_eq!(cmdline, &Some("root=UUID=1234 single".to_string()));
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_parse_submenu() {
        let config = r#"
submenu 'Advanced options' {
    menuentry 'Ubuntu' {
        linux /boot/vmlinuz
        initrd /boot/initrd.img
    }
    menuentry 'Ubuntu (recovery)' {
        linux /boot/vmlinuz single
    }
}
"#;

        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Submenu { title, entries } => {
                assert_eq!(title, "Advanced options");
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].title(), "Ubuntu");
                assert_eq!(entries[1].title(), "Ubuntu (recovery)");
            }
            _ => panic!("Expected Submenu variant"),
        }
    }

    #[test]
    fn test_parse_no_initrd() {
        let config = r#"
menuentry 'Test' {
    linux /vmlinuz
}
"#;

        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry {
                linux,
                initrd,
                cmdline,
                ..
            } => {
                assert_eq!(linux, &Some("/vmlinuz".to_string()));
                assert_eq!(initrd, &None);
                assert_eq!(cmdline, &None);
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_parse_empty_config() {
        let config = "";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_parse_nested_braces() {
        let config = r#"
menuentry 'Ubuntu' {
    if [ x$feature_all_video_module = xy ]; then
      insmod all_video
    fi
    linux /boot/vmlinuz root=/dev/sda1
}
"#;

        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title(), "Ubuntu");
        match &entries[0] {
            MenuEntry::Entry { linux, .. } => {
                assert_eq!(linux, &Some("/boot/vmlinuz".to_string()));
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_parse_quoted_strings() {
        let config = r#"
menuentry "Ubuntu '24.04'" {
    linux /boot/vmlinuz root="UUID=1234 5678"
}
"#;
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title(), "Ubuntu '24.04'");
        match &entries[0] {
            MenuEntry::Entry { cmdline, .. } => {
                assert_eq!(cmdline, &Some("root=UUID=1234 5678".to_string()));
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_parse_real_ubuntu_cfg() {
        // Load the real-world grub.cfg from testdata
        let config = include_str!("../testdata/grub.cfg");

        let entries = parse_grub_cfg(config);
        // We expect at least the top-level Ubuntu entry and the submenu
        assert!(entries.len() >= 2, "parsed entries: {}", entries.len());

        // Find the top-level Ubuntu entry
        assert_eq!(entries[0].title(), "Ubuntu");
        match &entries[0] {
            MenuEntry::Entry {
                linux,
                initrd,
                cmdline,
                ..
            } => {
                assert_eq!(linux, &Some("/vmlinuz-6.8.0-90-generic".to_string()));
                assert_eq!(initrd, &Some("/initrd.img-6.8.0-90-generic".to_string()));
                assert_eq!(
                    cmdline,
                    &Some("root=LABEL=cloudimg-rootfs ro console=tty1 console=ttyS0".to_string())
                );
            }
            _ => panic!("Expected Entry variant"),
        }

        // Submenu should be present and contain an entry with the recovery/advanced title
        let submenu_entry = entries
            .iter()
            .find(|e| e.title().starts_with("Advanced options"))
            .expect("submenu not found");

        match submenu_entry {
            MenuEntry::Submenu { entries, .. } => {
                assert!(!entries.is_empty());
                let child = &entries[0];
                assert_eq!(child.title(), "Ubuntu, with Linux 6.8.0-90-generic");
                match child {
                    MenuEntry::Entry {
                        linux,
                        initrd,
                        cmdline,
                        ..
                    } => {
                        assert_eq!(linux, &Some("/vmlinuz-6.8.0-90-generic".to_string()));
                        assert_eq!(initrd, &Some("/initrd.img-6.8.0-90-generic".to_string()));
                        assert_eq!(
                            cmdline,
                            &Some(
                                "root=LABEL=cloudimg-rootfs ro console=tty1 console=ttyS0"
                                    .to_string()
                            )
                        );
                    }
                    _ => panic!("Expected Entry variant for submenu child"),
                }
            }
            _ => panic!("Expected Submenu variant"),
        }
    }

    #[test]
    fn test_tokenizer_single_quotes() {
        // Single quotes don't support escapes - they take everything literally
        let config = r"menuentry 'Test with \quotes' {}";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title(), r"Test with \quotes");
    }

    #[test]
    fn test_tokenizer_double_quotes_with_escapes() {
        let config = r#"menuentry "Test with \"quotes\" and \\backslash" {}"#;
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title(), r#"Test with "quotes" and \backslash"#);
    }

    #[test]
    fn test_tokenizer_comment_handling() {
        let config = r#"
# This is a comment
menuentry 'Ubuntu' { # inline comment
    linux /boot/vmlinuz # another comment
    # Comment in body
    initrd /boot/initrd.img
}
"#;
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry { linux, initrd, .. } => {
                assert_eq!(linux, &Some("/boot/vmlinuz".to_string()));
                assert_eq!(initrd, &Some("/boot/initrd.img".to_string()));
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_menuentry_without_open_brace() {
        let config = "menuentry 'Test'";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_menuentry_without_title() {
        let config = "menuentry { linux /boot/vmlinuz }";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_submenu_without_open_brace() {
        let config = "submenu 'Test'";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_submenu_without_title() {
        // When submenu has no title ('{' comes immediately), parse_submenu returns None
        // The parser then continues and finds the menuentry inside at the top level
        let config = "submenu { menuentry 'test' { linux /vmlinuz } }";
        let entries = parse_grub_cfg(config);
        // The submenu is rejected, but the menuentry inside is parsed at top level
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry { title, linux, .. } => {
                assert_eq!(title, "test");
                assert_eq!(linux, &Some("/vmlinuz".to_string()));
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_linux_command_without_path() {
        let config = "menuentry 'Test' { linux }";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry { linux, .. } => {
                assert_eq!(linux, &None);
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_initrd_command_without_path() {
        let config = "menuentry 'Test' { initrd }";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry { initrd, .. } => {
                assert_eq!(initrd, &None);
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_nested_submenu() {
        let config = r#"
submenu 'Level 1' {
    submenu 'Level 2' {
        menuentry 'Test' {
            linux /vmlinuz
        }
    }
}
"#;
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Submenu { entries, .. } => {
                assert_eq!(entries.len(), 1);
                match &entries[0] {
                    MenuEntry::Submenu { entries, .. } => {
                        assert_eq!(entries.len(), 1);
                        assert_eq!(entries[0].title(), "Test");
                    }
                    _ => panic!("Expected nested Submenu"),
                }
            }
            _ => panic!("Expected Submenu variant"),
        }
    }

    #[test]
    fn test_menuentry_with_multiple_cmdline_args() {
        let config = r#"
menuentry 'Test' {
    linux /vmlinuz arg1 arg2 arg3="value with spaces"
}
"#;
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry { cmdline, .. } => {
                assert_eq!(
                    cmdline,
                    &Some("arg1 arg2 arg3=value with spaces".to_string())
                );
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_deeply_nested_braces() {
        let config = r#"
menuentry 'Test' {
    if [ x = y ]; then
        if [ a = b ]; then
            echo "nested"
        fi
    fi
    linux /vmlinuz
}
"#;
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry { linux, .. } => {
                assert_eq!(linux, &Some("/vmlinuz".to_string()));
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_menuentry_only_linux_no_cmdline() {
        let config = "menuentry 'Test' { linux /vmlinuz\n }";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry { linux, cmdline, .. } => {
                assert_eq!(linux, &Some("/vmlinuz".to_string()));
                assert_eq!(cmdline, &None);
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_tokenizer_carriage_return() {
        let config = "menuentry 'Test'\r\n{\r\n    linux /vmlinuz\r\n}";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry { linux, .. } => {
                assert_eq!(linux, &Some("/vmlinuz".to_string()));
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_tokenizer_tab_characters() {
        let config = "menuentry\t'Test'\t{\n\tlinux\t/vmlinuz\n}";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry { linux, .. } => {
                assert_eq!(linux, &Some("/vmlinuz".to_string()));
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_tokenizer_semicolon_separator() {
        let config = "menuentry 'Test' { linux /vmlinuz; initrd /initrd.img; }";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry { linux, initrd, .. } => {
                assert_eq!(linux, &Some("/vmlinuz".to_string()));
                assert_eq!(initrd, &Some("/initrd.img".to_string()));
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_new_entry_constructor() {
        let entry = MenuEntry::new_entry("Test Entry".to_string());
        match entry {
            MenuEntry::Entry {
                title,
                linux,
                initrd,
                cmdline,
            } => {
                assert_eq!(title, "Test Entry");
                assert_eq!(linux, None);
                assert_eq!(initrd, None);
                assert_eq!(cmdline, None);
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_new_submenu_constructor() {
        let entry = MenuEntry::new_submenu("Test Submenu".to_string());
        match entry {
            MenuEntry::Submenu { title, entries } => {
                assert_eq!(title, "Test Submenu");
                assert_eq!(entries.len(), 0);
            }
            _ => panic!("Expected Submenu variant"),
        }
    }

    #[test]
    fn test_submenu_with_nested_menuentry() {
        let config = r#"
submenu 'Options' {
    menuentry 'Entry1' {
        linux /vmlinuz1
    }
    if [ x = y ]; then
        echo test
    fi
    menuentry 'Entry2' {
        linux /vmlinuz2
    }
}
"#;
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Submenu { entries, .. } => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].title(), "Entry1");
                assert_eq!(entries[1].title(), "Entry2");
            }
            _ => panic!("Expected Submenu variant"),
        }
    }

    #[test]
    fn test_menuentry_with_extra_arguments() {
        let config = "menuentry 'Test' --class ubuntu --class gnu-linux { linux /vmlinuz }";
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Entry { title, linux, .. } => {
                assert_eq!(title, "Test");
                assert_eq!(linux, &Some("/vmlinuz".to_string()));
            }
            _ => panic!("Expected Entry variant"),
        }
    }

    #[test]
    fn test_submenu_with_extra_arguments() {
        let config = r#"
submenu 'Test' --id=test {
    menuentry 'Entry' { linux /vmlinuz }
}
"#;
        let entries = parse_grub_cfg(config);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MenuEntry::Submenu { title, entries } => {
                assert_eq!(title, "Test");
                assert_eq!(entries.len(), 1);
            }
            _ => panic!("Expected Submenu variant"),
        }
    }
}
