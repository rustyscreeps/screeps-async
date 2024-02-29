//! Macros to make common workflows easier

pub use screeps_async_macros::*;

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
/// The code block must return an [`Option`]. If the code block returns [`None`],
/// then execution will continue next tick. If the code block returns [`Some`],
/// then looping will cease and `each_tick!` will return whatever the value returned by the code block
///
/// Returns [`Some`] if the inner block ever returned [`Some`]
///
/// # Errors
///
/// Returns [`None`] if any dependency ever fails to resolve
///
/// # Examples
///
/// ```
/// use screeps::*;
/// async fn harvest_forever(creep: ObjectId<Creep>, source: ObjectId<Source>) {
///     screeps_async::each_tick!(creep, source, {
///         let _ = creep.harvest(&source);
///         None::<()> // do this forever
///     }).await;
/// }
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
                if let Some(ret) = func().await {
                    return Some(ret);
                }

                ::screeps_async::time::yield_tick().await;
            }
        };

        inner()
    }}
}
