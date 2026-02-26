use crate::*;

#[test]
fn test_namespaces_exist() {
    if !netns_available() {
        eprintln!("SKIP: netns not available — run sudo ./scripts/netns-up.sh first");
        return;
    }

    let out_a =
        netns_exec(NS_A, &["ip", "link", "show", VETH_A]).expect("veth-a should exist in summit-a");
    assert!(out_a.contains(VETH_A), "veth-a not found in summit-a");

    let out_b =
        netns_exec(NS_B, &["ip", "link", "show", VETH_B]).expect("veth-b should exist in summit-b");
    assert!(out_b.contains(VETH_B), "veth-b not found in summit-b");

    println!("Both namespaces exist with correct interfaces.");
}

#[test]
fn test_link_local_addresses_assigned() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }

    let addr_a = link_local_addr(NS_A, VETH_A).expect("summit-a should have a link-local address");
    let addr_b = link_local_addr(NS_B, VETH_B).expect("summit-b should have a link-local address");

    println!("summit-a: {addr_a}");
    println!("summit-b: {addr_b}");

    assert!(
        addr_a.starts_with("fe80::"),
        "expected link-local in summit-a"
    );
    assert!(
        addr_b.starts_with("fe80::"),
        "expected link-local in summit-b"
    );
    assert_ne!(addr_a, addr_b, "addresses should be different");
}

#[test]
fn test_ping_a_to_b() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }

    let addr_b_raw =
        link_local_addr(NS_B, VETH_B).expect("summit-b should have a link-local address");
    let addr_b = addr_b_raw
        .split('%')
        .next()
        .map(|a| format!("{a}%{VETH_A}"))
        .unwrap();

    println!("Pinging {addr_b} from summit-a...");
    let result = netns_exec(NS_A, &["ping", "-6", "-c", "3", "-W", "2", &addr_b]);
    match &result {
        Ok(out) => println!("{out}"),
        Err(e) => panic!("ping6 from summit-a to summit-b failed: {e}"),
    }
    assert!(result.is_ok());
}

#[test]
fn test_ping_b_to_a() {
    if !netns_available() {
        eprintln!("SKIP: netns not available");
        return;
    }

    let addr_a_raw =
        link_local_addr(NS_A, VETH_A).expect("summit-a should have a link-local address");
    let addr_a = addr_a_raw
        .split('%')
        .next()
        .map(|a| format!("{a}%{VETH_B}"))
        .unwrap();

    println!("Pinging {addr_a} from summit-b...");
    let result = netns_exec(NS_B, &["ping", "-6", "-c", "3", "-W", "2", &addr_a]);
    match &result {
        Ok(out) => println!("{out}"),
        Err(e) => panic!("ping6 from summit-b to summit-a failed: {e}"),
    }
    assert!(result.is_ok());
}
