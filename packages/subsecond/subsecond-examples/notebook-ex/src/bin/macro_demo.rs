/// Demonstration of the #[notebook] proc macro.
///
/// The macro derives the dependency graph from function signatures:
/// - cell_1 takes no arguments → no upstream dependencies
/// - cell_2 takes Arc<Cell1State> → depends on cell_1
///
/// It generates:
/// - HotFn wrappers for each cell
/// - Option<Arc<T>> caches with ptr_address()-based invalidation
/// - A run_notebook() function with topological execution order
use notebook_macros::notebook;

// State structs live outside the module so they're visible to callers.
// In a real codegen scenario these would also be generated.
#[derive(Debug)]
pub struct Cell1State {
    pub data: Vec<i32>,
    pub multiplier: i32,
}

#[derive(Debug)]
pub struct Cell2State {
    pub processed: Vec<i32>,
}

#[notebook]
mod my_notebook {
    use super::{Cell1State, Cell2State};

    #[cell]
    pub fn cell_1() -> Arc<Cell1State> {
        println!("[Cell 1] Executing...");
        Arc::new(Cell1State {
            data: vec![1, 2, 3, 4, 5],
            multiplier: 10,
        })
    }

    #[cell]
    pub fn cell_2(state: Arc<Cell1State>) -> Arc<Cell2State> {
        println!("[Cell 2] Executing...");
        let processed = state.data.iter().map(|x| x * state.multiplier).collect();
        println!("[Cell 2] Output: {:?}", processed);
        Arc::new(Cell2State { processed })
    }
}

fn main() {
    dioxus_devtools::connect_subsecond();
    my_notebook::run_notebook();
}
