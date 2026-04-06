use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Block, Ident, Pat, Stmt};

struct Notebook {
    cells: Vec<Cell>,
}

struct Cell {
    block: Block,
}

impl Parse for Notebook {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut cells = Vec::new();
        while !input.is_empty() {
            let _kw: Ident = input.parse()?; // Matches 'cell'
            let block: Block = input.parse()?;
            cells.push(Cell { block });
        }
        Ok(Notebook { cells })
    }
}

#[proc_macro]
pub fn notebook(input: TokenStream) -> TokenStream {
    let input2 = proc_macro2::TokenStream::from(input);

    let nb = match syn::parse2::<Notebook>(input2.clone()) {
        Ok(nb) => nb,
        Err(e) => {
            let mut err = e.to_compile_error();
            err.extend(input2);
            return TokenStream::from(err);
        }
    };

    let mut cell_tokens = Vec::new();
    let cell_count = nb.cells.len();

    for (idx, cell) in nb.cells.iter().enumerate() {
        let stmts = &cell.block.stmts;
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

        let cell_gen = quote! {
            // Uses the vector index!
            let is_dirty = flags.get(#idx).copied().unwrap_or(true);

            let #export_tuple = dioxus_devtools::subsecond::notebook_engine::memoize(#idx, is_dirty, || {
                #(#stmts)*
                #export_tuple
            });
        };

        cell_tokens.push(cell_gen);
    }

    let expanded = quote! {
        #[allow(unused_variables, unused_mut, unused_imports, clippy::let_and_return)]
        pub fn run_notebook(flags: std::vec::Vec<bool>) -> usize {
            #(#cell_tokens)*

            // Return the total number of cells back to the host!
            #cell_count
        }
    };

    TokenStream::from(expanded)
}
