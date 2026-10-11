use std::collections::HashSet;
use std::error::Error;

use orders::{billing, report, shipping, Order, OrderStatus, ParseStatusError};

const ALL: [OrderStatus; 4] = [
    OrderStatus::Pending,
    OrderStatus::Paid,
    OrderStatus::Shipped,
    OrderStatus::Cancelled,
];

#[test]
fn names_round_trip() {
    for (status, name) in ALL.into_iter().zip(["pending", "paid", "shipped", "cancelled"]) {
        assert_eq!(status.as_str(), name);
        assert_eq!(status.to_string(), name);
        assert_eq!(name.parse::<OrderStatus>().unwrap(), status);
    }
    let unique: HashSet<OrderStatus> = ALL.into_iter().collect();
    assert_eq!(unique.len(), 4);
}

#[test]
fn unknown_names_are_rejected_with_a_real_error() {
    let err: ParseStatusError = "Paid".parse::<OrderStatus>().unwrap_err();
    assert!(err.to_string().contains("Paid"), "{err}");
    let boxed: Box<dyn Error> = Box::new("lost".parse::<OrderStatus>().unwrap_err());
    assert!(boxed.to_string().contains("lost"));
    assert!("".parse::<OrderStatus>().is_err());
}

#[test]
fn lifecycle_transitions() {
    use OrderStatus::*;
    let allowed = [(Pending, Paid), (Pending, Cancelled), (Paid, Shipped), (Paid, Cancelled)];
    for from in ALL {
        for to in ALL {
            assert_eq!(
                from.can_transition_to(to),
                allowed.contains(&(from, to)),
                "{from} -> {to}"
            );
        }
    }
}

#[test]
fn orders_carry_the_enum() {
    let mut order = Order::new(9, "eve", 10);
    assert_eq!(order.status, OrderStatus::Pending);
    billing::pay(&mut order).unwrap();
    assert_eq!(order.status, OrderStatus::Paid);
    order.cancel().unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);
    assert_eq!(
        order.cancel().unwrap_err(),
        "order 9 cannot be cancelled: it is cancelled"
    );
    let mut paid = Order::new(10, "fay", 20);
    billing::pay(&mut paid).unwrap();
    shipping::ship(&mut paid).unwrap();
    assert_eq!(
        billing::pay(&mut paid).unwrap_err(),
        "order 10 cannot be paid: it is shipped"
    );
}

#[test]
fn parsing_lines_uses_the_same_names() {
    let order = report::from_line(" 7,gus,300,shipped\n").unwrap();
    assert_eq!(order.status, OrderStatus::Shipped);
    assert!(report::from_line("7,gus,300,Shipped").is_err());
    assert!(report::from_line("7,gus,300").is_err());
    let counts: Vec<(String, usize)> = report::status_counts(&[order.clone(), order])
        .into_iter()
        .map(|(s, n)| (s.to_string(), n))
        .collect();
    assert_eq!(counts, [("shipped".to_owned(), 2)]);
}
