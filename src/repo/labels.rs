pub(crate) fn labels_match(
    labels: &[String],
    expected: &'static [&'static str],
) -> bool {
    expected
        .iter()
        .all(|l| labels.iter().any(|sl| sl == l))
}