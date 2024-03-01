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

    if input.sig.asyncness.is_some() {
        async_main(input)
    } else {
        sync_main(input)
    }
}

fn async_main(_input: ItemFn) -> TokenStream {
    todo!("Async main not supported yet")
}

fn sync_main(input: ItemFn) -> TokenStream {
    let stmts = &input.block.stmts;

    let body = quote! {
        #(#stmts)*

        ::screeps_async::run();
    };

    let ident = input.sig.ident;
    let attrs = input.attrs;
    let vis = input.vis;
    let args = input.sig.inputs;

    quote! {
        #(#attrs)*
        #vis fn #ident(#args) {
            #body
        }
    }
}
