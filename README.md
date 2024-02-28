# Screeps Async
Tick-aware Async Runtime for Screeps

### Getting Started
Add `screeps-async` to your `Cargo.toml`
```toml
[dependencies]
screeps-async = "0.1.0"
```

Create the runtime and ensure you call it once each tick:
```rust
// Use thread local just as a convenient way to avoid Send/Sync requirements as Screeps is single-threaded
// You may store the runtime however you like as long as it persists across ticks
thread_local! {
    static RUNTIME: RefCell<ScreepsRuntime> = RefCell::new(screeps_async::runtime::Builder::new().build());
}

#[wasm_bindgen(js_name = loop)]
pub fn game_loop() {
    // Tick logic that spawns some tasks
    screeps_async::spawn(async {
        println!("Hello!");
    });
    
    // Should generally be the last thing in your loop as by default
    // the runtime will keep polling for work until 90% of this tick's
    // CPU time has been exhausted. Thus, with enough scheduled work,
    // this function will run for AT LEAST 90% of the tick time
    // (90% + however long the last Future takes to poll)
    RUNTIME.with_borrow_mut(|runtime| {
        runtime.run()
    });
}
```
