use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Block, Ident, Pat, Stmt};

enum CellType {
    Cell(Block),
    Global(Block),
}

struct Notebook {
    items: Vec<CellType>,
}

impl Parse for Notebook {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut items = Vec::new();
        while !input.is_empty() {
            let kw: Ident = input.parse()?;
            let block: Block = input.parse()?;

            if kw == "cell" {
                items.push(CellType::Cell(block));
            } else if kw == "global" {
                items.push(CellType::Global(block));
            } else {
                return Err(syn::Error::new(kw.span(), "expected `cell` or `global`"));
            }
        }
        Ok(Notebook { items })
    }
}

#[proc_macro]
pub fn notebook(input: TokenStream) -> TokenStream {
    let input2 = proc_macro2::TokenStream::from(input);

    let nb = match syn::parse2::<Notebook>(input2.clone()) {
        Ok(nb) => nb,
        Err(e) => {
            let mut err = e.to_compile_error();

            // RUST-ANALYZER RESILIENT FALLBACK:
            // Instead of dumping raw `global {}` and `cell {}` syntax (which RA rejects),
            // we manually parse the token stream, dump global items to the module scope,
            // and dump cell items into a dummy function so local variables remain valid!
            let mut global_fallback = proc_macro2::TokenStream::new();
            let mut cell_fallback = proc_macro2::TokenStream::new();

            let mut iter = input2.into_iter().peekable();
            while let Some(tt) = iter.next() {
                if let proc_macro2::TokenTree::Ident(ref id) = tt {
                    if id == "global" {
                        if let Some(proc_macro2::TokenTree::Group(g)) = iter.peek() {
                            if g.delimiter() == proc_macro2::Delimiter::Brace {
                                global_fallback.extend(g.stream());
                                iter.next();
                            }
                        }
                    } else if id == "cell" {
                        if let Some(proc_macro2::TokenTree::Group(g)) = iter.peek() {
                            if g.delimiter() == proc_macro2::Delimiter::Brace {
                                cell_fallback.extend(g.stream());
                                iter.next();
                            }
                        }
                    }
                }
            }

            err.extend(global_fallback);
            err.extend(quote! {
                fn _ra_fallback() {
                    #cell_fallback
                }
            });

            return TokenStream::from(err);
        }
    };

    let mut cell_tokens = Vec::new();
    let mut global_tokens = Vec::new();
    let mut cell_count = 0usize;

    for item in nb.items {
        match item {
            CellType::Global(block) => {
                // Just dump the statements directly into the module scope!
                let stmts = &block.stmts;
                global_tokens.push(quote! { #(#stmts)* });
            }
            CellType::Cell(block) => {
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
                let idx = cell_count; // Freeze current index for this cell

                let cell_gen = quote! {
                    let is_dirty = flags.get(#idx).copied().unwrap_or(true);

                    let #export_tuple = dioxus_devtools::subsecond::notebook_engine::memoize(#idx, is_dirty, || {
                        #(#stmts)*
                        #export_tuple
                    });
                };

                cell_tokens.push(cell_gen);
                cell_count += 1;
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
