mod common;

use archon_aether::fake::{self, Scripted};
use archon_aether::wire::CreateAgent;
use archon_core::ArchonError;
use common::{AGENT, TASK, connect, fake_aether};

#[tokio::test]
async fn a_root_request_is_never_silently_sent_to_an_older_daemon() {
    for supported in [false, true] {
        let (_dir, fake) = fake_aether().await;
        fake.set_features(if supported {
            vec!["repository_root".into()]
        } else {
            vec![]
        });
        fake.set_default(
            "create_agent",
            Scripted::reply("agent", fake::agent(AGENT, Some(TASK))),
        );
        let client = connect(&fake).await;
        assert_eq!(client.welcome().supports_repository_root(), supported);
        let created = client
            .create_agent(CreateAgent {
                role: "worker".into(),
                repository_root: Some("/checkout".into()),
                ..Default::default()
            })
            .await;
        if supported {
            assert_eq!(created.unwrap().id.as_str(), AGENT);
            assert_eq!(
                fake.received_named("create_agent")[0]["repository_root"],
                "/checkout"
            );
        } else {
            assert!(matches!(created, Err(ArchonError::Unavailable(_))));
            assert!(fake.received_named("create_agent").is_empty());
        }
    }
}
