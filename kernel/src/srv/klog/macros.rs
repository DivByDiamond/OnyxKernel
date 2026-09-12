//! kdbg!/kinf!/kwrn!/kerr!/kpanic! macros — public logging API of `klog`.

// The enabled() guard sits in the macro (not inside emit) so a filtered-out
// call also skips building the Arg slice and the vformat work in emit.
#[macro_export]
macro_rules! kdbg { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { if $crate::srv::klog::enabled($crate::srv::klog::Level::Dbg) { $crate::srv::klog::emit($crate::srv::klog::Level::Dbg, $tag, $fmt, &[$($arg),*]) } }; }
#[macro_export]
macro_rules! kinf { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { if $crate::srv::klog::enabled($crate::srv::klog::Level::Inf) { $crate::srv::klog::emit($crate::srv::klog::Level::Inf, $tag, $fmt, &[$($arg),*]) } }; }
#[macro_export]
macro_rules! kwrn { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { if $crate::srv::klog::enabled($crate::srv::klog::Level::Wrn) { $crate::srv::klog::emit($crate::srv::klog::Level::Wrn, $tag, $fmt, &[$($arg),*]) } }; }
#[macro_export]
macro_rules! kerr { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { if $crate::srv::klog::enabled($crate::srv::klog::Level::Err) { $crate::srv::klog::emit($crate::srv::klog::Level::Err, $tag, $fmt, &[$($arg),*]) } }; }
// Panic output is intentionally unconditional.
#[macro_export]
macro_rules! kpanic { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { { $crate::srv::klog::emit($crate::srv::klog::Level::Err, $tag, $fmt, &[$($arg),*]); $crate::srv::klog::halt() } }; }
