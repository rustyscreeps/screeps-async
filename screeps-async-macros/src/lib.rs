#![warn(missing_docs)]

//! Macros used with screeps-async

mod entry;

use proc_macro::TokenStream;

/// Helper to wrap your main loop entrypoint in code that will automatically configure
/// the ScreepsRuntime and invoke it at the end of each tick.
///
/// The wrapped function can optionally take a parameter of type `&mut ScreepsRuntime` to allow
/// for invoking the runtime at a different time in the tick
/// (although it should still only be called once per tick)
#[proc_macro_attribute]
pub fn main(args: TokenStream, item: TokenStream) -> TokenStream {
    entry::main(args.into(), item.into()).into()
}
