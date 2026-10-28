use super::*;
use crate::syscall::fs_sys2::sbrk_with;
use std::collections::BTreeSet;

const BASE: u64 = regs::USER_HEAP_BASE;

#[test]
fn sbrk_maps_growth_beyond_initial_64k_before_publishing_break() {
    let mut heap_brk = BASE + 65536;
    let mut pages: BTreeSet<_> = (BASE..heap_brk).step_by(4096).collect();
    let old = heap_brk;
    let returned = sbrk_with(old, 65536 + 15, |addr| {
        change_heap_break(
            &mut heap_brk,
            addr,
            |va| {
                pages.insert(va);
                Ok(())
            },
            |_, _| panic!("growth must not unmap"),
        )
        .map_or_else(Errno::as_i64, |addr| addr as i64)
    });
    assert_eq!(returned, old as i64);
    assert_eq!(heap_brk, old + 65536 + 15);
    assert_eq!(pages.len(), 33);
    assert!(pages.contains(&(BASE + 65536))); // The formerly faulting extent.
    assert!(pages.contains(&((heap_brk - 1) & !4095)));
}

#[test]
fn failed_sbrk_growth_keeps_old_break_and_retries_already_mapped_pages() {
    let mut heap_brk = BASE + 65536;
    let old = heap_brk;
    let mut pages = BTreeSet::new();
    let returned = sbrk_with(old, 3 * 4096, |addr| {
        change_heap_break(
            &mut heap_brk,
            addr,
            |va| {
                if va == old + 4096 {
                    return Err(Errno::NoMem);
                }
                pages.insert(va);
                Ok(())
            },
            |_, _| panic!("growth must not unmap"),
        )
        .map_or_else(Errno::as_i64, |addr| addr as i64)
    });
    assert_eq!(returned, Errno::NoMem.as_i64());
    assert_eq!(heap_brk, old);
    assert_eq!(pages.len(), 1); // Partial success stays owned/accounted.
    let mut allocated = 0;
    let returned = sbrk_with(old, 3 * 4096, |addr| {
        change_heap_break(
            &mut heap_brk,
            addr,
            |va| {
                if pages.insert(va) {
                    allocated += 1;
                }
                Ok(())
            },
            |_, _| panic!("growth must not unmap"),
        )
        .map_or_else(Errno::as_i64, |addr| addr as i64)
    });
    assert_eq!(returned, old as i64);
    assert_eq!(heap_brk, old + 3 * 4096);
    assert_eq!(allocated, 2);
}

#[test]
fn sbrk_rejects_overflow_and_out_of_heap_without_mapping() {
    assert_eq!(
        sbrk_with(BASE, i64::MIN, |_| panic!("invalid target")),
        Errno::NoMem.as_i64()
    );
    assert_eq!(
        sbrk_with(u64::MAX, 1, |_| panic!("overflow")),
        Errno::NoMem.as_i64()
    );
    for increment in [
        -1,
        -(BASE as i64),
        (regs::USER_HEAP_SIZE + 1) as i64,
        i64::MAX,
    ] {
        let mut heap_brk = BASE;
        let returned = sbrk_with(BASE, increment, |addr| {
            change_heap_break(
                &mut heap_brk,
                addr,
                |_| panic!("invalid target must not map"),
                |_, _| panic!("invalid target must not unmap"),
            )
            .map_or_else(Errno::as_i64, |addr| addr as i64)
        });
        assert_eq!(returned, Errno::NoMem.as_i64());
        assert_eq!(heap_brk, BASE);
    }
}

#[test]
fn sbrk_query_and_same_page_changes_do_not_touch_page_tables() {
    let mut heap_brk = BASE + 65536 + 31;
    for increment in [0, -1, 1] {
        let old = heap_brk;
        let returned = sbrk_with(old, increment, |addr| {
            change_heap_break(
                &mut heap_brk,
                addr,
                |_| panic!("live page already mapped"),
                |_, _| panic!("live partial page must remain mapped"),
            )
            .map_or_else(Errno::as_i64, |addr| addr as i64)
        });
        assert_eq!(returned, old as i64);
        assert_eq!(heap_brk, old.checked_add_signed(increment).unwrap());
    }
}

#[test]
fn shrinking_keeps_partial_live_page_and_releases_only_full_pages() {
    let mut heap_brk = BASE + 3 * 4096 + 9;
    let mut released = None;
    change_heap_break(
        &mut heap_brk,
        BASE + 4096 + 1,
        |_| panic!("shrink must not allocate"),
        |from, size| {
            released = Some((from, size));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(released, Some((BASE + 2 * 4096, 2 * 4096)));
    assert_eq!(heap_brk, BASE + 4096 + 1);
}

#[test]
fn heap_ceiling_allows_crossing_64mib_and_rejects_growth_past_256mib() {
    assert_eq!(regs::USER_HEAP_SIZE, 256 * 1024 * 1024);
    let ceiling = BASE + regs::USER_HEAP_SIZE;
    assert_eq!(ceiling, 0x1100_0000);
    assert!(ceiling < 0x2000_0000); // mmap's minimum accepted address.
    assert!(ceiling < regs::USER_TOP - regs::USER_STACK_PAGES as u64 * 4096);
    for (old, increment) in [
        (BASE + 64 * 1024 * 1024 - 4096, 8192),
        (ceiling - 4096, 4096),
    ] {
        let mut heap_brk = old;
        let mut mapped = 0;
        let returned = sbrk_with(old, increment, |addr| {
            change_heap_break(
                &mut heap_brk,
                addr,
                |va| {
                    assert!(va >= old && va < ceiling);
                    mapped += 1;
                    Ok(())
                },
                |_, _| panic!("growth must not unmap"),
            )
            .map_or_else(Errno::as_i64, |addr| addr as i64)
        });
        assert_eq!(returned, old as i64);
        assert_eq!(heap_brk, old + increment as u64);
        assert_eq!(mapped, increment / 4096);
    }
    assert_eq!(
        sbrk_with(ceiling, 1, |_| panic!("must reject before mapping")),
        Errno::NoMem.as_i64(),
    );
}
