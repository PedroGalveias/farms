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

#[cfg(test)]
mod tests {
    use sqlx::{Encode, Type, encode::IsNull, postgres::PgArgumentBuffer};

    // `Decode`'s `PgValueRef` can only be constructed from real wire bytes
    // produced by a live connection, so the round trip through Postgres is
    // exercised by the domain types' own DB-backed integration tests
    // (e.g. `tests/api/farms.rs` inserting/reading back `Address`, `Canton`,
    // `Categories`). What's unit-testable here, without a database, is that
    // `type_info` and `Encode` for the generated wrapper agree with the
    // wrapped type's - which is the whole point of these macros.

    #[derive(Debug, Clone)]
    struct StringWrapper(String);
    impl_sqlx_for_string_domain_type!(StringWrapper);

    #[derive(Debug, Clone)]
    struct VecStringWrapper(Vec<String>);
    impl_sqlx_for_vec_string_domain_type!(VecStringWrapper);

    #[test]
    fn string_wrapper_type_info_matches_string() {
        assert_eq!(
            <StringWrapper as Type<sqlx::Postgres>>::type_info(),
            <String as Type<sqlx::Postgres>>::type_info()
        );
    }

    #[test]
    fn string_wrapper_encodes_identically_to_the_wrapped_string() {
        let value = "hello world".to_string();
        let wrapper = StringWrapper(value.clone());

        let mut wrapper_buf = PgArgumentBuffer::default();
        let wrapper_result = wrapper.encode_by_ref(&mut wrapper_buf).unwrap();

        let mut string_buf = PgArgumentBuffer::default();
        let string_result = value.encode_by_ref(&mut string_buf).unwrap();

        assert!(matches!(wrapper_result, IsNull::No));
        assert!(matches!(string_result, IsNull::No));
        assert_eq!(&*wrapper_buf, &*string_buf);
    }

    #[test]
    fn vec_string_wrapper_type_info_matches_vec_string() {
        assert_eq!(
            <VecStringWrapper as Type<sqlx::Postgres>>::type_info(),
            <Vec<String> as Type<sqlx::Postgres>>::type_info()
        );
    }

    #[test]
    fn vec_string_wrapper_encodes_identically_to_the_wrapped_vec() {
        let value = vec!["Dairy".to_string(), "Egg".to_string()];
        let wrapper = VecStringWrapper(value.clone());

        let mut wrapper_buf = PgArgumentBuffer::default();
        let wrapper_result = wrapper.encode_by_ref(&mut wrapper_buf).unwrap();

        let mut vec_buf = PgArgumentBuffer::default();
        let vec_result = value.encode_by_ref(&mut vec_buf).unwrap();

        assert!(matches!(wrapper_result, IsNull::No));
        assert!(matches!(vec_result, IsNull::No));
        assert_eq!(&*wrapper_buf, &*vec_buf);
    }

    #[test]
    fn vec_string_wrapper_encodes_an_empty_vec() {
        let wrapper = VecStringWrapper(Vec::new());
        let mut buf = PgArgumentBuffer::default();

        let result = wrapper.encode_by_ref(&mut buf).unwrap();

        assert!(matches!(result, IsNull::No));
    }
}
