use notebook_macros::notebook;
use std::sync::Arc;

// --- THE USER'S NOTEBOOK ---

notebook! {
    // Cell 0
    cell {
        println!("\n[Cell 0] Fetching dataset...");
        let multiplier = 10;
        let data = Arc::new(vec![1, 2, 3, 4, 6]);
        // data.to_owned
    }
    global {
        struct Hi {
            a: i64,
        }
    }

    // Cell 1
    cell {
        let _ = Hi {a: 2};
        println!("[Cell 1] Calculating...");
        let processed: Vec<i32> = data.iter().map(|x| x * multiplier).collect();
    }

    // Cell 2
    cell {
        println!("[Cell 2] Outputting: {:?}", processed);
    }

    cell {
        println!("[Cell 3] Outputting: {:?}", processed);
    }
}

fn main() {
    dioxus_devtools::connect_subsecond();
    // We can do standard setup here now!
    println!("Initializing environment...");
    let data = Arc::new(vec![1, 2, 3, 4, 6]);

    // Hand off control to the interactive loop
    dioxus_devtools::subsecond::notebook_engine::run_interactive(run_notebook);
}
