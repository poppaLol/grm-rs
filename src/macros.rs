#[macro_export]
macro_rules! typed_id {
    ($name:ident) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            ::serde::Serialize,
            ::serde::Deserialize,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Default
        )]
        pub struct $name(pub i64);

        impl ::core::convert::From<i64> for $name {
            fn from(v: i64) -> Self {
                Self(v)
            }
        }

        impl ::core::convert::From<$name> for i64 {
            fn from(v: $name) -> Self {
                v.0
            }
        }
    };
}