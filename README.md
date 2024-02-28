# Screeps Async
Tick-aware Async Runtime for Screeps

[![Crates.io][crates-badge]][crates-url]
[![CI][actions-badge]][actions-url]

[crates-badge]: https://img.shields.io/crates/v/screeps-async.svg
[crates-url]: https://crates.io/crates/screeps-async
[actions-badge]: https://github.com/scottbot95/screeps-async/actions/workflows/ci.yml/badge.svg
[actions-url]: https://github.com/scottbot95/screeps-async/actions/workflows/ci.yml

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

### The `each_tick!` macro 

In synchronous Screeps code, one common pattern is to use a state machine and decide what logic to use based off the state.
With asynchronous programming you are liberated from the shackles of the tick and free to write a single function whose
execution spans across ticks. This allows you to "unroll" a state machine into a single function that execute top-to-bottom.
In such cases you will likely still need to write code that loops each tick until moving on (ie repeatedly calling `move_to`).

However, it is not safe to store references to `RoomObjects` across ticks as they may be destroyed/go out of vision. 
The solution to this problem is to store `ObjectId`s instead and `.resolve()` them each tick.

The `each_tick!` macro may be used to help reduce the boilerplate of looping and `.resolve()`'ing each tick. 
`each_tick!` accepts a list of dependencies (usually `ObjectId`s) and will call `.resolve()` on them each tick, making the
resolved value available to the provided code block under the same name as the dependency (see example for details)


```rust
async fn collect_and_upgrade(creep: ObjectId<Creep>, source: ObjectId<Source>, controller: ObjectId<StructureController>) {
    // Collect until full
    each_tick!(creep, source, {
        if creep.store().get_free_capacity(Some(ResourceType::Energy)) == 0 {
            return Some(());
        }
        
        if let Err(_) = creep.harvest(&source) {
            let _ = creep.move_to(&source);
        }
        
        // Do this forever
        None
    }).await.expect("Creep or source is gone!");
    
    each_tick!(creep, controller, {
        if creep.store().get_used_capacity(Some(ResourceType::Energy)) == 0 {
            return Some(());
        }
        
        if let Err(_) = creep.upgrade_controller(&controller) {
            let _ = creep.move_to(&controller);
        }
        
        // Do this forever
        None
    }).await.expect("Creep or controller is gone!");
}

```