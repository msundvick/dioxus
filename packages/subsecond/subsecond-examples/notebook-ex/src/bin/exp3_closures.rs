use std::thread;
use std::time::Duration;

// A hot-patched function that generates a stateful closure
fn hot_closure_generator() -> Box<dyn Fn() -> String> {
    let x = 10;
    let y = 20;

    // ---- EDIT THIS WHILE RUNNING ----
    // Try adding a new variable: `let z = 30;`
    // And change the format string to use it: `format!("x: {}, y: {}, z: {}", x, y, z)`
    Box::new(move || format!("Captured state - x: {}, y: {}", x, y))
    // ---------------------------------
}

fn main() {
    dioxus_devtools::connect_subsecond();

    println!("Starting Experiment 3: Closure Struct Layouts");
    println!("Try capturing a new variable in the closure while running!");

    loop {
        // The boundary occurs here
        dioxus_devtools::subsecond::call(|| {
            let closure = hot_closure_generator();
            let result = closure();
            println!(" {}", result);
        });

        thread::sleep(Duration::from_secs(1));
    }
}
