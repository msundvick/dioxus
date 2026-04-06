use dioxus_devtools::subsecond::HotFn;
use std::sync::Arc; // The magic for reactive snapshots

// use std::thread;
// use std::time::Duration;

// =====================================================================
// GENERATOR VIEW: The State Struct
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
// GENERATOR VIEW: The Wrapped Cells
// =====================================================================

// Cell 1 returns its state wrapped in an Arc.
pub fn run_cell_1() -> Arc<Cell1State> {
    println!("\n[Cell 1] 🔄 Executing (Heavy Task)...");

    // --- USER CODE ---
    let data = vec![1, 2, 3, 4, 5];
    let multiplier = 10;
    // -----------------

    Arc::new(Cell1State { data, multiplier })
}

// --- CELL 2 WRAPPER ---
// The AST parser detects Cell 2 needs `data` and `multiplier`.
// It maps them to the `state_1` reference.
pub fn run_cell_2(state_1: Arc<Cell1State>) -> Arc<Cell2State> {
    println!("[Execution] ⚡ Running Cell 2 (Fast calculation...)");

    // The generator re-mapped the user's variables to the struct fields
    let data = &state_1.data;
    let multiplier = state_1.multiplier;

    // ----------------- USER CODE START: CELL 2 -----------------
    // Try editing this math while the program is running!
    // e.g., change `* multiplier` to `+ multiplier`
    let processed: Vec<i32> = data.iter().map(|x| x * multiplier).collect();

    println!("Output: {:?}", processed);
    // ------------------ USER CODE END: CELL 2 ------------------
    Arc::new(Cell2State { processed })
}
// =====================================================================
// HOST RUNNER: The Reactive Loop
// =====================================================================

fn main() {
    dioxus_devtools::connect_subsecond();

    // We use Arc in the type signature. Arc is a concrete type,
    // so HotFn doesn't lock any lifetimes!
    let mut cell_1_hot = HotFn::current(run_cell_1);
    let mut cell_2_hot = HotFn::current(run_cell_2);

    let mut state_1_cache: Option<Arc<Cell1State>> = None;

    // Simulation flags
    let mut cell_1_modified = true;
    let mut cell_2_modified = true;

    loop {
        // 1. Reactive Update for Cell 1
        if cell_1_modified || state_1_cache.is_none() {
            state_1_cache = Some(cell_1_hot.call(()));
            cell_1_modified = false;
            cell_2_modified = true; // Dependency trigger!
        }

        // 2. Reactive Update for Cell 2
        if cell_2_modified {
            if let Some(ref state) = state_1_cache {
                // We clone the Arc (cheap pointer increment).
                // No borrows are held across iterations!
                cell_2_hot.call((Arc::clone(state),));
            }
            cell_2_modified = false;
        }

        // --- INTERACTIVE TEST ---
        // Force Cell 2 to stay "live" so you can edit it and see patches,
        // but keep Cell 1 memoized (it won't re-run).
        // cell_2_modified = true;

        // thread::sleep(Duration::from_secs(1));
    }
}
