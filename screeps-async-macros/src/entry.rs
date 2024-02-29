use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{FnArg, ItemFn};

fn token_stream_with_error(mut tokens: TokenStream, error: syn::Error) -> TokenStream {
    tokens.extend(error.into_compile_error());
    tokens
}

pub fn main(_args: TokenStream, item: TokenStream) -> TokenStream {
    let input: ItemFn = match syn::parse2(item.clone()) {
        Ok(it) => it,
        Err(e) => return token_stream_with_error(item, e),
    };

    if input.sig.asyncness.is_some() {
        async_main(input)
    } else {
        sync_main(input)
    }
}

fn async_main(input: ItemFn) -> TokenStream {
    if !input.sig.inputs.is_empty() {
        let msg = "Async main functions cannot accept arguments";
        return token_stream_with_error(
            input.to_token_stream(),
            syn::Error::new_spanned(&input.sig.ident, msg),
        );
    }

    todo!("Async main not supported yet")
}

fn sync_main(input: ItemFn) -> TokenStream {
    if input.sig.inputs.len() > 1 {
        let msg = "Sync main functions can accept at most 1 argument (which is the runtime)";
        return token_stream_with_error(
            input.to_token_stream(),
            syn::Error::new_spanned(&input.sig.ident, msg),
        );
    }

    let stmts = &input.block.stmts;

    let body = if let Some(arg) = input.sig.inputs.first() {
        match arg {
            FnArg::Receiver(_) => {
                let msg = "Associated methods not supported";
                return token_stream_with_error(
                    input.to_token_stream(),
                    syn::Error::new_spanned(&input.sig.ident, msg),
                );
            }
            FnArg::Typed(arg) => {
                quote! {
                    fn inner(#arg) {
                        #(#stmts)*
                    }

                    __SCREEPS_ASYNC_RUNTIME.with_borrow_mut(|runtime| {
                        inner(runtime);
                    });
                }
            }
        }
    } else {
        quote! {
            fn inner() {
                #(#stmts)*
            }

            inner();

            __SCREEPS_ASYNC_RUNTIME.with_borrow_mut(|runtime| {
                runtime.run()
            });
        }
    };

    let ident = input.sig.ident;
    let attrs = input.attrs;
    let vis = input.vis;

    quote! {
        ::std::thread_local! {
            static __SCREEPS_ASYNC_RUNTIME: ::std::cell::RefCell<::screeps_async::runtime::ScreepsRuntime> = ::std::cell::RefCell::new(::screeps_async::runtime::Builder::new().build());
        }

        #(#attrs)*
        #vis fn #ident() {
            #body
        }
    }
}
