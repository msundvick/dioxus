use dioxus_devtools::subsecond::HotFn;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

// =====================================================================
// 1. THE ENGINE INFRASTRUCTURE (Hidden from User)
// =====================================================================

// A global cache to hold the memoized outputs of every cell.
// We use OnceLock to safely initialize this without needing external crates like lazy_static.
static CACHE: OnceLock<Mutex<HashMap<&'static str, Box<dyn Any + Send>>>> = OnceLock::new();

fn get_cache() -> &'static Mutex<HashMap<&'static str, Box<dyn Any + Send>>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

// The Magic Memoizer:
// T is completely inferred by the closure's return type.
// No explicit typing required by the AST generator!
fn memoize<T: Clone + Send + 'static>(
    cell_id: &'static str,
    is_dirty: bool,
    cell_logic: impl FnOnce() -> T,
) -> T {
    let mut cache = get_cache().lock().unwrap();

    if is_dirty || !cache.contains_key(cell_id) {
        // Execute the cell logic
        let result = cell_logic();
        // Save a boxed clone of the tuple to the cache
        cache.insert(cell_id, Box::new(result.clone()));
        result
    } else {
        // Skip execution! Downcast the cached Any box safely to T
        let cached_any = cache.get(cell_id).unwrap();
        cached_any.downcast_ref::<T>().unwrap().clone()
    }
}

pub type DirtyFlags = HashMap<&'static str, bool>;

// =====================================================================
// 2. THE GENERATED CODE (What `syn` produces)
// =====================================================================
// Notice how the generated boundary is a strict function pointer, and
// there are ZERO type definitions required for the cell exports!

pub fn run_notebook(flags: DirtyFlags) {
    println!("\n--- 📓 Notebook Execution Start ---");

    // --- GENERATED CELL 1 ---
    let is_cell_1_dirty = flags.get("cell_1").copied().unwrap_or(true);

    // rustc infers the type of `data` and `multiplier`!
    let (data, multiplier) = memoize("cell_1", is_cell_1_dirty, || {
        println!("[Cell 1] 🔄 Running heavy DB/Network query...");

        // --- USER CODE START ---
        // We encourage Arc for large datasets so caching/cloning is instant.
        let data = Arc::new(vec![1, 2, 3, 4, 5, 6]);
        let multiplier = 10;
        // --- USER CODE END ---

        // SYN GENERATOR just needs to list the variable names exported
        (data, multiplier)
    });

    // --- GENERATED CELL 2 ---
    // If Cell 1 ran, Cell 2 MUST be marked dirty by the host.
    let is_cell_2_dirty = flags.get("cell_2").copied().unwrap_or(true);

    let (_processed_result,) = memoize("cell_2", is_cell_2_dirty, || {
        println!("[Cell 2] ⚡ Running fast computation...");

        // --- USER CODE START ---
        // The user accesses variables natively, as if they were in the same scope!
        // Try changing `x * multiplier` to `x + multiplier` while running.
        let processed_result: Vec<i32> = data.iter().map(|x| x * multiplier).collect();
        println!("Output: {:?}", processed_result);
        // --- USER CODE END ---

        // SYN GENERATOR output
        (processed_result,)
    });

    println!("--- 📓 Notebook Execution End ---");
}

// =====================================================================
// 3. THE HOST RUNNER
// =====================================================================
use std::io::{self, Write};

fn main() {
    dioxus_devtools::connect_subsecond();
    println!("--- Starting Interactive Magic Memoization ---");
    println!("Commands:");
    println!("  '1'   -> Mark Cell 1 as dirty (Forces 1 and 2 to run)");
    println!("  '2'   -> Mark Cell 2 as dirty (Only Cell 2 runs)");
    println!("  'all' -> Mark all cells dirty");
    println!("  <RET> -> Run with no dirty flags (tests caching)");
    println!("  Edit the code, save, then type a command to test hot-patching!\n");

    let mut notebook_hot = HotFn::current(run_notebook as fn(DirtyFlags));
    let mut flags: DirtyFlags = HashMap::new();

    // Initial run: everything is dirty
    flags.insert("cell_1", true);
    flags.insert("cell_2", true);

    loop {
        // 1. Execute the patch boundary with current flags
        notebook_hot.call((flags.clone(),));

        // 2. Reset flags after execution
        flags.insert("cell_1", false);
        flags.insert("cell_2", false);

        // 3. Pause and wait for user command
        print!("\n> Enter command: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let cmd = input.trim();

        // 4. Update the dirty flags based on input (Simulating the AST Hasher / UI)
        match cmd {
            "1" => {
                println!("🧠 Host: Marking Cell 1 (and dependents) as dirty.");
                flags.insert("cell_1", true);
                // DAG LOGIC: Because Cell 2 depends on Cell 1, the host must invalidate it too!
                flags.insert("cell_2", true);
            }
            "2" => {
                println!("🧠 Host: Marking Cell 2 as dirty.");
                flags.insert("cell_2", true);
            }
            "all" => {
                println!("🧠 Host: Marking all cells as dirty.");
                flags.insert("cell_1", true);
                flags.insert("cell_2", true);
            }
            _ => {
                println!("🧠 Host: Executing with clean flags (Testing memoization).");
            }
        }
    }
}
