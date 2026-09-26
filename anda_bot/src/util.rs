pub mod file_uri;
pub mod fs;
pub mod http_client;
pub mod json_schema;
pub mod locale;
pub mod request_meta;
pub mod text;
pub mod tool_response;
pub mod windows_process;

/// Polls `future` behind a type-erased box, as an inlining barrier.
///
/// An optimizer inlines an async function's state machine into its only
/// caller, and the merged poll frame keeps a separate slot for every await it
/// absorbed: in a development build, one browser agent turn that opened a
/// session briefing reached 2 MiB, the default worker stack. A virtual call
/// cannot be inlined, so the caller's frame stays its own size. Use it where a
/// subsystem is entered, not on every await.
pub fn boxed<'a, T>(future: impl Future<Output = T> + Send + 'a) -> anda_core::BoxFut<'a, T> {
    Box::pin(future)
}
