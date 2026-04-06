use std::sync::Arc;
use std::thread;
use std::time::Duration;

// Simulated compiled Cell A
fn source_data() -> Arc<Vec<i32>> {
    Arc::new(vec![1, 2, 3, 4, 5])
}

// Simulated compiled Cell B (The one we will hot-patch)
fn process_data(data: Arc<Vec<i32>>) -> Arc<Vec<i32>> {
    // ---- EDIT THIS WHILE RUNNING ----
    let processed: Vec<i32> = data.iter().map(|n| n * 2).collect();
    println!("Processed Data: {:?}", processed);
    // ---------------------------------
    Arc::new(processed)
}

fn main() {
    dioxus_devtools::connect_subsecond();

    println!("Starting Experiment 2: Reactive Arc Passing");
    println!("Try changing 'n * 2' to 'n * 100' in the source code!");

    // The host caches the state of Cell A (memoization)
    let cell_a_cache = source_data();

    loop {
        let current_data = Arc::clone(&cell_a_cache);

        // Subsecond hot-patch boundary
        let _cell_b_cache = dioxus_devtools::subsecond::call(|| process_data(current_data.clone()));

        thread::sleep(Duration::from_secs(1));
    }
}
