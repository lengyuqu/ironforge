pub mod cargo;
pub mod generic;
pub mod npm;

pub use cargo::CargoAdapter;
pub use generic::GenericAdapter;
pub use npm::NpmAdapter;
