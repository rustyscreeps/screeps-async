# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/rustyscreeps/screeps-async/compare/screeps-async-v0.1.1...screeps-async-v0.2.0) - 2024-03-05

### Added
- *(spawn)* `JobHandle` now supports cancellation.

### Fixed
- Improve robustness of tests

### Other
- Fix example expansion of `#[main]` macro

## [0.1.1](https://github.com/rustyscreeps/screeps-async/compare/screeps-async-v0.1.0...screeps-async-v0.1.1) - 2024-03-03

### Fixed
- Typo in module docs
- Add missing README to crate artifacts

## [0.1.0](https://github.com/rustyscreeps/screeps-async/releases/tag/screeps-async-v0.1.0) - 2024-03-03

Initial Release!

### Added
- *(spawn)* `spawn` function to run async tasks in the background
- *(block_on)* Add `block_on` function to allow blocking on a future
- *(yield_now)* Add `yield_now` helper to yield execution back to scheduler without delaying to the next tick
- *(each_tick)* Add `each_tick!` macro to run resolve a set of dependencies an async block each tick
