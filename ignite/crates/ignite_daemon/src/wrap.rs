//! The compiler wrapper and the test runner: `RUSTC_WRAPPER` and Cargo's target runner point at
//! the `ignite` executable, which records each compile unit's and each test process's cost.
//!
//! Stub. The ticket Thrh4.25 (Supervisor, `ignite run` and the compiler wrapper) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

/// The hidden `ignite rustc-wrap <rustc> <args...>` subcommand.
pub fn rustc_wrap(args: &[String]) -> i32 {
    todo!("Thrh4.25 (Supervisor, `ignite run` and the compiler wrapper)")
}

/// The hidden `ignite runner <test binary> <args...>` subcommand.
pub fn runner(args: &[String]) -> i32 {
    todo!("Thrh4.25 (Supervisor, `ignite run` and the compiler wrapper)")
}
