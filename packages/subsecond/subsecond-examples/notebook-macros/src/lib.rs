use proc_macro::TokenStream;
use quote::quote;
use syn::{Pat, Stmt};

#[proc_macro]
pub fn notebook(input: TokenStream) -> TokenStream {
    let input2 = proc_macro2::TokenStream::from(input);
    let mut iter = input2.into_iter().peekable();

    let mut cell_tokens = Vec::new();
    let mut global_tokens = Vec::new();
    let mut cell_count = 0usize;

    // Manually scan the token stream for `global` and `cell` identifiers
    while let Some(tt) = iter.next() {
        if let proc_macro2::TokenTree::Ident(ref id) = tt {
            let is_global = id == "global";
            let is_cell = id == "cell";

            if is_global || is_cell {
                // Grab the { ... } block immediately following the keyword
                if let Some(proc_macro2::TokenTree::Group(g)) = iter.peek() {
                    if g.delimiter() == proc_macro2::Delimiter::Brace {
                        let group_stream = g.stream();
                        iter.next(); // Consume the group

                        if is_global {
                            global_tokens.push(group_stream);
                        } else {
                            // Try to parse JUST this cell's contents using syn
                            let block_quote = quote! { { #group_stream } };
                            match syn::parse2::<syn::Block>(block_quote) {
                                Ok(block) => {
                                    // SYNTAX IS VALID: Do our standard tuple extraction
                                    let stmts = &block.stmts;
                                    let mut exports = Vec::new();

                                    for stmt in stmts {
                                        if let Stmt::Local(local) = stmt {
                                            let pat = match &local.pat {
                                                Pat::Type(pat_type) => &*pat_type.pat,
                                                other => other,
                                            };
                                            if let Pat::Ident(pat_ident) = pat {
                                                exports.push(pat_ident.ident.clone());
                                            }
                                        }
                                    }

                                    let export_tuple = quote! { ( #( #exports, )* ) };
                                    let idx = cell_count;

                                    let cell_gen = quote! {
                                        let is_dirty = flags.get(#idx).copied().unwrap_or(true);

                                        let #export_tuple = dioxus_devtools::subsecond::notebook_engine::memoize(#idx, is_dirty, || {
                                            #(#stmts)*
                                            #export_tuple
                                        });
                                    };

                                    cell_tokens.push(cell_gen);
                                }
                                Err(_) => {
                                    // SYNTAX IS BROKEN (e.g. user typing `data.`):
                                    // We gracefully degrade ONLY this cell. We dump its raw
                                    // tokens into a local scope so RA can type-check and provide
                                    // auto-complete perfectly!
                                    let cell_gen = quote! {
                                        {
                                            #group_stream
                                        }
                                    };
                                    cell_tokens.push(cell_gen);
                                }
                            }
                            cell_count += 1;
                        }
                    }
                }
            }
        }
    }

    let expanded = quote! {
        // --- EXPLICIT GLOBALS ---
        #(#global_tokens)*

        #[allow(unused_variables, unused_mut, unused_imports, clippy::let_and_return)]
        pub fn run_notebook(flags: std::vec::Vec<bool>) -> usize {
            #(#cell_tokens)*
            #cell_count
        }
    };

    TokenStream::from(expanded)
}
