use grm_rs::{GraphClient, InMemoryBackend, NodeModel, RelModel, Result, typed_id, Query, NodePattern};
use serde::{Deserialize, Serialize};

// Models
typed_id!(UserId);
typed_id!(PostId);
typed_id!(AuthoredId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
struct User {
    #[grm(id)]
    #[serde(skip)]
    pub(crate) id: UserId,
    pub name: String,
    pub age: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
struct Post {
    #[grm(id)]
    #[serde(skip)]
    pub(crate) id: PostId,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RelModel)]
#[grm(from = "User", to = "Post", ty = "AUTHORED")]
struct Authored {
    #[grm(id)]
    #[serde(skip)]
    pub(crate) id: AuthoredId,
    pub year: u64,
    
    #[grm(skip)]
    pub(crate) from: UserId,

    #[grm(skip)]
    pub(crate) to: PostId,
}

#[tokio::main]
async fn main() -> Result<()> {
    let json_file = "graph.json";

    let backend = InMemoryBackend::new();

    // Create data
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

    let mut post1 = Post {
        id: PostId::from(101),
        title: "Hello World".to_string(),
    };

    let mut post2 = Post {
        id: PostId::from(102),
        title: "Graph Persistence".to_string(),
    };

    let mut authored1 = Authored {
        id: AuthoredId::from(1),
        year: 2024,
        from: UserId::default(),
        to: PostId::default()
    };

    let mut authored2 = Authored {
        id: AuthoredId::from(2),
        year: 2024,
        from: UserId::default(),
        to: PostId::default()
    };

    // Persist to JSON
    println!("Creating graph and persisting to JSON...");
    let client = GraphClient::new(backend);
    let mut tx = client.transaction().await?;

    let mut repo = tx.repo();
    repo.nodes::<User>().create(&mut user1).await?;
    repo.nodes::<User>().create(&mut user2).await?;
    repo.nodes::<Post>().create(&mut post1).await?;
    repo.nodes::<Post>().create(&mut post2).await?;
    repo.rels::<Authored>().create_between(user1.id(), post1.id(), &mut authored1).await?;
    repo.rels::<Authored>().create_between(user2.id(), post2.id(), &mut authored2).await?;

    tx.commit().await.expect("commit failed");

    // Persist to JSON
    println!("  -> Writing to {}...", json_file);
    client.persistence().expect("Backend does not support persistence").save_to_file(json_file)?;

    println!("✓ Graph persisted to JSON\n");

    // Load from JSON
    println!("Loading graph from JSON...");
    {
        let client = GraphClient::new(
            InMemoryBackend::load_from_file(json_file)?
        );
        
        // Verify data
        println!("  -> Loaded successfully");
        println!("\nUsers in graph:");
        let mut tx = client.transaction().await?;
        let users = tx.query::<User, User>(Query::matching(NodePattern::new())).await?;
        for user in users {
            println!("    - {}: {} (age: {})", user.id.0, user.name, user.age);
        }
        drop(tx);

        println!("\nPosts in graph:");
        let mut tx = client.transaction().await?;
        let posts = tx.query::<Post, Post>(Query::matching(NodePattern::new())).await?;
        for post in posts {
            println!("    - {}: {}", post.id.0, post.title);
        }
        drop(tx);

        println!("\nRelationships:");
        let mut tx = client.transaction().await?;
        let rels = tx.query_rel::<User, Authored>(Query::matching(
            NodePattern::<User>::new().out::<Authored>().to::<Post>()).return_rel())
            .await?;
        for rel in rels {
            println!("{:?} Links - User {}: to Post {}", rel.id, rel.from.0, rel.to.0);
        }
        drop(tx);
    }

    println!("\n✓ Graph loaded and verified!");

    Ok(())
}
