//! The language server, run with `minlang lsp`. It speaks LSP over standard
//! input and output, and analyzes each open document whenever it changes.

pub mod analysis;

use std::collections::HashMap;
use std::error::Error;

use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{
    Completion, DocumentHighlightRequest, DocumentSymbolRequest, GotoDefinition, HoverRequest,
    References, Rename, SemanticTokensFullRequest,
};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionResponse, Diagnostic,
    DiagnosticSeverity, DocumentHighlight, DocumentSymbol, DocumentSymbolResponse,
    GotoDefinitionResponse, Hover, HoverContents, HoverProviderCapability, InitializeParams,
    Location, MarkupContent, MarkupKind, OneOf, Position, PositionEncodingKind,
    PublishDiagnosticsParams, Range, SemanticToken, SemanticTokenModifier, SemanticTokenType,
    SemanticTokens, SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensResult, SemanticTokensServerCapabilities, ServerCapabilities, SymbolKind,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri,
    WorkspaceEdit,
};

use analysis::{Analysis, CompletionKind, TokenClass, analyze, modifiers};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// How LSP positions count columns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf16,
    /// Characters, like the compiler.
    Utf32,
}

/// Converts between the compiler's character columns and LSP positions.
struct Positions<'a> {
    lines: &'a [String],
    encoding: Encoding,
}

impl Positions<'_> {
    fn lsp_position(&self, (line, column): (usize, usize)) -> Position {
        let character = match self.encoding {
            Encoding::Utf32 => column,
            Encoding::Utf16 => self.lines.get(line).map_or(column, |text| {
                text.chars()
                    .take(column)
                    .map(char::len_utf16)
                    .sum::<usize>()
                    + column.saturating_sub(text.chars().count())
            }),
        };
        Position::new(line as u32, character as u32)
    }

    fn compiler_position(&self, position: Position) -> (usize, usize) {
        let line = position.line as usize;
        let character = position.character as usize;
        let column = match self.encoding {
            Encoding::Utf32 => character,
            Encoding::Utf16 => self.lines.get(line).map_or(character, |text| {
                let mut units = 0;
                let mut column = 0;
                for c in text.chars() {
                    if units >= character {
                        break;
                    }
                    units += c.len_utf16();
                    column += 1;
                }
                column
            }),
        };
        (line, column)
    }

    fn range(&self, (start, end): ((usize, usize), (usize, usize))) -> Range {
        Range::new(self.lsp_position(start), self.lsp_position(end))
    }
}

struct Server {
    connection: Connection,
    encoding: Encoding,
    documents: HashMap<Uri, Analysis>,
}

fn file_name(uri: &Uri) -> String {
    uri.path().as_str().to_owned()
}

/// Runs the server on standard input and output until the client shuts it
/// down.
pub fn run() -> Result<()> {
    let (connection, io_threads) = Connection::stdio();

    let (id, params) = connection.initialize_start()?;
    let params: InitializeParams = serde_json::from_value(params)?;
    let utf32 = params
        .capabilities
        .general
        .and_then(|general| general.position_encodings)
        .is_some_and(|encodings| encodings.contains(&PositionEncodingKind::UTF32));
    let encoding = if utf32 {
        Encoding::Utf32
    } else {
        Encoding::Utf16
    };

    let capabilities = capabilities(encoding);
    let result = serde_json::json!({
        "capabilities": capabilities,
        "serverInfo": { "name": "minlang", "version": env!("CARGO_PKG_VERSION") },
    });
    connection.initialize_finish(id, result)?;

    let mut server = Server {
        connection,
        encoding,
        documents: HashMap::new(),
    };
    server.main_loop()?;

    let Server { connection, .. } = server;
    drop(connection);
    io_threads.join()?;
    Ok(())
}

fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TokenClass::ALL
            .iter()
            .map(|class| SemanticTokenType::new(class.name()))
            .collect(),
        token_modifiers: modifiers::NAMES
            .iter()
            .map(|name| SemanticTokenModifier::new(name))
            .collect(),
    }
}

fn capabilities(encoding: Encoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(match encoding {
            Encoding::Utf16 => PositionEncodingKind::UTF16,
            Encoding::Utf32 => PositionEncodingKind::UTF32,
        }),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".to_owned(), ":".to_owned()]),
            ..CompletionOptions::default()
        }),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: legend(),
                full: Some(SemanticTokensFullOptions::Bool(true)),
                range: None,
                ..SemanticTokensOptions::default()
            },
        )),
        ..ServerCapabilities::default()
    }
}

/// Extracts a request's parameters if it's of type `R`.
fn cast<R: lsp_types::request::Request>(
    request: Request,
) -> std::result::Result<(RequestId, R::Params), Request> {
    match request.extract(R::METHOD) {
        Ok(value) => Ok(value),
        Err(ExtractError::MethodMismatch(request)) => Err(request),
        Err(ExtractError::JsonError { method, error }) => {
            panic!("invalid parameters for {method}: {error}")
        }
    }
}

impl Server {
    fn main_loop(&mut self) -> Result<()> {
        while let Ok(message) = self.connection.receiver.recv() {
            match message {
                Message::Request(request) => {
                    if self.connection.handle_shutdown(&request)? {
                        return Ok(());
                    }
                    self.handle_request(request)?;
                }
                Message::Notification(notification) => self.handle_notification(notification)?,
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    fn respond(&self, id: RequestId, result: impl serde::Serialize) -> Result<()> {
        let response = Response::new_ok(id, result);
        self.connection.sender.send(Message::Response(response))?;
        Ok(())
    }

    fn positions<'a>(&self, analysis: &'a Analysis) -> Positions<'a> {
        Positions {
            lines: &analysis.lines,
            encoding: self.encoding,
        }
    }

    /// The document and character position a request is about.
    fn locate(&self, params: &TextDocumentPositionParams) -> Option<(&Analysis, (usize, usize))> {
        let analysis = self.documents.get(&params.text_document.uri)?;
        let position = self.positions(analysis).compiler_position(params.position);
        Some((analysis, position))
    }

    // `Uri` caches parts of itself, which clippy mistakes for mutability
    #[allow(clippy::mutable_key_type)]
    fn handle_request(&mut self, request: Request) -> Result<()> {
        let request = match cast::<HoverRequest>(request) {
            Ok((id, params)) => {
                let hover = self
                    .locate(&params.text_document_position_params)
                    .and_then(|(analysis, (line, column))| analysis.hover(line, column))
                    .map(|text| Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: text,
                        }),
                        range: None,
                    });
                return self.respond(id, hover);
            }
            Err(request) => request,
        };

        let request = match cast::<GotoDefinition>(request) {
            Ok((id, params)) => {
                let position = &params.text_document_position_params;
                let location = self
                    .locate(position)
                    .and_then(|(analysis, (line, column))| {
                        let range = analysis.definition(line, column)?;
                        Some(GotoDefinitionResponse::Scalar(Location::new(
                            position.text_document.uri.clone(),
                            self.positions(analysis).range(range),
                        )))
                    });
                return self.respond(id, location);
            }
            Err(request) => request,
        };

        let request = match cast::<References>(request) {
            Ok((id, params)) => {
                let position = &params.text_document_position;
                let locations: Vec<Location> = self
                    .locate(position)
                    .map(|(analysis, (line, column))| {
                        analysis
                            .references(line, column)
                            .into_iter()
                            .map(|range| {
                                Location::new(
                                    position.text_document.uri.clone(),
                                    self.positions(analysis).range(range),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                return self.respond(id, locations);
            }
            Err(request) => request,
        };

        let request = match cast::<DocumentHighlightRequest>(request) {
            Ok((id, params)) => {
                let highlights: Vec<DocumentHighlight> = self
                    .locate(&params.text_document_position_params)
                    .map(|(analysis, (line, column))| {
                        analysis
                            .references(line, column)
                            .into_iter()
                            .map(|range| DocumentHighlight {
                                range: self.positions(analysis).range(range),
                                kind: None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                return self.respond(id, highlights);
            }
            Err(request) => request,
        };

        let request = match cast::<Rename>(request) {
            Ok((id, params)) => {
                let position = &params.text_document_position;
                let edit = self
                    .locate(position)
                    .and_then(|(analysis, (line, column))| {
                        if !analysis.can_rename(line, column) {
                            return None;
                        }
                        let edits = analysis
                            .references(line, column)
                            .into_iter()
                            .map(|range| TextEdit {
                                range: self.positions(analysis).range(range),
                                new_text: params.new_name.clone(),
                            })
                            .collect();
                        let mut changes = HashMap::new();
                        changes.insert(position.text_document.uri.clone(), edits);
                        Some(WorkspaceEdit::new(changes))
                    });
                return self.respond(id, edit);
            }
            Err(request) => request,
        };

        let request = match cast::<DocumentSymbolRequest>(request) {
            Ok((id, params)) => {
                let symbols = self
                    .documents
                    .get(&params.text_document.uri)
                    .map(|analysis| {
                        let positions = self.positions(analysis);
                        let symbols = analysis
                            .symbols()
                            .into_iter()
                            .map(|symbol| document_symbol(symbol, &positions))
                            .collect();
                        DocumentSymbolResponse::Nested(symbols)
                    });
                return self.respond(id, symbols);
            }
            Err(request) => request,
        };

        let request = match cast::<Completion>(request) {
            Ok((id, params)) => {
                let items: Vec<CompletionItem> = self
                    .locate(&params.text_document_position)
                    .map(|(analysis, (line, column))| {
                        analysis
                            .completions(line, column)
                            .into_iter()
                            .map(|completion| CompletionItem {
                                label: completion.label,
                                kind: Some(match completion.kind {
                                    CompletionKind::Keyword => CompletionItemKind::KEYWORD,
                                    CompletionKind::Type => CompletionItemKind::STRUCT,
                                    CompletionKind::Function => CompletionItemKind::FUNCTION,
                                    CompletionKind::Variable => CompletionItemKind::VARIABLE,
                                    CompletionKind::Field => CompletionItemKind::FIELD,
                                    CompletionKind::EnumMember => CompletionItemKind::ENUM_MEMBER,
                                    CompletionKind::Module => CompletionItemKind::MODULE,
                                }),
                                detail: completion.detail,
                                ..CompletionItem::default()
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                return self.respond(id, CompletionResponse::Array(items));
            }
            Err(request) => request,
        };

        let request = match cast::<SemanticTokensFullRequest>(request) {
            Ok((id, params)) => {
                let tokens = self
                    .documents
                    .get(&params.text_document.uri)
                    .map(|analysis| self.semantic_tokens(analysis));
                return self.respond(id, tokens.map(SemanticTokensResult::Tokens));
            }
            Err(request) => request,
        };

        let response = Response::new_err(
            request.id,
            lsp_server::ErrorCode::MethodNotFound as i32,
            format!("unsupported request: {}", request.method),
        );
        self.connection.sender.send(Message::Response(response))?;
        Ok(())
    }

    fn semantic_tokens(&self, analysis: &Analysis) -> SemanticTokens {
        let positions = self.positions(analysis);
        let mut data = Vec::new();
        let (mut previous_line, mut previous_start) = (0, 0);

        for token in analysis.semantic_tokens() {
            let start = positions.lsp_position((token.line, token.column));
            let end = positions.lsp_position((token.line, token.column + token.length));
            let delta_line = start.line - previous_line;
            let delta_start = if delta_line == 0 {
                start.character - previous_start
            } else {
                start.character
            };
            data.push(SemanticToken {
                delta_line,
                delta_start,
                length: end.character - start.character,
                token_type: TokenClass::ALL
                    .iter()
                    .position(|class| *class == token.class)
                    .unwrap_or(0) as u32,
                token_modifiers_bitset: token.modifiers,
            });
            previous_line = start.line;
            previous_start = start.character;
        }

        SemanticTokens {
            result_id: None,
            data,
        }
    }

    fn handle_notification(&mut self, notification: Notification) -> Result<()> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params: lsp_types::DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let document = params.text_document;
                self.update(document.uri, &document.text, Some(document.version))?;
            }
            DidChangeTextDocument::METHOD => {
                let params: lsp_types::DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                // Full sync: the last change is the whole document
                if let Some(change) = params.content_changes.into_iter().last() {
                    let document = params.text_document;
                    self.update(document.uri, &change.text, Some(document.version))?;
                }
            }
            DidCloseTextDocument::METHOD => {
                let params: lsp_types::DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.documents.remove(&params.text_document.uri);
                self.publish(params.text_document.uri, Vec::new(), None)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Re-analyzes a document and publishes its problems.
    fn update(&mut self, uri: Uri, text: &str, version: Option<i32>) -> Result<()> {
        let analysis = analyze(text, Some(&file_name(&uri)));
        let positions = self.positions(&analysis);
        let diagnostics = analysis
            .problems
            .iter()
            .map(|problem| Diagnostic {
                range: positions.range((
                    (problem.line, problem.column),
                    (problem.line, problem.column + problem.length),
                )),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("minlang".to_owned()),
                message: problem.message.clone(),
                ..Diagnostic::default()
            })
            .collect();
        self.documents.insert(uri.clone(), analysis);
        self.publish(uri, diagnostics, version)
    }

    fn publish(&self, uri: Uri, diagnostics: Vec<Diagnostic>, version: Option<i32>) -> Result<()> {
        let params = PublishDiagnosticsParams::new(uri, diagnostics, version);
        let notification = Notification::new(PublishDiagnostics::METHOD.to_owned(), params);
        self.connection
            .sender
            .send(Message::Notification(notification))?;
        Ok(())
    }
}

#[allow(deprecated)]
fn document_symbol(symbol: analysis::Symbol, positions: &Positions) -> DocumentSymbol {
    DocumentSymbol {
        name: symbol.name,
        detail: None,
        kind: match symbol.kind {
            analysis::SymbolKind::Function => SymbolKind::FUNCTION,
            analysis::SymbolKind::Struct => SymbolKind::STRUCT,
            analysis::SymbolKind::Enum => SymbolKind::ENUM,
            analysis::SymbolKind::Variable => SymbolKind::VARIABLE,
            analysis::SymbolKind::Constant => SymbolKind::CONSTANT,
            analysis::SymbolKind::Field => SymbolKind::FIELD,
            analysis::SymbolKind::EnumMember => SymbolKind::ENUM_MEMBER,
        },
        tags: None,
        deprecated: None,
        range: positions.range(symbol.range),
        selection_range: positions.range(symbol.selection),
        children: Some(
            symbol
                .children
                .into_iter()
                .map(|child| document_symbol(child, positions))
                .collect(),
        ),
    }
}
