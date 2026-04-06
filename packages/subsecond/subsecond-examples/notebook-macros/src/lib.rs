use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Error, FnArg, Ident, ItemFn, ItemMod, ReturnType, Type,
};

/// Mark a function as a notebook cell.
///
/// The function must:
/// - Take zero or more `Arc<CellNState>` arguments (upstream cell outputs)
/// - Return `Arc<T>` (its own output state)
///
/// The `#[notebook]` macro on the containing module will wire these together.
///
/// # Example
/// ```rust
/// #[cell]
/// pub fn cell_1() -> Arc<Cell1State> { ... }
///
/// #[cell]
/// pub fn cell_2(s: Arc<Cell1State>) -> Arc<Cell2State> { ... }
/// ```
#[proc_macro_attribute]
pub fn cell(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Just pass through — the actual work is done by #[notebook] on the module.
    item
}

/// Apply to a `mod` block containing `#[cell]`-annotated functions.
///
/// Generates:
/// - `HotFn` wrappers for each cell
/// - Cached `Option<Arc<T>>` state for each cell's output
/// - Dirty flags for each cell
/// - A `run_notebook()` function with the reactive memoization loop
///
/// Dependency graph is derived from cell function parameter types:
/// if `cell_2` takes `Arc<Cell1State>`, it depends on `cell_1`.
///
/// Emits a compile error if cycles are detected.
///
/// # Example
/// ```rust
/// #[notebook]
/// mod my_notebook {
///     #[cell]
///     pub fn cell_1() -> Arc<Cell1State> { ... }
///
///     #[cell]
///     pub fn cell_2(s: Arc<Cell1State>) -> Arc<Cell2State> { ... }
/// }
///
/// fn main() {
///     dioxus_devtools::connect_subsecond();
///     my_notebook::run_notebook();
/// }
/// ```
#[proc_macro_attribute]
pub fn notebook(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let module = parse_macro_input!(item as ItemMod);
    match notebook_impl(module) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

// ── Cell descriptor ──────────────────────────────────────────────────────────

struct CellDesc {
    /// Function name, e.g. `cell_1`
    name: Ident,
    /// Names of upstream cells this cell depends on (derived from parameter types)
    deps: Vec<Ident>,
    /// The inner type of the Arc return, e.g. `Cell1State` for `-> Arc<Cell1State>`
    output_type: Type,
    /// The original function item (passed through unchanged)
    item: ItemFn,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract the inner type T from `Arc<T>`.
fn arc_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(tp) = ty else { return None };
    let seg = tp.path.segments.last()?;
    if seg.ident != "Arc" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(ref ab) = seg.arguments else {
        return None;
    };
    let syn::GenericArgument::Type(inner) = ab.args.first()? else {
        return None;
    };
    Some(inner)
}

/// Given an `Arc<CellNState>` parameter type, return the cell name that produces it.
/// Convention: `Cell1State` → `cell_1`, `Cell2State` → `cell_2`.
/// We derive this by finding which cell's output type matches.
fn dep_cell_name_from_type(ty: &Type, cells: &[CellDesc]) -> Option<Ident> {
    let inner = arc_inner(ty)?;
    for cell in cells {
        let cell_inner = arc_inner(&cell.output_type)?;
        // Compare by stringified form (types from the same module will match)
        if quote!(#cell_inner).to_string() == quote!(#inner).to_string() {
            return Some(cell.name.clone());
        }
    }
    None
}

/// Topological sort — returns cell indices in execution order.
/// Returns Err if a cycle is detected.
fn topo_sort(cells: &[CellDesc]) -> Result<Vec<usize>, String> {
    let n = cells.len();
    let mut result = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    let mut in_stack = vec![false; n];

    fn visit(
        i: usize,
        cells: &[CellDesc],
        visited: &mut Vec<bool>,
        in_stack: &mut Vec<bool>,
        result: &mut Vec<usize>,
    ) -> Result<(), String> {
        if in_stack[i] {
            return Err(format!("Cycle detected involving cell `{}`", cells[i].name));
        }
        if visited[i] {
            return Ok(());
        }
        in_stack[i] = true;
        // Find dependency indices
        for dep_name in &cells[i].deps {
            if let Some(dep_idx) = cells.iter().position(|c| &c.name == dep_name) {
                visit(dep_idx, cells, visited, in_stack, result)?;
            }
        }
        in_stack[i] = false;
        visited[i] = true;
        result.push(i);
        Ok(())
    }

    for i in 0..n {
        visit(i, cells, &mut visited, &mut in_stack, &mut result)?;
    }
    Ok(result)
}

// ── Main implementation ───────────────────────────────────────────────────────

fn notebook_impl(mut module: ItemMod) -> Result<TokenStream2, Error> {
    let mod_name = &module.ident;
    let vis = &module.vis;

    let Some((_, ref mut items)) = module.content else {
        return Err(Error::new_spanned(
            &module.ident,
            "#[notebook] requires an inline module (mod foo { ... })",
        ));
    };

    // Collect all #[cell] functions
    let mut cells: Vec<CellDesc> = Vec::new();

    for item in items.iter() {
        let syn::Item::Fn(ref f) = item else { continue };

        let is_cell = f.attrs.iter().any(|a| a.path().is_ident("cell"));
        if !is_cell {
            continue;
        }

        // Validate return type is Arc<T>
        let ReturnType::Type(_, ref ret_ty) = f.sig.output else {
            return Err(Error::new_spanned(
                &f.sig,
                "#[cell] function must return Arc<T>",
            ));
        };
        if arc_inner(ret_ty).is_none() {
            return Err(Error::new_spanned(
                ret_ty,
                "#[cell] function must return Arc<T>",
            ));
        }

        cells.push(CellDesc {
            name: f.sig.ident.clone(),
            deps: Vec::new(), // filled in below
            output_type: *ret_ty.clone(),
            item: f.clone(),
        });
    }

    if cells.is_empty() {
        return Err(Error::new_spanned(
            mod_name,
            "#[notebook] module contains no #[cell] functions",
        ));
    }

    // Resolve dependencies from parameter types (two-pass: collect all cells first)
    let mut dep_lists: Vec<Vec<Ident>> = Vec::new();
    for cell in &cells {
        let mut deps = Vec::new();
        for param in &cell.item.sig.inputs {
            let FnArg::Typed(pt) = param else { continue };
            if let Some(dep_name) = dep_cell_name_from_type(&pt.ty, &cells) {
                deps.push(dep_name);
            }
        }
        dep_lists.push(deps);
    }
    for (cell, deps) in cells.iter_mut().zip(dep_lists) {
        cell.deps = deps;
    }

    // Topological sort
    let order = topo_sort(&cells).map_err(|msg| Error::new_spanned(mod_name, msg))?;

    // ── Code generation ─────────────────────────────────────────────────────

    // Pass through all original items (strip #[cell] attr so they compile normally)
    let mut passthrough_items: Vec<TokenStream2> = Vec::new();
    for item in items.iter() {
        let syn::Item::Fn(ref f) = item else {
            passthrough_items.push(quote!(#item));
            continue;
        };
        let is_cell = f.attrs.iter().any(|a| a.path().is_ident("cell"));
        if !is_cell {
            passthrough_items.push(quote!(#item));
            continue;
        }
        // Strip #[cell] attribute
        let mut f2 = f.clone();
        f2.attrs.retain(|a| !a.path().is_ident("cell"));
        passthrough_items.push(quote!(#f2));
    }

    // Generate HotFn variable names and cache names
    let hot_vars: Vec<Ident> = cells
        .iter()
        .map(|c| format_ident!("{}_hot", c.name))
        .collect();
    let cache_vars: Vec<Ident> = cells
        .iter()
        .map(|c| format_ident!("{}_cache", c.name))
        .collect();
    let dirty_vars: Vec<Ident> = cells
        .iter()
        .map(|c| format_ident!("{}_dirty", c.name))
        .collect();
    let prev_ptr_vars: Vec<Ident> = cells
        .iter()
        .map(|c| format_ident!("{}_prev_ptr", c.name))
        .collect();

    let fn_names: Vec<&Ident> = cells.iter().map(|c| &c.name).collect();
    let output_types: Vec<&Type> = cells.iter().map(|c| &c.output_type).collect();

    // HotFn declarations
    let hotfn_decls = fn_names.iter().zip(hot_vars.iter()).map(|(fname, hvar)| {
        quote! {
            let mut #hvar = ::dioxus_devtools::subsecond::HotFn::current(#fname);
        }
    });

    // Cache declarations
    let cache_decls = cache_vars.iter().zip(output_types.iter()).map(|(cvar, oty)| {
        quote! {
            let mut #cvar: ::std::option::Option<#oty> = None;
        }
    });

    // Dirty flag declarations (all start true for initial execution)
    let dirty_decls = dirty_vars.iter().map(|dvar| {
        quote! {
            let mut #dvar: bool = true;
        }
    });

    // Prev ptr declarations
    let prev_ptr_decls = hot_vars.iter().zip(prev_ptr_vars.iter()).map(|(hvar, pvar)| {
        quote! {
            let mut #pvar = #hvar.ptr_address();
        }
    });

    // Patch detection block
    let patch_detect = cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let hvar = &hot_vars[i];
            let pvar = &prev_ptr_vars[i];
            let dvar = &dirty_vars[i];
            let cvar = &cache_vars[i];
            let cell_name_str = cell.name.to_string();

            // Compute downstream dirty sets: all cells that depend (transitively) on this cell
            let downstream_dirty: Vec<&Ident> = cells
                .iter()
                .enumerate()
                .filter(|(j, c)| *j != i && c.deps.contains(&cell.name))
                .map(|(j, _)| &dirty_vars[j])
                .collect();

            quote! {
                {
                    let curr = #hvar.ptr_address();
                    if curr != #pvar {
                        println!("[Runtime] {} was patched — invalidating cache.", #cell_name_str);
                        #cvar = None;
                        #dvar = true;
                        #(#downstream_dirty = true;)*
                        #pvar = curr;
                    }
                }
            }
        });

    // Execution blocks in topological order
    let exec_blocks = order.iter().map(|&i| {
        let cell = &cells[i];
        let hvar = &hot_vars[i];
        let cvar = &cache_vars[i];
        let dvar = &dirty_vars[i];

        // Build argument list: for each parameter, find the matching upstream cache var
        let args: Vec<TokenStream2> = cell
            .item
            .sig
            .inputs
            .iter()
            .filter_map(|param| {
                let FnArg::Typed(pt) = param else { return None };
                // Find which cell produces this type
                let dep_name = dep_cell_name_from_type(&pt.ty, &cells)?;
                let dep_idx = cells.iter().position(|c| c.name == dep_name)?;
                let dep_cache = &cache_vars[dep_idx];
                Some(quote! { ::std::sync::Arc::clone(#dep_cache.as_ref().unwrap()) })
            })
            .collect();

        // Downstream cells that need to be marked dirty when this cell re-runs
        let downstream_dirty: Vec<&Ident> = cells
            .iter()
            .enumerate()
            .filter(|(j, c)| *j != i && c.deps.contains(&cell.name))
            .map(|(j, _)| &dirty_vars[j])
            .collect();

        if args.is_empty() {
            quote! {
                if #dvar {
                    #cvar = Some(#hvar.call(()));
                    #dvar = false;
                    #(#downstream_dirty = true;)*
                }
            }
        } else {
            quote! {
                if #dvar {
                    #cvar = Some(#hvar.call((#(#args,)*)));
                    #dvar = false;
                    #(#downstream_dirty = true;)*
                }
            }
        }
    });

    let generated = quote! {
        #vis mod #mod_name {
            use ::std::sync::Arc;

            #(#passthrough_items)*

            /// Generated reactive notebook runner.
            /// Call this from `main()` after `dioxus_devtools::connect_subsecond()`.
            #[allow(unused_assignments, unused_variables)]
            pub fn run_notebook() {
                #(#hotfn_decls)*
                #(#cache_decls)*
                #(#dirty_decls)*
                #(#prev_ptr_decls)*

                loop {
                    // Detect which cells were patched since last iteration
                    #(#patch_detect)*

                    // Execute dirty cells in topological order
                    #(#exec_blocks)*

                    ::std::thread::sleep(::std::time::Duration::from_millis(50));
                }
            }
        }
    };

    Ok(generated)
}
