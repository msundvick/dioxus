use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Block, Ident, LitStr, Pat, Stmt};

struct Notebook {
    cells: Vec<Cell>,
}

struct Cell {
    name: LitStr,
    block: Block,
}

impl Parse for Notebook {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut cells = Vec::new();
        while !input.is_empty() {
            let _kw: Ident = input.parse()?;
            let name: LitStr = input.parse()?;
            let block: Block = input.parse()?;
            cells.push(Cell { name, block });
        }
        Ok(Notebook { cells })
    }
}

#[proc_macro]
pub fn notebook(input: TokenStream) -> TokenStream {
    // 1. Convert to proc_macro2::TokenStream so we can clone and manipulate it safely
    let input2 = proc_macro2::TokenStream::from(input);

    // 2. RUST-ANALYZER MAGIC: Catch parsing errors instead of panicking!
    let nb = match syn::parse2::<Notebook>(input2.clone()) {
        Ok(nb) => nb,
        Err(e) => {
            // If the user is mid-typing and the AST is invalid, emit the compile error,
            // BUT ALSO spit their raw tokens back out so RA has context for auto-complete!
            let mut err = e.to_compile_error();
            err.extend(input2);
            return TokenStream::from(err);
        }
    };

    let mut cell_tokens = Vec::new();

    for cell in nb.cells {
        let name = &cell.name;
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
            let is_dirty = flags.get(#name).copied().unwrap_or(true);

            let #export_tuple = memoize(#name, is_dirty, || {
                #(#stmts)*
                #export_tuple
            });
        };

        cell_tokens.push(cell_gen);
    }

    // 3. Add #[allow(...)] to silence the unused variable warnings
    let expanded = quote! {
        #[allow(unused_variables, unused_mut, unused_imports)]
        pub fn run_notebook(flags: std::collections::HashMap<&'static str, bool>) {
            #(#cell_tokens)*
        }
    };

    TokenStream::from(expanded)
}
