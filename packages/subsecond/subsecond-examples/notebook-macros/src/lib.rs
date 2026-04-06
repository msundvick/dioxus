use proc_macro::TokenStream;
use quote::quote;
use syn::{Pat, Stmt};

#[proc_macro]
pub fn notebook(input: TokenStream) -> TokenStream {
    let input2 = proc_macro2::TokenStream::from(input);

    let mut total_cells = 0usize;
    let mut pre_iter = input2.clone().into_iter().peekable();
    while let Some(tt) = pre_iter.next() {
        if let proc_macro2::TokenTree::Ident(ref id) = tt {
            if id == "cell" {
                if let Some(proc_macro2::TokenTree::Group(g)) = pre_iter.peek() {
                    if g.delimiter() == proc_macro2::Delimiter::Brace {
                        total_cells += 1;
                    }
                }
            }
        }
    }

    let mut iter = input2.into_iter().peekable();
    let mut cell_tokens = Vec::new();
    let mut global_tokens = Vec::new();
    let mut cell_count = 0usize;

    while let Some(tt) = iter.next() {
        if let proc_macro2::TokenTree::Ident(ref id) = tt {
            let is_global = id == "global";
            let is_cell = id == "cell";

            if is_global || is_cell {
                if let Some(proc_macro2::TokenTree::Group(g)) = iter.peek() {
                    if g.delimiter() == proc_macro2::Delimiter::Brace {
                        let g_clone = g.clone();
                        let group_stream = g.stream();
                        iter.next();

                        if is_global {
                            global_tokens.push(group_stream);
                        } else {
                            let block_quote = quote! { { #group_stream } };
                            match syn::parse2::<syn::Block>(block_quote) {
                                Ok(block) => {
                                    let mut stmts = block.stmts.clone();
                                    let mut exports = Vec::new();

                                    let mut trailing_expr = None;
                                    if let Some(Stmt::Expr(expr, None)) = stmts.last() {
                                        trailing_expr = Some(expr.clone());
                                        stmts.pop();
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

                                    let display_gen = if let Some(ref expr) = trailing_expr {
                                        let span = syn::spanned::Spanned::span(expr);
                                        quote::quote_spanned! {span=>
                                            let __cell_res = #expr;
                                            dioxus_devtools::subsecond::notebook_engine::set_display(#idx, std::format!("{:#?}", __cell_res));
                                        }
                                    } else {
                                        quote! {}
                                    };

                                    let cell_gen = quote! {
                                        let is_dirty = flags.get(#idx).copied().unwrap_or(true);

                                        let __memo_res = dioxus_devtools::subsecond::notebook_engine::memoize(#idx, is_dirty, || {
                                            dioxus_devtools::subsecond::notebook_engine::get_runtime().block_on(async {
                                                #(#stmts)*
                                                #display_gen
                                                #export_tuple
                                            })
                                        });

                                        let #export_tuple = match __memo_res {
                                            Ok(res) => res,
                                            Err(_) => {
                                                return #total_cells;
                                            }
                                        };
                                    };

                                    cell_tokens.push(cell_gen);
                                }
                                Err(_) => {
                                    let idx = cell_count;
                                    let fallback_fn = quote::format_ident!("_ra_fallback_{}", idx);

                                    let cell_gen = quote! {
                                        #[allow(dead_code)]
                                        async fn #fallback_fn() #g_clone
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
        #(#global_tokens)*

        #[allow(unused_variables, unused_mut, unused_imports, clippy::let_and_return)]
        pub fn run_notebook(flags: std::vec::Vec<bool>) -> usize {
            #(#cell_tokens)*
            #total_cells
        }
    };

    TokenStream::from(expanded)
}
