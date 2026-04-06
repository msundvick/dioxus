use dioxus_devtools::subsecond::HotFn;
use notebook_macros::notebook;
use std::collections::HashMap;
use std::sync::Arc;

// Include the `memoize` helper function and `get_cache` logic we wrote earlier here...
static CACHE: std::sync::OnceLock<
    std::sync::Mutex<HashMap<&'static str, Box<dyn std::any::Any + Send>>>,
> = std::sync::OnceLock::new();

fn get_cache() -> &'static std::sync::Mutex<HashMap<&'static str, Box<dyn std::any::Any + Send>>> {
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

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

// Watch how clean the user input is:
notebook! {
    cell "cell_1" {
        println!("[Cell 1] Running heavy generation...");
        let data = Arc::new(vec![1, 2, 3, 4, 5]);
        let multiplier = 10;
    }

    cell "cell_2" {
        println!("[Cell 2] Running quick calculation...");
        // Because `data` and `multiplier` were exported above,
        // they are automatically in scope here!
        let processed: Vec<i32> = data.iter().map(|x| x * multiplier).collect();
        println!("Output: {:?}", processed);
    }
}

fn main() {
    dioxus_devtools::connect_subsecond();
    // 1. Setup HotFn
    let mut notebook_hot = HotFn::current(run_notebook as fn(HashMap<&'static str, bool>));

    // 2. Setup your flags and stdin loop here...
    let mut flags = HashMap::new();
    flags.insert("cell_1", true);
    flags.insert("cell_2", true);

    notebook_hot.call((flags,));
}
