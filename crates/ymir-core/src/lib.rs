//! Core types, traits, and provenance tracking for the Ymir pipeline.
//!
//! This crate defines the foundational types shared across all pipeline stages,
//! including the `Sourced<T>` wrapper for provenance tracking, override file
//! parsing, dependency graph resolution, and deterministic PRNG management.

pub mod dependency_graph;
pub mod override_file;
pub mod prng;
pub mod provenance;
pub mod sourced;
