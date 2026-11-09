use super::*;
#[test]
fn retransmits_only_unacknowledged_prefix_and_eventually_reports_failure() {
    let _serial = crate::net::lock::test_serial();
    let mut connection = crate::net::tcp::tests::test_conn();
    connection.state = 2;
    connection.snd_una = 100;
    connection.snd_nxt = 103;
    connection.send_len = 3;
    assert_eq!(plan(&mut connection, 999_999), None);
    for retry in 1..=5 {
        assert_eq!(
            plan(&mut connection, retry * RETRY_US),
            Some(Plan {
                flags: 0x18,
                sequence: 100,
                length: 3
            })
        );
    }
    assert_eq!(plan(&mut connection, 6 * RETRY_US), None);
    assert_eq!(connection.state, 6);
}
