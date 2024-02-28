//! Macros to make common workflows easier

/// Run a block of code each tick, resolving the specified list of
/// dependencies by calling `.resolve` each tick and exiting if
/// any dependency can't be resolved.
///
/// Dependencies can be of any type, but must have a `.resolve` method
/// that returns an [`Option`]
///
/// The resolved dependencies will be available within the code block
/// under the same ident. EG an [`screeps::ObjectId<screeps::Creep>`] called `creep`
/// will be resolved to a [`screeps::Creep`] also called `creep`
///
/// The code block must return whether the task is "done" (ie `false`
/// if it should run again next tick or `true` if the task is done)
///
/// Returns `false` if any dependency ever fails to resolve.
/// Returns `true` otherwise.
///
/// # Examples
///
/// ```ignore
/// use screeps::*;
/// let creep: ObjectId<Creep> = todo!("Acquire a creep ID somehow");
/// let source: ObjectId<Source> = todo!("Acquire a Source ID somehow");
/// screeps_async_runtime::each_tick!(creep, source, {
///     creep.harvest(source);
///     false // Do this forever
/// });
/// ```
#[macro_export]
macro_rules! each_tick {
    ($($dep:ident),*, $body:block) => {{
        let inner = || async move {
            loop {
                $(
                    let $dep = $dep.resolve()?;
                )*
                let func = || async move $body;
                if !func().await {
                    return Some(());
                }

                ::screeps_async::yield_tick().await;
            }
        };

        inner().await
    }}
}
