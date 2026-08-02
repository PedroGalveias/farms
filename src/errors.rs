pub fn error_chain_fmt(
    e: &impl std::error::Error,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    writeln!(f, "{}\n", e)?;
    let mut current = e.source();
    while let Some(cause) = current {
        writeln!(f, "Caused by:\n\t{}", cause)?;
        current = cause.source();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(thiserror::Error)]
    enum TestError {
        #[error("top-level failure")]
        Wrapper(#[source] anyhow::Error),
    }

    impl std::fmt::Debug for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            error_chain_fmt(self, f)
        }
    }

    #[test]
    fn debug_prints_the_full_source_chain() {
        let root = anyhow::anyhow!("root cause").context("middle cause");
        let err = TestError::Wrapper(root);

        let output = format!("{err:?}");

        assert!(output.starts_with("top-level failure\n"));
        assert!(output.contains("Caused by:\n\tmiddle cause"));
        assert!(output.contains("Caused by:\n\troot cause"));
    }

    #[test]
    fn debug_stops_at_the_last_source() {
        let err = TestError::Wrapper(anyhow::anyhow!("lone cause"));

        let output = format!("{err:?}");

        assert_eq!(output, "top-level failure\n\nCaused by:\n\tlone cause\n");
    }
}
