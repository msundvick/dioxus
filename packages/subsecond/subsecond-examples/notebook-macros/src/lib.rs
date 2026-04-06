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
                        // CLONE THE ORIGINAL GROUP: This preserves the exact spans of the
                        // user's original `{` and `}`.
                        // let g_clone = g.clone();
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
                                    let mut stmts = block.stmts.clone();
                                    let mut exports = Vec::new();

                                    // Detect and extract trailing expression (no semicolon)
                                    let mut trailing_expr = None;
                                    if let Some(Stmt::Expr(expr, None)) = stmts.last() {
                                        trailing_expr = Some(expr.clone());
                                        stmts.pop(); // Remove it from the main body
                                    }

                                    for stmt in &stmts {
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

                                    // If there was a trailing expression, print it!
                                    let display_gen = if let Some(expr) = trailing_expr {
                                        quote! {
                                            let __cell_res = #expr;
                                            // Using the Debug trait to prove we can display trailing returns
                                            println!("Out[{}]: {:?}", #idx, __cell_res);
                                        }
                                    } else {
                                        quote! {}
                                    };

                                    let cell_gen = quote! {
                                        let is_dirty = flags.get(#idx).copied().unwrap_or(true);

                                        let #export_tuple = dioxus_devtools::subsecond::notebook_engine::memoize(#idx, is_dirty, || {
                                            #(#stmts)*
                                            #display_gen
                                            #export_tuple
                                        });
                                    };

                                    cell_tokens.push(cell_gen);
                                }
                                Err(_) => {
                                    // SYNTAX IS BROKEN (e.g. user typing `data.`):
                                    let idx = cell_count;
                                    let fallback_fn = quote::format_ident!("_ra_fallback_{}", idx);

                                    let mut last_span = proc_macro2::Span::call_site();
                                    for tt in group_stream.clone() {
                                        last_span = tt.span();
                                    }

                                    // ISOLATE THE BROKEN TOKENS
                                    // We use `#g_clone` directly as the body of the function.
                                    // Because `g_clone` is a `{ ... }` block, `fn _ra_fallback_0() { ... }`
                                    // becomes a perfectly valid native Rust function declaration!
                                    // `rust-analyzer` handles trailing dots inside standard functions flawlessly.
                                    // We intentionally omit `unreachable!()` and exports here to prevent
                                    // poisoning downstream type inference with `!` (never) types.
                                    let mut semi =
                                        proc_macro2::Punct::new(';', proc_macro2::Spacing::Alone);
                                    semi.set_span(last_span);
                                    let cell_gen = quote! {
                                        #[allow(dead_code)]
                                        fn #fallback_fn() { #group_stream #semi }
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
