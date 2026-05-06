/* This was an exercise in matching the codebase functionality to a user test case
 *
 * A Paragraph may be linked to a Metric by a CONTAINS relationship. POC Code here
 * shows how the library can be used to acheive this.
 */
use grm_rs::{
    GraphClient, InMemoryBackend, KernelValue, NodeModel, NodePattern, Query, RelModel, Result,
    ReturnKind,
    decode::{ResultShape, node, rel},
    dsl::NodeValue,
    typed_id,
};
use serde::{Deserialize, Serialize};

typed_id!(MetricId);
typed_id!(ParagraphId);
typed_id!(ContainsId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct Metric {
    #[grm(id)]
    #[serde(skip)]
    pub(crate) id: MetricId,
    pub name: String,
    pub value: i32,
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct Paragraph {
    #[grm(id)]
    #[serde(skip)]
    pub id: ParagraphId,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RelModel)]
#[grm(from = "Paragraph", to = "Metric", ty = "CONTAINS")]
pub struct Contains {
    #[grm(id)]
    #[serde(skip)]
    pub id: ContainsId,
    #[grm(skip)]
    from: ParagraphId,
    #[grm(skip)]
    to: MetricId,
}

pub fn nodevalue_labels_match<M: NodeModel>(n: &NodeValue) -> bool {
    M::LABELS.iter().all(|l| n.labels.iter().any(|nl| nl == l))
}

#[tokio::test]
async fn test_document_schema_construction() -> Result<()> {
    let backend = InMemoryBackend::new();
    let client = GraphClient::new(backend);
    let mut tx = client.transaction().await?;

    let (para_id, para2_id, metric_id) = {
        let mut repo = tx.repo();

        let mut para = Paragraph {
            id: ParagraphId::default(),
            content: "I am a paragraph about metric number 1 which is the miles per hour I type"
                .into(),
        };

        let mut para2 = Paragraph {
            id: ParagraphId::default(),
            content:
                "I am a second paragraph about metric number 1 which is the miles per hour I type"
                    .into(),
        };

        let mut metric = Metric {
            end_date: "Tomorrow".into(),
            start_date: "Yesterday".into(),
            id: MetricId::default(),
            name: "Laurie Types At this speed".into(),
            value: 1,
        };

        repo.nodes::<Paragraph>().create(&mut para).await?;
        repo.nodes::<Paragraph>().create(&mut para2).await?;
        repo.nodes::<Metric>().create(&mut metric).await?;

        let mut cont = Contains {
            id: ContainsId::default(),
            from: ParagraphId::default(),
            to: MetricId::default(),
        };
        let mut cont2 = Contains {
            id: ContainsId::default(),
            from: ParagraphId::default(),
            to: MetricId::default(),
        };

        repo.rels::<Contains>()
            .create_between(&para.id, &metric.id, &mut cont)
            .await?;
        repo.rels::<Contains>()
            .create_between(&para2.id, &metric.id, &mut cont2)
            .await?;

        (para.id, para2.id, metric.id)
    };

    assert_ne!(para_id, ParagraphId::default());
    assert_ne!(para2_id, ParagraphId::default());
    assert_ne!(metric_id, MetricId::default());

    // Query: (Paragraph)-[Contains]->(Metric) returning Paragraph by default
    let q: Query<_> = Query::<Paragraph>::matching(
        NodePattern::<Paragraph>::new()
            .out::<Contains>()
            .to::<Metric>(),
    );

    let exec = tx.execute(q).await?;

    //test result shaping temp:
    let row = exec.qr.rows.first().unwrap();

    // Find the var ids by inspecting the row
    let mut paragraph_var = None;
    let mut contains_var = None;
    let mut metric_var = None;

    for v in row.keys().copied() {
        match row.get(&v).unwrap() {
            KernelValue::Node(n) => {
                if nodevalue_labels_match::<Paragraph>(n) {
                    paragraph_var = Some(v);
                } else if nodevalue_labels_match::<Metric>(n) {
                    metric_var = Some(v);
                }
            }
            KernelValue::Rel(_) => {
                contains_var = Some(v);
            }
            KernelValue::Scalar(_) => {
                // Scalar values don't need special handling
            }
        }
    }

    let paragraph_var = paragraph_var.expect("Paragraph var not found in row");
    let contains_var = contains_var.expect("Contains var not found in row");
    let metric_var = metric_var.expect("Metric var not found in row");

    // Now decode the shaped tuple
    let shape = (
        node::<Paragraph>(paragraph_var),
        rel::<Contains>(contains_var),
        node::<Metric>(metric_var),
    );

    // _p is a Paragraph, _c is a Contains, _m is a Metric
    let (_p, _c, _m): (Paragraph, Contains, Metric) = shape.decode(&exec.gq, row)?;

    // Validate query invariants (good for catching compiler issues)
    exec.gq.validate()?;

    let bound = exec.gq.bound_vars();
    let ret_var = exec.gq.return_var();
    let ret_kind = exec.gq.return_kind();

    assert!(!exec.qr.rows.is_empty());

    // Assert: every row contains all bound vars
    for row in &exec.qr.rows {
        for v in &bound {
            assert!(
                row.values.contains_key(v),
                "row missing bound var {:?}; keys={:?}",
                v,
                row.values.keys().collect::<Vec<_>>()
            );
        }

        // Assert: return var has correct kind
        match ret_kind {
            ReturnKind::Node => assert!(matches!(
                row.values.get(&ret_var),
                Some(KernelValue::Node(_))
            )),
            ReturnKind::Rel => assert!(matches!(
                row.values.get(&ret_var),
                Some(KernelValue::Rel(_))
            )),
        }
    }

    // ---- Show the results (debug-style) ----
    // Extract returned Paragraph kernel ids (i64) and print a readable summary.
    let mut returned_paragraph_kernel_ids: Vec<i64> = Vec::new();

    for (i, row) in exec.qr.rows.iter().enumerate() {
        eprintln!("--- row {i} ---");

        // Sort by VarId for stable output (VarId likely implements Ord; if not, remove sort)
        let mut items: Vec<_> = row.values.iter().collect();
        items.sort_by_key(|(k, _)| *k);

        for (var, kv) in items {
            match kv {
                KernelValue::Node(n) => {
                    eprintln!(
                        "  {var:?} => Node(id={}, labels={:?}, props_keys={:?})",
                        n.id,
                        n.labels,
                        n.props.keys().collect::<Vec<_>>()
                    );

                    // If this node is the return var, collect it
                    if *var == ret_var {
                        returned_paragraph_kernel_ids.push(n.id);
                    }
                }
                KernelValue::Rel(r) => {
                    eprintln!(
                        "  {var:?} => Rel(id={}, type={}, from={}, to={}, props_keys={:?})",
                        r.id,
                        r.ty,
                        r.from,
                        r.to,
                        r.props.keys().collect::<Vec<_>>()
                    );
                }
                KernelValue::Scalar(_) => {
                    eprintln!("  {var:?} => Scalar(_)");
                }
            }
        }
    }

    // ---- Assert we got our paragraph back ----
    //
    // ParagraphId is likely a typed wrapper around i64 and implements Into<i64>.
    // If not, replace with whatever "extract raw i64" you have.
    let para_raw: i64 = para_id.into();
    let para2_raw: i64 = para2_id.into();

    assert!(
        returned_paragraph_kernel_ids.contains(&para_raw)
            || returned_paragraph_kernel_ids.contains(&para2_raw),
        "expected returned paragraphs to include created ones; got={returned_paragraph_kernel_ids:?}, expected one of {para_raw:?} or {para2_raw:?}"
    );

    tx.commit().await?;

    Ok(())
}
