use dioxus_devtools::subsecond::HotFn;
use std::sync::Arc;
use std::time::Duration;

// =====================================================================
// GENERATOR VIEW: The State Structs
// Pure data — no methods, no mutation. Cells are pure functions.
// When a cell's output struct gains/loses fields, the runtime detects
// the code change via ptr_address() and drops the cached Arc so the
// new layout is never aliased with old memory.
// =====================================================================
#[derive(Debug)]
pub struct Cell1State {
    pub data: Vec<i32>,
    pub multiplier: i32,
}

#[derive(Debug)]
pub struct Cell2State {
    pub processed: Vec<i32>,
}

// =====================================================================
// GENERATOR VIEW: The Cell Functions
// Pure functions: inputs -> output state. No side effects on external
// state. println! is fine for display; it doesn't affect reactivity.
// =====================================================================

pub fn run_cell_1() -> Arc<Cell1State> {
    println!("\n[Cell 1] Executing...");
    let data = vec![1, 2, 3, 4, 5];
    let multiplier = 10;
    Arc::new(Cell1State { data, multiplier })
}

// Cell 2 explicitly declares its dependency on Cell1State via its signature.
// The proc macro will auto-derive this dependency graph from parameter types.
pub fn run_cell_2(state_1: Arc<Cell1State>) -> Arc<Cell2State> {
    println!("[Cell 2] Executing...");
    let data = &state_1.data;
    let multiplier = state_1.multiplier;

    // Try editing this math while the program is running!
    let processed: Vec<i32> = data.iter().map(|x| x * multiplier).collect();
    println!("[Cell 2] Output: {:?}", processed);

    Arc::new(Cell2State { processed })
}

// =====================================================================
// HOST RUNNER: The Reactive Loop
// Generated from the dependency graph. Cells execute in topological
// order. Modification detection uses ptr_address() — if a cell's
// function pointer changes after a patch, it was modified.
// =====================================================================

fn main() {
    dioxus_devtools::connect_subsecond();

    let mut cell_1_hot = HotFn::current(run_cell_1);
    let mut cell_2_hot = HotFn::current(run_cell_2);

    // Cached outputs — Option so we can drop on layout change.
    // Dropping the Arc before re-executing ensures we never pass
    // old-layout memory into new-layout code.
    let mut state_1: Option<Arc<Cell1State>> = None;

    // Dirty flags — true means "needs re-execution"
    let mut cell_1_dirty = true;
    let mut cell_2_dirty = true;

    // Snapshot ptrs from the previous iteration for change detection.
    // After a patch lands, ptr_address() returns the new jump table entry.
    let mut prev_cell1_ptr = cell_1_hot.ptr_address();
    let mut prev_cell2_ptr = cell_2_hot.ptr_address();

    loop {
        // --- Modification detection ---
        // Compare current function pointer addresses to previous snapshot.
        // If they differ, the cell was patched since last iteration.
        let curr_cell1_ptr = cell_1_hot.ptr_address();
        let curr_cell2_ptr = cell_2_hot.ptr_address();

        if curr_cell1_ptr != prev_cell1_ptr {
            println!("[Runtime] Cell 1 was patched — invalidating cache and marking dirty.");
            // Drop cached output so old-layout Arc is gone before re-execution.
            state_1 = None;
            cell_1_dirty = true;
            cell_2_dirty = true; // propagate to all downstream cells
            prev_cell1_ptr = curr_cell1_ptr;
        }

        if curr_cell2_ptr != prev_cell2_ptr {
            println!("[Runtime] Cell 2 was patched — marking dirty.");
            cell_2_dirty = true;
            prev_cell2_ptr = curr_cell2_ptr;
        }

        // --- Topological execution (cell 1 before cell 2) ---
        // Cell 1 has no upstream dependencies.
        if cell_1_dirty {
            state_1 = Some(cell_1_hot.call(()));
            cell_1_dirty = false;
            // Always re-run downstream after cell 1 produces new output.
            cell_2_dirty = true;
        }

        // Cell 2 depends on cell 1's output.
        if cell_2_dirty {
            if let Some(ref s) = state_1 {
                cell_2_hot.call((Arc::clone(s),));
            }
            cell_2_dirty = false;
        }

        // Brief pause so we don't busy-wait at 100% CPU between patches.
        std::thread::sleep(Duration::from_millis(50));
    }
}
