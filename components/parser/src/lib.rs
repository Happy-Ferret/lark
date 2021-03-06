#![feature(box_patterns)]
#![feature(box_syntax)]
#![feature(crate_visibility_modifier)]
#![feature(in_band_lifetimes)]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#[macro_use]
pub mod lexer;

pub mod ast;
pub mod grammar;
pub mod grammar_helpers;
pub mod keywords;
pub mod lexer_helpers;
pub mod pos;
pub mod program;
pub mod token;
pub mod tokenizer;

#[cfg(test)]
pub mod test_helpers;

#[cfg(test)]
use self::test_helpers::LineTokenizer;

pub use self::grammar::ProgramParser;
pub use self::lexer_helpers::ParseError;
pub use self::program::{Environment, Module, ModuleTable, NameId, StringId};
pub use self::token::Token;
pub use self::tokenizer::Tokenizer;

pub use self::pos::{Span, Spanned};

use itertools::Itertools;
use log::{trace, warn};
use std::collections::HashMap;

use crate::lexer::KeywordList;

use codespan::ByteIndex;
use derive_new::new;
use std::borrow::{Borrow, Cow};
use std::error::Error;
use std::fmt;

pub fn parse(
    source: impl Into<Cow<'source, str>>,
    table: &'source mut ModuleTable,
    start: u32,
) -> Result<ast::Module, ParseError> {
    let cow = source.into();
    let tokenizer = Tokenizer::new(table, cow.borrow(), start);
    let parser = ProgramParser::new();
    let module = parser
        .parse(tokenizer)
        .map_err(|err| lalrpop_err(err, table));
    Ok(module?)
}

crate fn lalrpop_err(
    err: lalrpop_util::ParseError<ByteIndex, Token, ParseError>,
    table: &ModuleTable,
) -> ParseError {
    use lalrpop_util::ParseError::*;

    match err {
        InvalidToken { location } => ParseError::from_pos("Invalid Token", location),
        UnrecognizedToken {
            token: Some((left, token, right)),
            expected,
        } => ParseError::from(
            format!(
                "Unexpected token {}, expected: {}",
                token.source(table),
                KeywordList::new(expected)
            ),
            left,
            right,
        ),

        UnrecognizedToken {
            token: None,
            expected,
        } => ParseError::from_eof(format!(
            "Unrecognized EOF, expected: {}",
            KeywordList::new(expected)
        )),

        ExtraToken {
            token: (left, token, right),
        } => ParseError::from(format!("Extra Token {}", token.source(table)), left, right),

        User { error } => error,
    }
}

#[cfg(test)]
mod test {
    #![cfg(broken)] // disable for now

    use super::parse;
    use super::tokenizer::Tokenizer;
    use super::LineTokenizer;
    use codespan::ByteOffset;
    use crate::ast::DebugModuleTable;

    use codespan::ByteIndex;
    use codespan::CodeMap;
    use crate::ast::{Debuggable, DebuggableVec, Mode};
    use crate::lexer_helpers::ParseError;
    use crate::pos::{Span, Spanned};
    use crate::program::ModuleTable;
    use crate::program::StringId;
    use crate::test_helpers::{self, Token};
    use crate::{self, ast};

    use derive_new::new;
    use itertools::Itertools;
    use language_reporting::{emit, Diagnostic, Label, Severity};
    use log::{debug, trace, warn};
    use std::collections::HashMap;
    use termcolor::{ColorChoice, StandardStream};
    use unindent::unindent;

    fn init() {
        pretty_env_logger::init();
    }

    fn parse_string(source: &str, annotations: &mut Annotations) -> (ast::Module, u32) {
        let filemap = annotations
            .codemap()
            .add_filemap("test".into(), source.to_string());
        let start = filemap.span().start().0;

        let module = parse(&source[..], annotations.table(), start)
            .unwrap_or_else(|e| print_parse_error(e, annotations.codemap()));

        (module, start)
    }

    #[test]
    fn test_struct() -> Result<(), Box<dyn std::error::Error>> {
        init();

        let source = unindent(
            r#"
            struct Diagnostic {
            ^^^^^^ ^^^^^^^^^^ ^ open-struct
              msg: own String,
              ^^^  ^^^ ^^^^^^ msg
              level: String,
              ^^^^^  ^^^^^^ level
            }
            ^ close-struct
            def new(msg: own String, level: String) -> Diagnostic {
            ^^^ ^^^ ^^^  ^^^ ^^^^^^  ^^^^^  ^^^^^^     ^^^^^^^^^^ ^ new
              Diagnostic { msg, level }
              ^^^^^^^^^^ ^ ^^^  ^^^^^ ^ construct-diag
            }
            ^ close-new
            def main() {
            ^^^ ^^^^   ^ main
                let var_name = "variable"
                ^^^ ^^^^^^^^   ^^^^^^^^^^ var-name
                let s = "variable is unused " + var_name
                ^^^ ^   ^^^^^^^^^^^^^^^^^^^^^ ^ ^^^^^^^^  ^ s-var
                new(s, "warning")
                ^^^ ^  ^^^^^^^^^~ invoke
            }
            ^ close-main
            "#,
        );

        let table = ModuleTable::new();
        let mut codemap = CodeMap::new();
        let (source, mut ann) = extract(&source, table, codemap, 1);
        let (actual, start) = parse_string(&source, &mut ann);

        let s = ast::Struct::new(
            ann.ident(("open-struct", 1)),
            vec![ann.field_mode("msg", 0), ann.field("level", 0)],
            ann.span(("open-struct", 0), ("close-struct", 0)),
        );

        let def = ast::Def::new(
            ann.ident(("new", 1)),
            vec![ann.field_mode("new", 2), ann.field("new", 5)],
            Some(ann.ty("new", 7)),
            ast::Block::spanned(
                vec![ast::BlockItem::Expr(ast::Expression::ConstructStruct(
                    ast::ConstructStruct::new(
                        ann.ident(("construct-diag", 0)),
                        vec![
                            ann.shorthand(("construct-diag", 2)),
                            ann.shorthand(("construct-diag", 3)),
                        ],
                        ann.span(("construct-diag", 0), ("construct-diag", 4)),
                    ),
                ))],
                ann.span(("new", 8), ("close-new", 0)),
            ),
            ann.span(("new", 0), ("close-new", 0)),
        );

        let main = ast::Def::new(
            ann.ident(("main", 1)),
            vec![],
            None,
            ast::Block::spanned(
                vec![
                    ast::BlockItem::let_decl(
                        ann.pat_ident(("var-name", 1)),
                        None,
                        Some(ann.string(("var-name", 2))),
                    ),
                    ast::BlockItem::let_decl(
                        ann.pat_ident(("s-var", 1)),
                        None,
                        Some(ast::Expression::binary(
                            ann.op(("s-var", 3)),
                            ann.string(("s-var", 2)),
                            ann.refers(("s-var", 4)),
                        )),
                    ),
                    ast::BlockItem::Expr(ast::Expression::call(
                        ann.ident(("invoke", 0)),
                        vec![ann.refers(("invoke", 1)), ann.string(("invoke", 2))],
                        ann.span(("invoke", 0), ("invoke", 3)),
                    )),
                ],
                ann.span(("main", 2), ("close-main", 0)),
            ),
            ann.span(("main", 0), ("close-main", 0)),
        );

        eq(
            actual,
            ast::Module::new(vec![
                ast::Item::Struct(s),
                ast::Item::Def(def),
                ast::Item::Def(main),
            ]),
            ann.table(),
        );

        Ok(())
    }

    fn eq(actual: ast::Module, expected: ast::Module, table: &ModuleTable) {
        let debug_actual = Debuggable::from(&actual, table);
        let debug_expected = Debuggable::from(&expected, table);

        assert!(actual == expected, format_eq(&actual, &expected, table));
    }

    fn format_eq(
        actual: &(impl DebugModuleTable + std::fmt::Debug),
        expected: &(impl DebugModuleTable + std::fmt::Debug),
        table: &ModuleTable,
    ) -> String {
        format!(
                "actual != expected\nactual: {:#?}\nexpected: {:#?}\n\nabbreviated:\n\nactual: {:#?}\n\nexpected: {:#?}\n",
                actual, expected, Debuggable::from(actual, table), Debuggable::from(expected, table)
            )
    }

    fn print_parse_error(e: ParseError, codemap: &CodeMap) -> ! {
        let error = Diagnostic::new(Severity::Error, e.description)
            .with_label(Label::new_primary(e.span.to_codespan()));
        let writer = StandardStream::stderr(ColorChoice::Auto);
        emit(
            &mut writer.lock(),
            &codemap,
            &error,
            &language_reporting::DefaultConfig,
        )
        .unwrap();
        panic!("Parse Error");
    }

    #[derive(Debug, new)]
    struct Annotations {
        codemap: CodeMap,
        table: ModuleTable,
        spans: HashMap<u32, Vec<Span>>,
        lines: HashMap<String, u32>,
    }

    trait Position: Copy {
        fn pos(&self) -> (&str, u32);
    }

    impl Position for (&str, u32) {
        fn pos(&self) -> (&str, u32) {
            (self.0, self.1)
        }
    }

    impl Annotations {
        fn get(&self, pos: impl Position) -> Span {
            let (name, pos) = pos.pos();

            let line = self.lines.get(name).expect(&format!(
                "Wrong line name {}, names={:?}",
                name,
                self.lines.keys()
            ));

            let spans = self.spans.get(line).expect(&format!(
                "Wrong line number {}, len={}",
                line,
                self.spans.len()
            ));

            spans[pos as usize]
        }

        fn codemap(&mut self) -> &mut CodeMap {
            &mut self.codemap
        }

        fn table(&mut self) -> &mut ModuleTable {
            &mut self.table
        }

        fn wrap<T>(&self, value: T, left: impl Position, right: impl Position) -> Spanned<T> {
            let span = self.span(left, right);

            Spanned::wrap_span(value, span)
        }

        fn wrap_one<T>(&self, value: T, pos: impl Position) -> Spanned<T> {
            Spanned::wrap_span(value, self.get(pos))
        }

        fn mode(&self, pos: impl Position) -> Spanned<Mode> {
            let src = self.src(pos);
            let mode = src.into();

            self.wrap_one(mode, pos)
        }

        fn op(&self, pos: impl Position) -> Spanned<ast::Op> {
            let src = self.src(pos);

            match src {
                "+" => self.wrap_one(ast::Op::Add, pos),
                other => panic!("Unexpected operator {:?}", other),
            }
        }

        fn pat_ident(&self, pos: impl Position) -> Spanned<ast::Pattern> {
            let id = self.ident(pos);
            self.wrap_one(ast::Pattern::Identifier(id, None), pos)
        }

        fn ty(&self, line: &str, start: u32) -> Spanned<ast::Type> {
            self.wrap_one(
                ast::Type::new(None, self.ident((line, start))),
                (line, start),
            )
        }

        fn ty_mode(&self, line: &str, start: u32) -> Spanned<ast::Type> {
            self.wrap(
                ast::Type::new(
                    Some(self.mode((line, start))),
                    self.ident((line, start + 1)),
                ),
                (line, start),
                (line, start + 1),
            )
        }

        fn field(&self, line: &str, start: u32) -> ast::Field {
            ast::Field::new(
                self.ident((line, start)),
                self.ty(line, start + 1),
                self.span((line, start), (line, start + 1)),
            )
        }

        fn field_mode(&self, line: &str, start: u32) -> ast::Field {
            ast::Field::new(
                self.ident((line, start)),
                self.ty_mode(line, start + 1),
                self.span((line, start), (line, start + 2)),
            )
        }

        fn shorthand(&self, pos: impl Position) -> ast::ConstructField {
            let id = self.ident(pos);

            ast::ConstructField::Shorthand(id)
        }

        fn string(&self, pos: impl Position) -> ast::Expression {
            let string = self.src(pos);
            let id = self.table.get(string).expect(&format!(
                "Missing expected string {:?}, had {:?}",
                string,
                self.table.values()
            ));

            ast::Expression::Literal(ast::Literal::String(self.wrap_one(id, pos)))
        }

        fn refers(&self, pos: impl Position) -> ast::Expression {
            let id = self.ident(pos);
            ast::Expression::Ref(id)
        }

        fn ident(&self, pos: impl Position) -> Spanned<StringId> {
            let span = self.get(pos);

            let file = self
                .codemap
                .find_file(span.to_codespan().start())
                .expect("Missing file");

            let src = file
                .src_slice(span.to_codespan())
                .expect("Missing src_slice");

            let id = self
                .table
                .get(src)
                .expect(&format!("Missing intern for {:?}", src));

            Spanned::wrap_span(id, span)
        }

        fn src(&self, pos: impl Position) -> &str {
            let span = self.get(pos);
            let file = self
                .codemap
                .find_file(span.to_codespan().start())
                .expect("Missing file");

            let src = file
                .src_slice(span.to_codespan())
                .expect("Missing src_slice");

            src
        }

        fn span(&self, from: impl Position, to: impl Position) -> Span {
            let left = self.get(from);
            let right = self.get(to);

            left.to(right)
        }
    }

    fn extract(
        s: &str,
        mut t: ModuleTable,
        codemap: CodeMap,
        mut codespan_start: u32,
    ) -> (String, Annotations) {
        let mut span_map = HashMap::new();
        let mut lines = HashMap::new();

        let mut source = String::new();
        let mut t2 = ModuleTable::new();

        for (i, mut chunk) in s.lines().chunks(2).into_iter().enumerate() {
            let mut line = chunk.next().expect("line in chunk");
            let annotations = chunk.next().expect("annotation in chunk");

            let mut spans = vec![];

            source.push_str(&line);
            source.push('\n');

            debug!("line:        {} {:?}", i, line);
            debug!("annotations: {} {:?}", i, annotations);

            let tokens = LineTokenizer::new(&mut t2, annotations, 0);
            let mut name = None;

            for token in tokens {
                trace!("{:?}", token);
                match token {
                    Err(err) => panic!(err),
                    Ok((start, token, end)) => match token {
                        Token::Underline => spans.push(Span::from(
                            start + ByteOffset(codespan_start as i64),
                            end + ByteOffset(codespan_start as i64),
                        )),
                        Token::Name(id) => {
                            name = Some(id);
                            break;
                        }
                        Token::WsKeyword => {}
                        Token::Sigil(id) => {}
                        Token::Whitespace => {}
                    },
                }
            }

            let name = t2.lookup(name.expect("Annotation line must have a name"));
            lines.insert(name.to_string(), i as u32);
            span_map.insert(i as u32, spans);

            codespan_start += (line.len() as u32) + 1;
        }

        (source, Annotations::new(codemap, t2, span_map, lines))
    }

}
