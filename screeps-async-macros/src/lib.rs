#![warn(missing_docs)]

//! Macros used with screeps-async

mod entry;

use proc_macro::TokenStream;

/// Helper to wrap your main loop entrypoint in code that will automatically configure
/// the ScreepsRuntime and invoke it at the end of each tick.
///
/// The wrapped function may be `async` in which case the body will be passed to
/// [screeps_async::block_on] before calling [screeps_async::run]
#[proc_macro_attribute]
pub fn main(args: TokenStream, item: TokenStream) -> TokenStream {
    entry::main(args.into(), item.into()).into()
}
