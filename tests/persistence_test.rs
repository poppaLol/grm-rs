mod common;

use common::{Authored, AuthoredId, Post, PostId, User, UserId};
use grm_rs::{GraphClient, InMemoryBackend, NodeModel, NodePattern, Query};

fn cleanup_test_file(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.bak"));
}

#[tokio::test]
async fn test_persistence() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing InMemoryBackend persistence...");

    std::fs::create_dir_all("test-dbs")?;
    let json_file = "test-dbs/test_graph.json";
    let bin_file = "test-dbs/test_graph.bin";
    cleanup_test_file(json_file);
    cleanup_test_file(bin_file);

    // Create a backend and save to file
    let backend = InMemoryBackend::new();
    let client = GraphClient::new(backend);

    println!("  -> Saving to {}...", json_file);
    client
        .persistence()
        .expect("Backend does not support persistence")
        .save_to_file(json_file)?;

    println!("  -> Saved successfully");

    println!("  -> Loading from {}...", json_file);
    let _loaded_client = GraphClient::new(InMemoryBackend::load_from_file(json_file)?);

    println!("  -> Saving binary to {}...", bin_file);
    client
        .persistence()
        .expect("Backend does not support persistence")
        .save_to_binary_file(bin_file)?;

    println!("  -> Loading binary from {}...", bin_file);
    let _loaded_binary_client = GraphClient::new(InMemoryBackend::load_from_binary_file(bin_file)?);

    println!("✓ InMemoryBackend persistence test passed!");

    // Clean up
    cleanup_test_file(json_file);
    cleanup_test_file(bin_file);
    println!("\n✓ Test file removed");

    Ok(())
}

#[tokio::test]
async fn test_persistence_with_typed_models() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing persistence with typed models...");

    std::fs::create_dir_all("test-dbs")?;
    let json_file = "test-dbs/test_graph_typed.json";
    let bin_file = "test-dbs/test_graph_typed.bin";
    cleanup_test_file(json_file);
    cleanup_test_file(bin_file);

    // Create data using typed models
    let backend = InMemoryBackend::new();
    let client = GraphClient::new(backend);

    // Create users
    let mut user1 = User {
        id: UserId::from(1),
        name: "Alice".to_string(),
        age: 30,
    };

    let mut user2 = User {
        id: UserId::from(2),
        name: "Bob".to_string(),
        age: 25,
    };

    // Create posts
    let mut post1 = Post {
        id: PostId::from(101),
        title: "Hello World".to_string(),
    };

    let mut post2 = Post {
        id: PostId::from(102),
        title: "Graph Persistence".to_string(),
    };

    // Create relationships
    let mut authored1 = Authored {
        id: AuthoredId::from(1),
        year: 2024,
        from: UserId::default(),
        to: PostId::default(),
    };

    let mut authored2 = Authored {
        id: AuthoredId::from(2),
        year: 2024,
        from: UserId::default(),
        to: PostId::default(),
    };

    // Persist to JSON
    println!("  -> Persisting to {}...", json_file);
    {
        let mut tx = client.transaction().await?;
        let mut repo = tx.repo();

        repo.nodes::<User>().create(&mut user1).await?;
        repo.nodes::<User>().create(&mut user2).await?;
        repo.nodes::<Post>().create(&mut post1).await?;
        repo.nodes::<Post>().create(&mut post2).await?;
        repo.rels::<Authored>()
            .create_between(user1.id(), post1.id(), &mut authored1)
            .await?;
        repo.rels::<Authored>()
            .create_between(user2.id(), post2.id(), &mut authored2)
            .await?;

        tx.commit().await?;
    }

    // Save using persistence accessor
    client
        .persistence()
        .expect("Backend does not support persistence")
        .save_to_file(json_file)?;

    println!("  -> Saved successfully");

    // Load from JSON
    println!("  -> Loading from {}...", json_file);
    let _loaded_client = GraphClient::new(InMemoryBackend::load_from_file(json_file)?);

    // Verify data
    println!("  -> Verifying users...");
    let mut tx = _loaded_client.transaction().await?;
    let users = tx
        .query::<User, User>(Query::matching(NodePattern::new()))
        .await?;
    drop(tx);

    assert_eq!(users.len(), 2, "Should have 2 users");
    assert!(users.iter().any(|u| u.name == "Alice"), "Should have Alice");
    assert!(users.iter().any(|u| u.name == "Bob"), "Should have Bob");

    println!("  -> Verifying posts...");
    let mut tx = _loaded_client.transaction().await?;
    let posts = tx
        .query::<Post, Post>(Query::matching(NodePattern::new()))
        .await?;
    drop(tx);

    assert_eq!(posts.len(), 2, "Should have 2 posts");
    assert!(
        posts.iter().any(|p| p.title == "Hello World"),
        "Should have Hello World post"
    );
    assert!(
        posts.iter().any(|p| p.title == "Graph Persistence"),
        "Should have Graph Persistence post"
    );

    println!("  -> Verifying relationships (skipped - Authored is a RelModel, not NodeModel)");

    println!("  -> Saving binary to {}...", bin_file);
    client
        .persistence()
        .expect("Backend does not support persistence")
        .save_to_binary_file(bin_file)?;

    println!("  -> Loading binary from {}...", bin_file);
    let _loaded_binary_client = GraphClient::new(InMemoryBackend::load_from_binary_file(bin_file)?);

    println!("  -> Verifying users from binary load...");
    let mut tx = _loaded_binary_client.transaction().await?;
    let users = tx
        .query::<User, User>(Query::matching(NodePattern::new()))
        .await?;
    drop(tx);

    assert_eq!(users.len(), 2, "Binary load should have 2 users");

    println!("  -> Verifying posts from binary load...");
    let mut tx = _loaded_binary_client.transaction().await?;
    let posts = tx
        .query::<Post, Post>(Query::matching(NodePattern::new()))
        .await?;
    drop(tx);

    assert_eq!(posts.len(), 2, "Binary load should have 2 posts");

    println!("✓ All typed model persistence tests passed!");
    println!("\nData verified successfully");

    // Clean up
    cleanup_test_file(json_file);
    cleanup_test_file(bin_file);
    println!("\n✓ Test file removed");

    Ok(())
}
