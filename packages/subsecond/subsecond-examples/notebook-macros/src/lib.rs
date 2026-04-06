use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{parse_macro_input, Block, Ident, LitStr, Pat, Stmt, Token};

// 1. Define our custom AST nodes
struct Notebook {
    cells: Vec<Cell>,
}

struct Cell {
    name: LitStr,
    block: Block,
}

// 2. Implement parsing for our custom syntax
impl Parse for Notebook {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut cells = Vec::new();
        while !input.is_empty() {
            // Parse the keyword `cell`, then a string literal, then a `{}` block
            let _kw: Ident = input.parse()?;
            let name: LitStr = input.parse()?;
            let block: Block = input.parse()?;
            cells.push(Cell { name, block });
        }
        Ok(Notebook { cells })
    }
}

// 3. The actual macro generator
#[proc_macro]
pub fn notebook(input: TokenStream) -> TokenStream {
    let nb = parse_macro_input!(input as Notebook);

    let mut cell_tokens = Vec::new();

    for cell in nb.cells {
        let name = &cell.name;
        let stmts = &cell.block.stmts; // <--- 1. Extract the statements
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
            let is_dirty = flags.get(#name).copied().unwrap_or(true);

            let #export_tuple = memoize(#name, is_dirty, || {
                #(#stmts)* // <--- 2. Unpack the statements without curly braces!

                #export_tuple
            });
        };

        cell_tokens.push(cell_gen);
    }

    // 7. Wrap it all in our FFI-friendly boundary
    let expanded = quote! {
        pub fn run_notebook(flags: std::collections::HashMap<&'static str, bool>) {
            #(#cell_tokens)*
        }
    };

    TokenStream::from(expanded)
}
