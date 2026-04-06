use std::any::Any;
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

// The Host's state registry
type StateRegistry = HashMap<String, Box<dyn Any>>;

// The function we will hot-patch
fn hot_cell_execution(registry: &mut StateRegistry) {
    // Attempt to read previous state
    let count = registry
        .get("counter")
        .and_then(|any| any.downcast_ref::<i32>())
        .copied()
        .unwrap_or(0);

    // ---- EDIT THIS WHILE RUNNING ----
    let new_count = count + 2;
    println!("Cell executed! Counter is now: {}", new_count);
    // ---------------------------------

    // Save state back to host
    registry.insert("counter".to_string(), Box::new(new_count));
}

fn main() {
    dioxus_devtools::connect_subsecond();

    let mut registry = StateRegistry::new();

    println!("Starting Experiment 1: Typed Registry");
    println!("Try changing 'count + 1' to 'count + 10' in the source code!");

    loop {
        // Subsecond hot-patch boundary
        dioxus_devtools::subsecond::call(|| {
            println!("s2");
            hot_cell_execution(&mut registry);
        });

        thread::sleep(Duration::from_secs(1));
    }
}
