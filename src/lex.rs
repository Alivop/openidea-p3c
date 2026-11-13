//! Java lexer: one pass over the source, keeping byte offsets.
//!
//! Literals and comments stay in the token stream under their own kinds rather
//! than being masked out, so a rule can skip them (code-only checks) or read
//! them (date-pattern checks) without a second pass that distorts offsets.

use openidea_plugin::Position;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ident,
    Number,
    String,
    Char,
    TextBlock,
    LineComment,
    BlockComment,
    Javadoc,
    Punct,
}

impl Kind {
    /// Code a rule can match on: not a literal's contents, not a comment.
    pub fn is_code(self) -> bool {
        matches!(self, Kind::Ident | Kind::Number | Kind::Punct)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
}

/// Byte offset of every line start, for UTF-16 editor positions.
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self { starts }
    }

    pub fn position(&self, text: &str, offset: usize) -> Position {
        let offset = offset.min(text.len());
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        let character = text[self.starts[line]..offset].chars().map(|c| c.len_utf16() as u32).sum();
        Position::new(line as u32, character)
    }

    /// 反查：列是 UTF-16 码元数，中文与 emoji 不能按字节数直接加。
    pub fn offset(&self, text: &str, pos: Position) -> usize {
        let line = (pos.line as usize).min(self.starts.len() - 1);
        let start = self.starts[line];
        let end = self.starts.get(line + 1).map_or(text.len(), |&s| s - 1);
        let mut col = 0;
        for (i, c) in text[start..end].char_indices() {
            if col >= pos.character {
                return start + i;
            }
            col += c.len_utf16() as u32;
        }
        end
    }
}

fn ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn ident_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Longest first. `<<`, `>>` and `>>>` stay single `>`/`<` tokens: no rule needs
/// shifts, and splitting them keeps nested generics (`List<List<T>>`) parsable.
const PUNCTS: [&str; 19] =
    ["...", "->", "::", "==", "!=", "<=", ">=", "&&", "||", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^="];

pub fn tokenize(text: &str) -> Vec<Token> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        let kind = match b[i] {
            b' ' | b'\t' | b'\r' | b'\n' | 0x0c => {
                i += 1;
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i += 2;
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                Kind::LineComment
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let javadoc = b.get(i + 2) == Some(&b'*') && b.get(i + 3) != Some(&b'/');
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i = (i + 2).min(b.len());
                if javadoc { Kind::Javadoc } else { Kind::BlockComment }
            }
            b'"' if b[i..].starts_with(b"\"\"\"") => {
                i += 3;
                while i < b.len() && !b[i..].starts_with(b"\"\"\"") {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 3).min(b.len());
                Kind::TextBlock
            }
            b'"' | b'\'' => {
                let quote = b[i];
                i += 1;
                while i < b.len() && b[i] != quote && b[i] != b'\n' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(b.len());
                if quote == b'"' { Kind::String } else { Kind::Char }
            }
            c if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) => {
                i = number_end(b, i);
                Kind::Number
            }
            _ => {
                let c = text[i..].chars().next().unwrap();
                if ident_start(c) {
                    i += c.len_utf8();
                    while i < b.len() {
                        let c = text[i..].chars().next().unwrap();
                        if !ident_part(c) {
                            break;
                        }
                        i += c.len_utf8();
                    }
                    Kind::Ident
                } else {
                    let punct = PUNCTS.iter().find(|p| b[i..].starts_with(p.as_bytes()));
                    i += punct.map_or(c.len_utf8(), |p| p.len());
                    Kind::Punct
                }
            }
        };
        out.push(Token { kind, start, end: i.min(b.len()) });
    }
    out
}

fn number_end(b: &[u8], mut i: usize) -> usize {
    let radix = if b[i] == b'0' && matches!(b.get(i + 1), Some(b'x' | b'X')) {
        i += 2;
        16
    } else if b[i] == b'0' && matches!(b.get(i + 1), Some(b'b' | b'B')) {
        i += 2;
        2
    } else {
        10
    };
    let digit = |c: u8| match radix {
        16 => c.is_ascii_hexdigit(),
        2 => matches!(c, b'0' | b'1'),
        _ => c.is_ascii_digit(),
    };
    while i < b.len() && (digit(b[i]) || b[i] == b'_') {
        i += 1;
    }
    if radix == 10 {
        // `1.` and `1.5` are literals; `1.foo` is not Java, so stop before an identifier.
        if b.get(i) == Some(&b'.') && !b.get(i + 1).is_some_and(|&c| ident_start(c as char)) {
            i += 1;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
                i += 1;
            }
        }
        if matches!(b.get(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(b.get(j), Some(b'+' | b'-')) {
                j += 1;
            }
            if b.get(j).is_some_and(u8::is_ascii_digit) {
                i = j;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
                    i += 1;
                }
            }
        }
    }
    if matches!(b.get(i), Some(b'l' | b'L' | b'f' | b'F' | b'd' | b'D')) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(Kind, &str)> {
        tokenize(text).into_iter().map(|t| (t.kind, &text[t.start..t.end])).collect()
    }

    #[test]
    fn literals_and_comments_keep_their_own_kinds() {
        let src = r#"// note
/* block */ /** doc */
String s = "a\"b"; char c = 'x'; long n = 2l;
"#;
        let got = kinds(src);
        assert!(got.contains(&(Kind::LineComment, "// note")));
        assert!(got.contains(&(Kind::BlockComment, "/* block */")));
        assert!(got.contains(&(Kind::Javadoc, "/** doc */")));
        assert!(got.contains(&(Kind::String, "\"a\\\"b\"")));
        assert!(got.contains(&(Kind::Char, "'x'")));
        assert!(got.contains(&(Kind::Number, "2l")));
        assert!(got.iter().filter(|(k, _)| k.is_code()).count() == 13);
    }

    #[test]
    fn an_empty_block_comment_is_not_javadoc() {
        assert_eq!(kinds("/**/"), vec![(Kind::BlockComment, "/**/")]);
        assert_eq!(kinds("/***/"), vec![(Kind::Javadoc, "/***/")]);
    }

    #[test]
    fn unterminated_literals_stop_at_the_line_or_the_end() {
        assert_eq!(kinds("\"abc"), vec![(Kind::String, "\"abc")]);
        assert_eq!(kinds("/* abc"), vec![(Kind::BlockComment, "/* abc")]);
        assert_eq!(kinds("'a"), vec![(Kind::Char, "'a")]);
    }

    #[test]
    fn text_blocks_span_lines() {
        let src = "String s = \"\"\"\n  a\"b\n  \"\"\";";
        let got = kinds(src);
        assert_eq!(got[3].0, Kind::TextBlock);
        assert_eq!(got[3].1, "\"\"\"\n  a\"b\n  \"\"\"");
    }

    #[test]
    fn numbers_cover_radix_separators_and_suffixes() {
        for src in ["0x1F", "0X1fl", "0b1010", "1_000L", "1.5e-3f", ".5", "1.", "42D"] {
            assert_eq!(kinds(src), vec![(Kind::Number, src)], "{src}");
        }
        assert_eq!(kinds("1.foo"), vec![(Kind::Number, "1"), (Kind::Punct, "."), (Kind::Ident, "foo")]);
    }

    #[test]
    fn identifiers_include_chinese_and_currency() {
        assert_eq!(kinds("中文 $a _b"), vec![(Kind::Ident, "中文"), (Kind::Ident, "$a"), (Kind::Ident, "_b")]);
    }

    #[test]
    fn nested_generics_split_the_closing_angles() {
        assert_eq!(
            kinds("List<List<T>>"),
            vec![
                (Kind::Ident, "List"),
                (Kind::Punct, "<"),
                (Kind::Ident, "List"),
                (Kind::Punct, "<"),
                (Kind::Ident, "T"),
                (Kind::Punct, ">"),
                (Kind::Punct, ">"),
            ]
        );
    }

    #[test]
    fn offsets_survive_multibyte_text() {
        let src = "// 中文注释\nlong n = 2l;";
        let tokens = tokenize(src);
        let n = tokens.iter().find(|t| &src[t.start..t.end] == "2l").unwrap();
        assert_eq!(&src[n.start..n.end], "2l");
        let lines = LineIndex::new(src);
        assert_eq!(lines.position(src, n.start), Position::new(1, 9));
    }

    #[test]
    fn columns_are_utf16_code_units() {
        let src = "String s = \"中文\"; long n = 2l;";
        let tokens = tokenize(src);
        let n = tokens.iter().find(|t| &src[t.start..t.end] == "2l").unwrap();
        // "String s = " is 11 units, the quoted pair is 4 more, "; long n = " is 11.
        assert_eq!(LineIndex::new(src).position(src, n.start), Position::new(0, 26));
    }

    #[test]
    fn astral_characters_count_as_two_utf16_units() {
        let src = "// 😀\n2l";
        let tokens = tokenize(src);
        assert_eq!(LineIndex::new(src).position(src, tokens[1].start), Position::new(1, 0));
        let src = "😀 2l";
        let tokens = tokenize(src);
        assert_eq!(LineIndex::new(src).position(src, tokens[1].start), Position::new(0, 3));
    }
}
