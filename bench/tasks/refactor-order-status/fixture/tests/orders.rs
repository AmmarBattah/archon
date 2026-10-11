use orders::{billing, report, shipping, Order};

fn status(order: &Order) -> String {
    order.status.to_string()
}

#[test]
fn an_order_moves_through_its_lifecycle() {
    let mut order = Order::new(1, "ada", 1200);
    assert_eq!(status(&order), "pending");
    billing::pay(&mut order).unwrap();
    assert_eq!(status(&order), "paid");
    shipping::ship(&mut order).unwrap();
    assert_eq!(status(&order), "shipped");
    assert!(!order.is_open());
    assert!(order.cancel().is_err());
}

#[test]
fn illegal_steps_are_refused_with_the_current_status() {
    let mut order = Order::new(2, "bob", 500);
    let err = shipping::ship(&mut order).unwrap_err();
    assert_eq!(err, "order 2 cannot be shipped: it is pending");
    order.cancel().unwrap();
    let err = billing::pay(&mut order).unwrap_err();
    assert_eq!(err, "order 2 cannot be paid: it is cancelled");
}

#[test]
fn reports_count_and_round_trip() {
    let mut a = Order::new(1, "ada", 100);
    let b = Order::new(2, "bob", 250);
    let mut c = Order::new(3, "cy", 75);
    billing::pay(&mut a).unwrap();
    c.cancel().unwrap();
    let orders = vec![a.clone(), b, c];
    let counts: Vec<(String, usize)> = report::status_counts(&orders)
        .into_iter()
        .map(|(s, n)| (s.to_string(), n))
        .collect();
    assert_eq!(
        counts,
        [("cancelled".into(), 1), ("paid".into(), 1), ("pending".into(), 1)]
    );
    assert_eq!(billing::outstanding_cents(&orders), 250);
    assert_eq!(shipping::ready_to_ship(&orders), [1]);
    let line = report::to_line(&a);
    assert_eq!(line, "1,ada,100,paid");
    assert_eq!(report::from_line(&line).unwrap(), a);
    assert!(report::from_line("4,dee,1,lost").is_err());
}
