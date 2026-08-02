#![forbid(unsafe_code)]

mod descriptor;

pub use descriptor::{
    CommandApproval, DescriptorError, DescriptorRegistry, DescriptorSource, ExecutableCandidate,
    LanguageServerDescriptor, ResolvedCommand, built_in_descriptors, registry_from_json,
};
