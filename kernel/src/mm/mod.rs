pub mod heap;
// Global-allocator bridge is target-only; host tests use the std allocator.
#[cfg(not(test))]
pub mod kalloc;
pub mod pmm;
pub mod vmm;

#[cfg(test)]
mod tests;
