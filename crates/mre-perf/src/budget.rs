//! The performance contract, compiled in from `perf/budget.toml`.

include!(concat!(env!("OUT_DIR"), "/budget_gen.rs"));

/// The per-frame budget in nanoseconds at the reference refresh rate.
pub const FRAME_BUDGET_NS: u64 = frame::TOTAL_NS as u64;

/// The per-frame budget on the weak-device floor (integrated GPU, 60 Hz).
pub const WEAK_DEVICE_BUDGET_NS: u64 = weak_device::TOTAL_NS as u64;

/// The largest a scene node is allowed to be — exactly one cache line.
pub const NODE_BUDGET_BYTES: usize = memory::NODE_STRUCT_BYTES as usize;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_table_is_populated() {
        assert!(
            TABLE.len() >= 20,
            "budget.toml looks truncated: {} entries",
            TABLE.len()
        );
    }

    #[test]
    fn frame_stages_fit_inside_the_frame() {
        let stages = frame::INPUT_NS
            + frame::STATE_NS
            + frame::LAYOUT_NS
            + frame::PAINT_NS
            + frame::TESS_NS
            + frame::SUBMIT_NS
            + frame::HEADROOM_NS;
        assert!(
            stages <= frame::TOTAL_NS,
            "stage budgets ({stages} ns) exceed the frame budget ({} ns)",
            frame::TOTAL_NS
        );
    }

    #[test]
    fn weak_device_is_slower_than_reference() {
        // Checked at compile time: the weak-device floor must be more generous.
        const { assert!(WEAK_DEVICE_BUDGET_NS > FRAME_BUDGET_NS) };
    }

    #[test]
    fn lookup_by_name_works() {
        assert_eq!(get("frame", "total_ns"), Some(Value::Int(frame::TOTAL_NS)));
        assert_eq!(
            get("alloc", "steady_state_allocs_per_frame"),
            Some(Value::Int(0))
        );
        assert_eq!(get("frame", "nope"), None);
    }
}
