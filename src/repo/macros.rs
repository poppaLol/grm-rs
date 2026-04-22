// src/macros.rs
#[macro_export]
macro_rules! autocommit {
    ($backend:expr, |$tx:ident| $body:expr) => {{
        // 1) Begin backend tx (raw)
        let __inner = $backend.begin_tx().await?;

        // 2) Wrap it in the public Transaction wrapper
        let mut $tx = $crate::client::Transaction::from_inner(__inner);

        // 3) Run body (must evaluate to Result<T>)
        let __out = $body;

        // 4) Commit or rollback once
        match __out {
            Ok(__v) => {
                $tx.commit().await?;
                Ok(__v)
            }
            Err(__e) => {
                let _ = $tx.rollback().await;
                Err(__e)
            }
        }
    }};
}
#[macro_export]
macro_rules! autoread {
    ($backend:expr, |$tx:ident| $body:expr) => {{
        let inner = $backend.begin_tx().await?;
        let mut $tx = Transaction::from_inner(inner);
        let result = { $body };
        result
    }};
}