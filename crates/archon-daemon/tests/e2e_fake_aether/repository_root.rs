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
async fn a_refused_root_node_fails_without_advancing_its_generation() {
    for code in ["invalid_argument", "not_found"] {
        let dir = tempfile::tempdir().unwrap();
        let aether = start_aether(dir.path()).await;
        aether.set_features(vec!["repository_root".into()]);
        script_fleet(&aether);
        aether.script(
            "create_agent",
            Scripted::refuse(code, "registered checkout is unreadable"),
        );
        let h = harness(&aether).await;
        let item = h.enqueue("refused frozen checkout").await;
        h.dispatch().await;
        let QueueState::Failed { error, .. } = h.state(&item.id).await else {
            panic!("a refused root must fail the item");
        };
        assert!(error.contains(&h.repo.display().to_string()), "{error}");
        assert!(
            error.contains("registered checkout is unreadable"),
            "{error}"
        );
        assert!(
            error.contains("re-register the repo or cancel the run"),
            "{error}"
        );
        let creates = aether.received_named("create_agent");
        assert_eq!(creates.len(), 2, "creation and retirement recovery replay");
        let run = h.run(&item.id).await;
        for create in &creates {
            assert_identity(create, Some(&h.repo));
            assert_eq!(create["idempotency_key"], format!("{}:node:1", run.id));
        }
        assert_eq!(
            without_request_id(creates[0].clone()),
            without_request_id(creates[1].clone())
        );
    }
}

#[tokio::test]
async fn a_moved_frozen_checkout_fails_before_replaying_the_node() {
    let dir = tempfile::tempdir().unwrap();
    let aether = start_aether(dir.path()).await;
    aether.set_features(vec!["repository_root".into()]);
    let fleet = script_fleet(&aether);
    let h = harness(&aether).await;
    reconnect(&h, &aether).await;
    fleet.lock().unwrap().lose_next_reply = true;
    let item = h.enqueue("checkout moved after identity froze").await;
    h.dispatch().await;
    assert_eq!(h.stages(&item.id).await[0].state, StageState::Spawning);
    std::fs::rename(&h.repo, h.repo.with_file_name("moved-checkout")).unwrap();
    h.dispatch().await;
    let QueueState::Failed { error, .. } = h.state(&item.id).await else {
        panic!("a moved root must fail the item");
    };
    assert!(error.contains(&h.repo.display().to_string()), "{error}");
    assert!(error.contains("moved or was removed"), "{error}");
    assert!(
        error.contains("re-register the repo or cancel the run"),
        "{error}"
    );
    let creates = aether.received_named("create_agent");
    assert_eq!(creates.len(), 1, "the missing root was not sent again");
    let run = h.run(&item.id).await;
    let raw = h.kv(&format!("run_node:{}", run.id)).await.unwrap();
    let stored: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(stored["generation"], 1);
    assert_eq!(stored["repository_identity"]["Root"], json!(h.repo));
}

#[tokio::test]
async fn replacing_a_dead_generation_two_node_keeps_its_root_after_feature_loss() {
    let dir = tempfile::tempdir().unwrap();
    let aether = start_aether(dir.path()).await;
    aether.set_features(vec!["repository_root".into()]);
    let fleet = script_fleet(&aether);
    let h = harness(&aether).await;
    let item = h.enqueue("keep root after generation two dies").await;
    h.dispatch().await;
    complete_running(&h, &fleet, |_| json!({"summary": "implemented"})).await;
    aether.script("get_agent", Scripted::refuse("not_found", "node one lost"));
    h.dispatch().await;
    let run = h.run(&item.id).await;
    let key = format!("run_node:{}", run.id);
    let raw = h.kv(&key).await.unwrap();
    let second: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(second["generation"], 2);
    assert_eq!(second["agent"], node_id(2));
    assert_eq!(h.running().await.len(), 2);

    aether.set_features(vec![]);
    reconnect(&h, &aether).await;
    let pass =
        json!({"summary": "VERDICT: pass", "structured": {"verdict": "pass", "findings": []}});
    complete_running(&h, &fleet, |_| pass.clone()).await;
    aether.script("get_agent", Scripted::refuse("not_found", "node two lost"));
    let before = aether.received_named("create_agent");
    h.dispatch().await;
    let raw = h.kv(&key).await.unwrap();
    let pending: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(pending["generation"], 3);
    assert_eq!(
        pending["repository_identity"],
        second["repository_identity"]
    );
    assert_eq!(pending["request"]["repository_root"], json!(h.repo));
    assert!(pending["request"]["repository"].is_null());
    assert!(
        h.stages(&item.id)
            .await
            .iter()
            .any(|s| s.state == StageState::Spawning)
    );
    h.dispatch().await;
    assert_eq!(
        h.kv(&key).await.unwrap(),
        raw,
        "retries keep the frozen generation"
    );
    assert_eq!(aether.received_named("create_agent"), before);
}
