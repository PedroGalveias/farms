//! Macros for implementing common domain type patterns.
//!
//! Provides reusable macros for implementing sqlx traits on domain types,
//! reducing boilerplate for database serialization and deserialization.

/// Macro to implement common sqlx traits for a new type wrappers around String
///
/// This macro implements Type, Encode, and Decode for types that wrap a String.
/// This is used to convert between the service types and the database types.
/// It's useful for domain types like Name, Canton, Address, etc.
///
/// # Example
/// ```ignore
/// use crate::impl_sqlx_for_string_domain_type;
///
/// #[derive(Debug, Clone)]
/// pub struct StructName(String);
///
/// impl_sqlx_for_string_domain_type!(StructName);
/// ```
#[macro_export]
macro_rules! impl_sqlx_for_string_domain_type {
    ($type_name:ty) => {
        impl sqlx::Type<sqlx::Postgres> for $type_name {
            fn type_info() -> sqlx::postgres::PgTypeInfo {
                <String as sqlx::Type<sqlx::Postgres>>::type_info()
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::Postgres> for $type_name {
            fn decode(
                value: sqlx::postgres::PgValueRef<'r>,
            ) -> Result<Self, sqlx::error::BoxDynError> {
                let s = <String as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
                Ok(Self(s))
            }
        }

        impl<'q> sqlx::Encode<'q, sqlx::Postgres> for $type_name {
            fn encode_by_ref(
                &self,
                buf: &mut sqlx::postgres::PgArgumentBuffer,
            ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <String as sqlx::Encode<sqlx::Postgres>>::encode_by_ref(&self.0, buf)
            }
        }
    };
}

/// Macro to implement common sqlx traits for new type wrappers around Vec<String>
///
/// This macro implements Type, Encode, and Decode for types that wrap a Vec<String>.
/// This is used to convert between the service types and the database types.
/// It's useful for domain types like Categories.
///
/// # Example
/// ```ignore
/// use crate::impl_sqlx_for_vec_string_domain_type;
///
/// #[derive(Debug, Clone)]
/// pub struct Categories(Vec<String>);
///
/// impl_sqlx_for_vec_string_domain_type!(Categories);
/// ```
#[macro_export]
macro_rules! impl_sqlx_for_vec_string_domain_type {
    ($type_name:ty) => {
        impl sqlx::Type<sqlx::Postgres> for $type_name {
            fn type_info() -> sqlx::postgres::PgTypeInfo {
                <Vec<String> as sqlx::Type<sqlx::Postgres>>::type_info()
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::Postgres> for $type_name {
            fn decode(
                value: sqlx::postgres::PgValueRef<'r>,
            ) -> Result<Self, sqlx::error::BoxDynError> {
                let vec = <Vec<String> as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
                Ok(Self(vec))
            }
        }

        impl<'q> sqlx::Encode<'q, sqlx::Postgres> for $type_name {
            fn encode_by_ref(
                &self,
                buf: &mut sqlx::postgres::PgArgumentBuffer,
            ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <Vec<String> as sqlx::Encode<sqlx::Postgres>>::encode_by_ref(&self.0, buf)
            }
        }
    };
}
