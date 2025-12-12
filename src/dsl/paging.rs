
pub fn apply_paging<T>(mut items: Vec<T>, offset: Option<usize>, limit: Option<usize>) -> Vec<T> {
    let start = offset.unwrap_or(0);
    if start >= items.len() {
        return Vec::new();
    }

    let end = if let Some(limit) = limit {
        start.saturating_add(limit).min(items.len())
    } else {
        items.len()
    };

    items.drain(..start);
    items.truncate(end - start);
    items
}