use super::*;

fn assert_identity(create: &Value, root: Option<&std::path::Path>) {
    assert!(create["scope"].is_object());
    assert!(create["scope"].get("repository_root").is_none());
    match root {
        Some(root) => {
            assert_eq!(create["repository_root"], json!(root));
            assert!(create["scope"].get("repository").is_none());
            assert_ne!(create["scope"]["workspace_root"], json!(root));
        }
        None => {
            assert!(create.get("repository_root").is_none());
            assert_eq!(create["scope"]["repository"], common::REPO_NAME);
        }
    }
}

async fn reconnect(h: &Harness, aether: &FakeAether) {
    let client = AetherClient::connect_with(
        &aether.endpoint(),
        ClientOptions {
            request_timeout: Duration::from_millis(100),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(client.welcome().protocol_version, 17);
    assert!(!client.welcome().instance_id.is_empty());
    h.ctx.aether.replace(Some(Arc::new(client))).unwrap();
}

async fn restart(h: &mut Harness, aether: &FakeAether) {
    h.restart();
    let ctx = Arc::get_mut(&mut h.ctx).unwrap();
    ctx.backend = Arc::new(AetherBackend::new(
        ctx.aether.clone(),
        ctx.config.models.policy(),
    ));
    reconnect(h, aether).await;
    assert!(h.ctx.runs.lock().await.drivers.is_empty());
    executor::recover(&h.ctx).await.unwrap();
    assert!(!h.ctx.runs.lock().await.drivers.is_empty());
}

fn without_request_id(mut frame: Value) -> Value {
    frame.as_object_mut().unwrap().remove("req").unwrap();
    frame
}

#[tokio::test]
async fn registered_checkout_names_nodes_and_stages_only_when_advertised() {
    for supported in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let aether = start_aether(dir.path()).await;
        aether.set_features(if supported {
            vec!["repository_root".into()]
        } else {
            vec![]
        });
        script_fleet(&aether);
        let h = harness(&aether).await;
        let item = h.enqueue("share checkout memory").await;
        h.dispatch().await;
        let creates = aether.received_named("create_agent");
        assert_eq!(creates.len(), 2);
        for create in &creates {
            assert_identity(create, supported.then_some(h.repo.as_path()));
        }
        assert_eq!(h.stages(&item.id).await[0].state, StageState::Running);
    }
}

#[tokio::test]
async fn lost_node_and_stage_replies_replay_the_frozen_identity_after_restart() {
    for (supported, stage_pending) in [(false, false), (false, true), (true, false), (true, true)] {
        let dir = tempfile::tempdir().unwrap();
        let aether = start_aether(dir.path()).await;
        aether.set_features(if supported {
            vec!["repository_root".into()]
        } else {
            vec![]
        });
        let fleet = script_fleet(&aether);
        let mut h = harness(&aether).await;
        reconnect(&h, &aether).await;
        if stage_pending {
            aether.script(
                "create_agent",
                Scripted::reply("agent", fake::agent(&node_id(1), None)),
            );
        }
        fleet.lock().unwrap().lose_next_reply = true;
        let item = h.enqueue("replay repository identity").await;
        h.dispatch().await;
        assert_eq!(h.stages(&item.id).await[0].state, StageState::Spawning);
        let before = aether.received_named("create_agent");
        aether.set_features(vec!["repository_root".into()]);
        restart(&mut h, &aether).await;
        h.dispatch().await;
        let after = aether.received_named("create_agent");
        assert_eq!(
            without_request_id(before.last().unwrap().clone()),
            without_request_id(after[before.len()].clone())
        );
        for create in &after {
            assert_identity(create, supported.then_some(h.repo.as_path()));
        }
        assert_eq!(h.stages(&item.id).await[0].state, StageState::Running);
    }
}

#[tokio::test]
async fn stages_keep_the_nodes_name_after_aether_gains_the_feature() {
    let dir = tempfile::tempdir().unwrap();
    let aether = start_aether(dir.path()).await;
    let fleet = script_fleet(&aether);
    let h = harness(&aether).await;
    let item = h.enqueue("keep the original memory scope").await;
    h.dispatch().await;
    aether.set_features(vec!["repository_root".into()]);
    reconnect(&h, &aether).await;
    complete_running(&h, &fleet, |_| json!({"summary": "implemented"})).await;
    h.dispatch().await;
    let creates = aether.received_named("create_agent");
    assert_eq!(creates.len(), 4);
    for create in &creates {
        assert_identity(create, None);
    }
    assert_children_of(&creates[1..], &node_id(1));
    assert_eq!(h.running().await.len(), 2);
    assert_eq!(
        h.run(&item.id).await.aether_run.unwrap().as_str(),
        "run_0001"
    );
}

#[tokio::test]
async fn a_refused_root_node_keeps_its_identity_in_the_next_generation() {
    let dir = tempfile::tempdir().unwrap();
    let aether = start_aether(dir.path()).await;
    aether.set_features(vec!["repository_root".into()]);
    script_fleet(&aether);
    aether.script(
        "create_agent",
        Scripted::refuse("invalid_argument", "old key"),
    );
    let h = harness(&aether).await;
    let item = h.enqueue("replace node without changing identity").await;
    h.dispatch().await;
    let creates = aether.received_named("create_agent");
    assert_eq!(creates.len(), 3);
    for create in &creates {
        assert_identity(create, Some(&h.repo));
    }
    let run = h.run(&item.id).await;
    assert_eq!(creates[1]["idempotency_key"], format!("{}:node:2", run.id));
    assert_eq!(h.stages(&item.id).await[0].state, StageState::Running);
}
