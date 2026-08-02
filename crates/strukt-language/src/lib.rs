#![forbid(unsafe_code)]

mod descriptor;
mod discovery;
mod feature;
mod framing;
mod position;
mod protocol;

pub use descriptor::{
    CommandApproval, DescriptorError, DescriptorRegistry, DescriptorSource, ExecutableCandidate,
    LanguageServerDescriptor, ResolvedCommand, built_in_descriptors, registry_from_json,
};
pub use discovery::{
    ApprovalStatus, DiscoveredServer, DiscoveryError, DiscoveryOutcome, discover,
    load_workspace_registry, select_descriptor,
};
pub use feature::{
    CompletionInsertion, CompletionItem, DefinitionAccess, DefinitionTarget, Diagnostic,
    DiagnosticSeverity, DocumentUri, FeatureError, LanguageRange, MarkupContent,
    normalize_completion_items, sanitize_hover_markdown,
};
pub use framing::{Frame, FrameDecoder, FrameError, FrameLimits, encode_frame};
pub use position::{
    LspPosition, PositionEncoding, PositionError, ScalarPosition, from_lsp_position,
    to_lsp_position,
};
pub use protocol::{
    IncomingMessage, NotificationMessage, ProtocolError, RequestId, RequestIdAllocator,
    RequestMessage, ResponseMessage, ResponseRouter, bounded_error_text, parse_message,
};
