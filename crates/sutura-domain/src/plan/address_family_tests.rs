//! [`AddressFamily`]'s choice among the addresses a host resolved to.

use core::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::plan::AddressFamily;

const FIRST_V4: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
const SECOND_V4: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2));
const FIRST_V6: IpAddr = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
const SECOND_V6: IpAddr = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2));

const DUAL_STACK: [IpAddr; 4] = [FIRST_V6, FIRST_V4, SECOND_V6, SECOND_V4];

#[test]
fn auto_keeps_the_ipv4_addresses_of_a_dual_stack_host() {
    assert_eq!(AddressFamily::Auto.keep(DUAL_STACK), vec![FIRST_V4, SECOND_V4]);
}

#[test]
fn auto_keeps_the_ipv6_addresses_of_a_host_with_no_ipv4_one() {
    assert_eq!(AddressFamily::Auto.keep([FIRST_V6, SECOND_V6]), vec![FIRST_V6, SECOND_V6]);
}

#[test]
fn ipv4_keeps_only_ipv4_addresses_and_none_of_an_ipv6_only_host() {
    assert_eq!(AddressFamily::Ipv4.keep(DUAL_STACK), vec![FIRST_V4, SECOND_V4]);
    assert_eq!(AddressFamily::Ipv4.keep([FIRST_V6, SECOND_V6]), Vec::<IpAddr>::new());
}

#[test]
fn ipv6_keeps_only_ipv6_addresses_and_none_of_an_ipv4_only_host() {
    assert_eq!(AddressFamily::Ipv6.keep(DUAL_STACK), vec![FIRST_V6, SECOND_V6]);
    assert_eq!(AddressFamily::Ipv6.keep([FIRST_V4, SECOND_V4]), Vec::<IpAddr>::new());
}

#[test]
fn a_host_that_resolved_to_nothing_keeps_nothing_in_any_family() {
    for family in [AddressFamily::Auto, AddressFamily::Ipv4, AddressFamily::Ipv6] {
        assert_eq!(family.keep([]), Vec::<IpAddr>::new(), "{family}");
    }
}

#[test]
fn a_literal_is_admitted_by_auto_and_by_its_own_family_alone() {
    assert!(AddressFamily::Auto.admits(FIRST_V4) && AddressFamily::Auto.admits(FIRST_V6));
    assert!(AddressFamily::Ipv4.admits(FIRST_V4) && !AddressFamily::Ipv4.admits(FIRST_V6));
    assert!(AddressFamily::Ipv6.admits(FIRST_V6) && !AddressFamily::Ipv6.admits(FIRST_V4));
}
