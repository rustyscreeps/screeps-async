use proc_macro2::TokenStream;
use quote::quote;
use syn::ItemFn;

fn token_stream_with_error(mut tokens: TokenStream, error: syn::Error) -> TokenStream {
    tokens.extend(error.into_compile_error());
    tokens
}

pub fn main(_args: TokenStream, item: TokenStream) -> TokenStream {
    let input: ItemFn = match syn::parse2(item.clone()) {
        Ok(it) => it,
        Err(e) => return token_stream_with_error(item, e),
    };

    let body = if input.sig.asyncness.is_some() {
        async_body(&input)
    } else {
        sync_body(&input)
    };

    let ident = input.sig.ident;
    let attrs = input.attrs;
    let vis = input.vis;
    let args = input.sig.inputs;

    quote! {
        static __SCREEPS_ASYNC_INIT: std::sync::Once = std::sync::Once::new();

        #(#attrs)*
        #vis fn #ident(#args) {
             __SCREEPS_ASYNC_INIT.call_once(|| {
                screeps_async::initialize();
            });

            #body
        }
    }
}

fn async_body(input: &ItemFn) -> TokenStream {
    let stmts = &input.block.stmts;

    quote! {
        let res = ::screeps_async::block_on(async move {
            #(#stmts)*
        });

        ::screeps_async::run();

        res
    }
}

fn sync_body(input: &ItemFn) -> TokenStream {
    let stmts = &input.block.stmts;

    quote! {
        let res = {
            #(#stmts)*
        };

        ::screeps_async::run();

        res
    }
}
