/// All token/syntax kinds produced by the scanner.
///
/// Mirrors TypeScript's `SyntaxKind` at the token level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    // ----- Literals -----
    NumericLiteral,
    BigIntLiteral,
    StringLiteral,
    NoSubstitutionTemplate,
    TemplateHead,
    TemplateMiddle,
    TemplateTail,
    RegExpLiteral,

    // ----- Identifiers -----
    Identifier,

    // ----- Reserved keywords -----
    Break,
    Case,
    Catch,
    Class,
    Const,
    Continue,
    Debugger,
    Default,
    Delete,
    Do,
    Else,
    Enum,
    Export,
    Extends,
    False,
    Finally,
    For,
    Function,
    If,
    Import,
    In,
    InstanceOf,
    Let,
    New,
    Null,
    Return,
    Super,
    Switch,
    This,
    Throw,
    True,
    Try,
    TypeOf,
    Var,
    Void,
    While,
    With,
    Yield,

    // ----- Contextual / strict-mode keywords -----
    As,
    Async,
    Await,
    Constructor,
    Declare,
    Get,
    Set,
    From,
    Of,
    Implements,
    Interface,
    Module,
    Namespace,
    Package,
    Private,
    Protected,
    Public,
    Static,
    Type,
    Global,
    Abstract,
    Override,
    Readonly,
    Keyof,
    Unique,
    Infer,
    Is,
    Asserts,
    Assert,
    Require,
    Never,
    UnknownKeyword,
    Any,
    Number,
    BigInt,
    String,
    Boolean,
    Symbol,
    Undefined,
    Object,
    Intrinsic,
    Satisfies,
    Using,
    Accessor,
    Out,

    // ----- Punctuation / operators -----
    OpenBrace,                   // {
    CloseBrace,                  // }
    OpenParen,                   // (
    CloseParen,                  // )
    OpenBracket,                 // [
    CloseBracket,                // ]
    Dot,                         // .
    DotDotDot,                   // ...
    Semicolon,                   // ;
    Comma,                       // ,
    LessThan,                    // <
    GreaterThan,                 // >
    LessEqual,                   // <=
    GreaterEqual,                // >=
    EqualsEquals,                // ==
    ExclEquals,                  // !=
    EqualsEqualsEquals,          // ===
    ExclEqualsEquals,            // !==
    FatArrow,                    // =>
    Plus,                        // +
    Minus,                       // -
    Asterisk,                    // *
    AsteriskAsterisk,            // **
    Slash,                       // /
    Percent,                     // %
    PlusPlus,                    // ++
    MinusMinus,                  // --
    LessLess,                    // <<
    GreaterGreater,              // >>
    GreaterGreaterGreater,       // >>>
    Ampersand,                   // &
    Bar,                         // |
    Caret,                       // ^
    Excl,                        // !
    Tilde,                       // ~
    AmpersandAmpersand,          // &&
    BarBar,                      // ||
    Question,                    // ?
    QuestionDot,                 // ?.
    QuestionQuestion,            // ??
    Colon,                       // :
    Equals,                      // =
    PlusEquals,                  // +=
    MinusEquals,                 // -=
    AsteriskEquals,              // *=
    AsteriskAsteriskEquals,      // **=
    SlashEquals,                 // /=
    PercentEquals,               // %=
    LessLessEquals,              // <<=
    GreaterGreaterEquals,        // >>=
    GreaterGreaterGreaterEquals, // >>>=
    AmpersandEquals,             // &=
    BarEquals,                   // |=
    CaretEquals,                 // ^=
    AmpersandAmpersandEquals,    // &&=
    BarBarEquals,                // ||=
    QuestionQuestionEquals,      // ??=
    At,                          // @
    Hash,                        // #
    Backtick,                    // `

    // ----- Trivia -----
    WhitespaceTrivia,
    NewLineTrivia,
    SingleLineCommentTrivia,
    MultiLineCommentTrivia,
    ShebangTrivia,
    ConflictMarkerTrivia,

    // ----- Special -----
    EndOfFile,
    Unknown,
}

/// Pack up to 8 ASCII bytes into a u64 (little-endian, zero-padded) so keyword
/// lookup is an integer compare rather than a `memcmp`.
const fn pack(s: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let mut i = 0;
    while i < s.len() {
        buf[i] = s[i];
        i += 1;
    }
    u64::from_le_bytes(buf)
}

/// Runtime counterpart of [`pack`]: the same little-endian u64 packing of a
/// 2..=8 byte string, for comparison against the `K_*` constants.
///
/// The obvious `buf[..len].copy_from_slice(b)` has a *runtime* length, which
/// LLVM lowers to a `call memcpy@plt` — a libc call per identifier, which
/// showed up as the hottest instruction in `keyword_from_str`. Instead, take
/// two fixed-width loads that between them span the whole string and overlap
/// in the middle. The overlapping bytes carry identical values, so OR-ing the
/// halves reproduces the packing exactly, with no call and no branch on length
/// beyond the single <=4 split.
#[inline(always)]
fn pack_le(b: &[u8]) -> u64 {
    let len = b.len();
    debug_assert!((2..=8).contains(&len));
    if len <= 4 {
        let lo = u16::from_le_bytes([b[0], b[1]]) as u64;
        let hi = u16::from_le_bytes([b[len - 2], b[len - 1]]) as u64;
        lo | (hi << (8 * (len - 2)))
    } else {
        let lo = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64;
        let hi = u32::from_le_bytes([b[len - 4], b[len - 3], b[len - 2], b[len - 1]]) as u64;
        lo | (hi << (8 * (len - 4)))
    }
}

const K_BREAK: u64 = pack(b"break");
const K_CASE: u64 = pack(b"case");
const K_CATCH: u64 = pack(b"catch");
const K_CLASS: u64 = pack(b"class");
const K_CONST: u64 = pack(b"const");
const K_CONTINUE: u64 = pack(b"continue");
const K_DEBUGGER: u64 = pack(b"debugger");
const K_DEFAULT: u64 = pack(b"default");
const K_DELETE: u64 = pack(b"delete");
const K_DO: u64 = pack(b"do");
const K_ELSE: u64 = pack(b"else");
const K_ENUM: u64 = pack(b"enum");
const K_EXPORT: u64 = pack(b"export");
const K_EXTENDS: u64 = pack(b"extends");
const K_FALSE: u64 = pack(b"false");
const K_FINALLY: u64 = pack(b"finally");
const K_FOR: u64 = pack(b"for");
const K_FUNCTION: u64 = pack(b"function");
const K_IF: u64 = pack(b"if");
const K_IMPORT: u64 = pack(b"import");
const K_IN: u64 = pack(b"in");
const K_LET: u64 = pack(b"let");
const K_NEW: u64 = pack(b"new");
const K_NULL: u64 = pack(b"null");
const K_RETURN: u64 = pack(b"return");
const K_SUPER: u64 = pack(b"super");
const K_SWITCH: u64 = pack(b"switch");
const K_THIS: u64 = pack(b"this");
const K_THROW: u64 = pack(b"throw");
const K_TRUE: u64 = pack(b"true");
const K_TRY: u64 = pack(b"try");
const K_TYPEOF: u64 = pack(b"typeof");
const K_VAR: u64 = pack(b"var");
const K_VOID: u64 = pack(b"void");
const K_WHILE: u64 = pack(b"while");
const K_WITH: u64 = pack(b"with");
const K_YIELD: u64 = pack(b"yield");
const K_AS: u64 = pack(b"as");
const K_ASYNC: u64 = pack(b"async");
const K_AWAIT: u64 = pack(b"await");
const K_DECLARE: u64 = pack(b"declare");
const K_GET: u64 = pack(b"get");
const K_SET: u64 = pack(b"set");
const K_FROM: u64 = pack(b"from");
const K_OF: u64 = pack(b"of");
const K_MODULE: u64 = pack(b"module");
const K_PACKAGE: u64 = pack(b"package");
const K_PRIVATE: u64 = pack(b"private");
const K_PUBLIC: u64 = pack(b"public");
const K_STATIC: u64 = pack(b"static");
const K_TYPE: u64 = pack(b"type");
const K_GLOBAL: u64 = pack(b"global");
const K_ABSTRACT: u64 = pack(b"abstract");
const K_OVERRIDE: u64 = pack(b"override");
const K_READONLY: u64 = pack(b"readonly");
const K_KEYOF: u64 = pack(b"keyof");
const K_UNIQUE: u64 = pack(b"unique");
const K_INFER: u64 = pack(b"infer");
const K_IS: u64 = pack(b"is");
const K_ASSERTS: u64 = pack(b"asserts");
const K_ASSERT: u64 = pack(b"assert");
const K_REQUIRE: u64 = pack(b"require");
const K_NEVER: u64 = pack(b"never");
const K_UNKNOWN: u64 = pack(b"unknown");
const K_ANY: u64 = pack(b"any");
const K_NUMBER: u64 = pack(b"number");
const K_BIGINT: u64 = pack(b"bigint");
const K_STRING: u64 = pack(b"string");
const K_BOOLEAN: u64 = pack(b"boolean");
const K_SYMBOL: u64 = pack(b"symbol");
const K_OBJECT: u64 = pack(b"object");
const K_USING: u64 = pack(b"using");
const K_ACCESSOR: u64 = pack(b"accessor");
const K_OUT: u64 = pack(b"out");

impl TokenKind {
    /// Fixed source spelling for keywords and punctuation. Literal, identifier,
    /// trivia, and end-of-file tokens have no fixed spelling.
    pub fn fixed_text(self) -> Option<&'static str> {
        Some(match self {
            Self::Break => "break",
            Self::Case => "case",
            Self::Catch => "catch",
            Self::Class => "class",
            Self::Const => "const",
            Self::Continue => "continue",
            Self::Debugger => "debugger",
            Self::Default => "default",
            Self::Delete => "delete",
            Self::Do => "do",
            Self::Else => "else",
            Self::Enum => "enum",
            Self::Export => "export",
            Self::Extends => "extends",
            Self::False => "false",
            Self::Finally => "finally",
            Self::For => "for",
            Self::Function => "function",
            Self::If => "if",
            Self::Import => "import",
            Self::In => "in",
            Self::InstanceOf => "instanceof",
            Self::Let => "let",
            Self::New => "new",
            Self::Null => "null",
            Self::Return => "return",
            Self::Super => "super",
            Self::Switch => "switch",
            Self::This => "this",
            Self::Throw => "throw",
            Self::True => "true",
            Self::Try => "try",
            Self::TypeOf => "typeof",
            Self::Var => "var",
            Self::Void => "void",
            Self::While => "while",
            Self::With => "with",
            Self::Yield => "yield",
            Self::As => "as",
            Self::Async => "async",
            Self::Await => "await",
            Self::Constructor => "constructor",
            Self::Declare => "declare",
            Self::Get => "get",
            Self::Set => "set",
            Self::From => "from",
            Self::Of => "of",
            Self::Implements => "implements",
            Self::Interface => "interface",
            Self::Module => "module",
            Self::Namespace => "namespace",
            Self::Package => "package",
            Self::Private => "private",
            Self::Protected => "protected",
            Self::Public => "public",
            Self::Static => "static",
            Self::Type => "type",
            Self::Global => "global",
            Self::Abstract => "abstract",
            Self::Override => "override",
            Self::Readonly => "readonly",
            Self::Keyof => "keyof",
            Self::Unique => "unique",
            Self::Infer => "infer",
            Self::Is => "is",
            Self::Asserts => "asserts",
            Self::Assert => "assert",
            Self::Require => "require",
            Self::Never => "never",
            Self::UnknownKeyword => "unknown",
            Self::Any => "any",
            Self::Number => "number",
            Self::BigInt => "bigint",
            Self::String => "string",
            Self::Boolean => "boolean",
            Self::Symbol => "symbol",
            Self::Undefined => "undefined",
            Self::Object => "object",
            Self::Intrinsic => "intrinsic",
            Self::Satisfies => "satisfies",
            Self::Using => "using",
            Self::Accessor => "accessor",
            Self::Out => "out",
            Self::OpenBrace => "{",
            Self::CloseBrace => "}",
            Self::OpenParen => "(",
            Self::CloseParen => ")",
            Self::OpenBracket => "[",
            Self::CloseBracket => "]",
            Self::Dot => ".",
            Self::DotDotDot => "...",
            Self::Semicolon => ";",
            Self::Comma => ",",
            Self::LessThan => "<",
            Self::GreaterThan => ">",
            Self::LessEqual => "<=",
            Self::GreaterEqual => ">=",
            Self::EqualsEquals => "==",
            Self::ExclEquals => "!=",
            Self::EqualsEqualsEquals => "===",
            Self::ExclEqualsEquals => "!==",
            Self::FatArrow => "=>",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Asterisk => "*",
            Self::AsteriskAsterisk => "**",
            Self::Slash => "/",
            Self::Percent => "%",
            Self::PlusPlus => "++",
            Self::MinusMinus => "--",
            Self::LessLess => "<<",
            Self::GreaterGreater => ">>",
            Self::GreaterGreaterGreater => ">>>",
            Self::Ampersand => "&",
            Self::Bar => "|",
            Self::Caret => "^",
            Self::Excl => "!",
            Self::Tilde => "~",
            Self::AmpersandAmpersand => "&&",
            Self::BarBar => "||",
            Self::Question => "?",
            Self::QuestionDot => "?.",
            Self::QuestionQuestion => "??",
            Self::Colon => ":",
            Self::Equals => "=",
            Self::PlusEquals => "+=",
            Self::MinusEquals => "-=",
            Self::AsteriskEquals => "*=",
            Self::AsteriskAsteriskEquals => "**=",
            Self::SlashEquals => "/=",
            Self::PercentEquals => "%=",
            Self::LessLessEquals => "<<=",
            Self::GreaterGreaterEquals => ">>=",
            Self::GreaterGreaterGreaterEquals => ">>>=",
            Self::AmpersandEquals => "&=",
            Self::BarEquals => "|=",
            Self::CaretEquals => "^=",
            Self::AmpersandAmpersandEquals => "&&=",
            Self::BarBarEquals => "||=",
            Self::QuestionQuestionEquals => "??=",
            Self::At => "@",
            Self::Hash => "#",
            Self::Backtick => "`",
            _ => return None,
        })
    }

    /// Returns `true` for identifiers and all tokens accepted as property names.
    /// These variants are deliberately contiguous in the enum.
    #[inline(always)]
    pub fn is_identifier_name(self) -> bool {
        let kind = self as u16;
        kind >= Self::Identifier as u16 && kind <= Self::Out as u16
    }

    /// Returns `true` for reserved keywords (always keyword, never identifier).
    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            Self::Break
                | Self::Case
                | Self::Catch
                | Self::Class
                | Self::Const
                | Self::Continue
                | Self::Debugger
                | Self::Default
                | Self::Delete
                | Self::Do
                | Self::Else
                | Self::Enum
                | Self::Export
                | Self::Extends
                | Self::False
                | Self::Finally
                | Self::For
                | Self::Function
                | Self::If
                | Self::Import
                | Self::In
                | Self::InstanceOf
                | Self::Let
                | Self::New
                | Self::Null
                | Self::Return
                | Self::Super
                | Self::Switch
                | Self::This
                | Self::Throw
                | Self::True
                | Self::Try
                | Self::TypeOf
                | Self::Var
                | Self::Void
                | Self::While
                | Self::With
                | Self::Yield
        )
    }

    /// Returns `true` for contextual keywords (can also be identifiers).
    pub fn is_contextual_keyword(self) -> bool {
        matches!(
            self,
            Self::As
                | Self::Async
                | Self::Await
                | Self::Constructor
                | Self::Declare
                | Self::Get
                | Self::Set
                | Self::From
                | Self::Of
                | Self::Implements
                | Self::Interface
                | Self::Module
                | Self::Namespace
                | Self::Package
                | Self::Private
                | Self::Protected
                | Self::Public
                | Self::Static
                | Self::Type
                | Self::Global
                | Self::Abstract
                | Self::Override
                | Self::Readonly
                | Self::Keyof
                | Self::Unique
                | Self::Infer
                | Self::Is
                | Self::Asserts
                | Self::Assert
                | Self::Require
                | Self::Never
                | Self::UnknownKeyword
                | Self::Any
                | Self::Number
                | Self::BigInt
                | Self::String
                | Self::Boolean
                | Self::Symbol
                | Self::Undefined
                | Self::Object
                | Self::Intrinsic
                | Self::Satisfies
                | Self::Using
                | Self::Accessor
                | Self::Out
        )
    }

    /// Returns `true` for trivia tokens (whitespace, comments, etc.).
    ///
    /// The trivia variants are contiguous, and LLVM already lowers this to a range
    /// check — hand-rolling one measured no faster, so keep the readable form.
    #[inline(always)]
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::WhitespaceTrivia
                | Self::NewLineTrivia
                | Self::SingleLineCommentTrivia
                | Self::MultiLineCommentTrivia
                | Self::ShebangTrivia
                | Self::ConflictMarkerTrivia
        )
    }

    /// Returns `true` for literal tokens.
    pub fn is_literal(self) -> bool {
        matches!(
            self,
            Self::NumericLiteral
                | Self::BigIntLiteral
                | Self::StringLiteral
                | Self::RegExpLiteral
                | Self::NoSubstitutionTemplate
        )
    }

    /// Returns `true` for template literal parts.
    pub fn is_template_literal(self) -> bool {
        matches!(
            self,
            Self::NoSubstitutionTemplate
                | Self::TemplateHead
                | Self::TemplateMiddle
                | Self::TemplateTail
        )
    }

    /// Returns `true` for punctuation / operator tokens.
    pub fn is_punctuation(self) -> bool {
        matches!(
            self,
            Self::OpenBrace
                | Self::CloseBrace
                | Self::OpenParen
                | Self::CloseParen
                | Self::OpenBracket
                | Self::CloseBracket
                | Self::Dot
                | Self::DotDotDot
                | Self::Semicolon
                | Self::Comma
                | Self::LessThan
                | Self::GreaterThan
                | Self::LessEqual
                | Self::GreaterEqual
                | Self::EqualsEquals
                | Self::ExclEquals
                | Self::EqualsEqualsEquals
                | Self::ExclEqualsEquals
                | Self::FatArrow
                | Self::Plus
                | Self::Minus
                | Self::Asterisk
                | Self::AsteriskAsterisk
                | Self::Slash
                | Self::Percent
                | Self::PlusPlus
                | Self::MinusMinus
                | Self::LessLess
                | Self::GreaterGreater
                | Self::GreaterGreaterGreater
                | Self::Ampersand
                | Self::Bar
                | Self::Caret
                | Self::Excl
                | Self::Tilde
                | Self::AmpersandAmpersand
                | Self::BarBar
                | Self::Question
                | Self::QuestionDot
                | Self::QuestionQuestion
                | Self::Colon
                | Self::Equals
                | Self::PlusEquals
                | Self::MinusEquals
                | Self::AsteriskEquals
                | Self::AsteriskAsteriskEquals
                | Self::SlashEquals
                | Self::PercentEquals
                | Self::LessLessEquals
                | Self::GreaterGreaterEquals
                | Self::GreaterGreaterGreaterEquals
                | Self::AmpersandEquals
                | Self::BarEquals
                | Self::CaretEquals
                | Self::AmpersandAmpersandEquals
                | Self::BarBarEquals
                | Self::QuestionQuestionEquals
                | Self::At
                | Self::Hash
                | Self::Backtick
        )
    }

    /// Returns `true` for assignment operators.
    pub fn is_assignment(self) -> bool {
        matches!(
            self,
            Self::Equals
                | Self::PlusEquals
                | Self::MinusEquals
                | Self::AsteriskEquals
                | Self::AsteriskAsteriskEquals
                | Self::SlashEquals
                | Self::PercentEquals
                | Self::LessLessEquals
                | Self::GreaterGreaterEquals
                | Self::GreaterGreaterGreaterEquals
                | Self::AmpersandEquals
                | Self::BarEquals
                | Self::CaretEquals
                | Self::AmpersandAmpersandEquals
                | Self::BarBarEquals
                | Self::QuestionQuestionEquals
        )
    }

    /// Look up a keyword from an identifier string.
    /// Returns `Identifier` if the string is not a keyword.
    /// Look up a keyword from an identifier string.
    /// Returns `Identifier` if the string is not a keyword.
    ///
    /// Every keyword is 2..=11 lowercase ASCII bytes. The <=8-byte ones (the vast
    /// majority) are packed into a u64 and matched as an integer, which compiles to
    /// a jump table — the previous `match s` compiled to chains of `memcmp` calls
    /// and cost ~3.5% of all parse instructions, since every identifier pays it.
    pub fn keyword_from_str(s: &str) -> TokenKind {
        let b = s.as_bytes();
        let len = b.len();
        if len < 2 || len > 11 {
            return TokenKind::Identifier;
        }
        if len <= 8 {
            return match pack_le(b) {
                K_BREAK => TokenKind::Break,
                K_CASE => TokenKind::Case,
                K_CATCH => TokenKind::Catch,
                K_CLASS => TokenKind::Class,
                K_CONST => TokenKind::Const,
                K_CONTINUE => TokenKind::Continue,
                K_DEBUGGER => TokenKind::Debugger,
                K_DEFAULT => TokenKind::Default,
                K_DELETE => TokenKind::Delete,
                K_DO => TokenKind::Do,
                K_ELSE => TokenKind::Else,
                K_ENUM => TokenKind::Enum,
                K_EXPORT => TokenKind::Export,
                K_EXTENDS => TokenKind::Extends,
                K_FALSE => TokenKind::False,
                K_FINALLY => TokenKind::Finally,
                K_FOR => TokenKind::For,
                K_FUNCTION => TokenKind::Function,
                K_IF => TokenKind::If,
                K_IMPORT => TokenKind::Import,
                K_IN => TokenKind::In,
                K_LET => TokenKind::Let,
                K_NEW => TokenKind::New,
                K_NULL => TokenKind::Null,
                K_RETURN => TokenKind::Return,
                K_SUPER => TokenKind::Super,
                K_SWITCH => TokenKind::Switch,
                K_THIS => TokenKind::This,
                K_THROW => TokenKind::Throw,
                K_TRUE => TokenKind::True,
                K_TRY => TokenKind::Try,
                K_TYPEOF => TokenKind::TypeOf,
                K_VAR => TokenKind::Var,
                K_VOID => TokenKind::Void,
                K_WHILE => TokenKind::While,
                K_WITH => TokenKind::With,
                K_YIELD => TokenKind::Yield,
                K_AS => TokenKind::As,
                K_ASYNC => TokenKind::Async,
                K_AWAIT => TokenKind::Await,
                K_DECLARE => TokenKind::Declare,
                K_GET => TokenKind::Get,
                K_SET => TokenKind::Set,
                K_FROM => TokenKind::From,
                K_OF => TokenKind::Of,
                K_MODULE => TokenKind::Module,
                K_PACKAGE => TokenKind::Package,
                K_PRIVATE => TokenKind::Private,
                K_PUBLIC => TokenKind::Public,
                K_STATIC => TokenKind::Static,
                K_TYPE => TokenKind::Type,
                K_GLOBAL => TokenKind::Global,
                K_ABSTRACT => TokenKind::Abstract,
                K_OVERRIDE => TokenKind::Override,
                K_READONLY => TokenKind::Readonly,
                K_KEYOF => TokenKind::Keyof,
                K_UNIQUE => TokenKind::Unique,
                K_INFER => TokenKind::Infer,
                K_IS => TokenKind::Is,
                K_ASSERTS => TokenKind::Asserts,
                K_ASSERT => TokenKind::Assert,
                K_REQUIRE => TokenKind::Require,
                K_NEVER => TokenKind::Never,
                K_UNKNOWN => TokenKind::UnknownKeyword,
                K_ANY => TokenKind::Any,
                K_NUMBER => TokenKind::Number,
                K_BIGINT => TokenKind::BigInt,
                K_STRING => TokenKind::String,
                K_BOOLEAN => TokenKind::Boolean,
                K_SYMBOL => TokenKind::Symbol,
                K_OBJECT => TokenKind::Object,
                K_USING => TokenKind::Using,
                K_ACCESSOR => TokenKind::Accessor,
                K_OUT => TokenKind::Out,
                _ => TokenKind::Identifier,
            };
        }
        // 9..=11 bytes: rare enough that a string match is fine.
        match s {
            "instanceof" => TokenKind::InstanceOf,
            "constructor" => TokenKind::Constructor,
            "implements" => TokenKind::Implements,
            "interface" => TokenKind::Interface,
            "namespace" => TokenKind::Namespace,
            "protected" => TokenKind::Protected,
            "undefined" => TokenKind::Undefined,
            "intrinsic" => TokenKind::Intrinsic,
            "satisfies" => TokenKind::Satisfies,
            _ => TokenKind::Identifier,
        }
    }
}

#[cfg(test)]
mod keyword_tests {
    use super::*;

    /// All 83 keywords `keyword_from_str` recognises. Kept here so the packing
    /// fast path can be proven to admit every one of them.
    const ALL_KEYWORDS: &[&str] = &[
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "import",
        "in",
        "let",
        "new",
        "null",
        "return",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
        "yield",
        "as",
        "async",
        "await",
        "declare",
        "get",
        "set",
        "from",
        "of",
        "module",
        "package",
        "private",
        "public",
        "static",
        "type",
        "global",
        "abstract",
        "override",
        "readonly",
        "keyof",
        "unique",
        "infer",
        "is",
        "asserts",
        "assert",
        "require",
        "never",
        "unknown",
        "any",
        "number",
        "bigint",
        "string",
        "boolean",
        "symbol",
        "object",
        "using",
        "accessor",
        "out",
        "instanceof",
        "constructor",
        "implements",
        "interface",
        "namespace",
        "protected",
        "undefined",
        "intrinsic",
        "satisfies",
    ];

    /// `pack_le` (two overlapping loads) must agree with the const `pack`
    /// (byte-by-byte) for every length and byte pattern it can see.
    #[test]
    fn pack_le_agrees_with_pack() {
        for len in 2..=8usize {
            for a in 0..=255u8 {
                for b in 0..=255u8 {
                    // Vary the ends (which the two loads overlap) and the middle.
                    let bytes: Vec<u8> = (0..len)
                        .map(|i| match i {
                            0 => a,
                            i if i == len - 1 => b,
                            i => a.wrapping_mul(i as u8).wrapping_add(b),
                        })
                        .collect();
                    assert_eq!(pack_le(&bytes), pack(&bytes), "len={len} bytes={bytes:?}");
                }
            }
        }
    }

    #[test]
    fn every_keyword_is_recognised() {
        for kw in ALL_KEYWORDS {
            assert_ne!(
                TokenKind::keyword_from_str(kw),
                TokenKind::Identifier,
                "keyword `{kw}` no longer classifies as a keyword"
            );
        }
        assert_eq!(ALL_KEYWORDS.len(), 83);
    }

    #[test]
    fn near_miss_identifiers_are_not_keywords() {
        for s in [
            "brea", "breaks", "clas", "classy", "constx", "aconst", "retur", "returns", "typeo",
            "iff", "inn", "ass", "xx", "ab", "zz",
        ] {
            assert_eq!(
                TokenKind::keyword_from_str(s),
                TokenKind::Identifier,
                "`{s}` should be an identifier"
            );
        }
    }
}
