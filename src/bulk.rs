//! Snapshot listings before acting, and retain progress when a batch stops.
use color_eyre::eyre::Report;
use futures::{Stream, TryStreamExt};
use object_store::{ObjectMeta, path::Path};
use std::{fmt, future::Future};

#[derive(Debug)]
pub(crate) enum BatchError {
    Listing(object_store::Error),
    Operation {
        completed: usize,
        total: usize,
        path: Path,
        cause: Report,
    },
}

impl fmt::Display for BatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Listing(cause) => {
                write!(f, "Folder listing failed; no objects processed: {cause}")
            }
            Self::Operation {
                completed,
                total,
                path,
                cause,
            } => {
                let status = if *completed == 0 {
                    "Failed"
                } else {
                    "Partially completed"
                };
                write!(
                    f,
                    "{status}: {completed}/{total} objects completed; stopped at {path}: {cause}"
                )
            }
        }
    }
}

impl std::error::Error for BatchError {}

pub(crate) async fn snapshot(
    listing: impl Stream<Item = object_store::Result<ObjectMeta>>,
) -> Result<Vec<ObjectMeta>, BatchError> {
    listing.try_collect().await.map_err(BatchError::Listing)
}

pub(crate) async fn run<F, Fut>(
    listing: impl Stream<Item = object_store::Result<ObjectMeta>>,
    mut operation: F,
) -> Result<usize, BatchError>
where
    F: FnMut(ObjectMeta) -> Fut,
    Fut: Future<Output = color_eyre::Result<()>>,
{
    // Complete the listing before mutations, especially for overlapping clones.
    let objects = snapshot(listing).await?;
    let total = objects.len();
    for (completed, meta) in objects.into_iter().enumerate() {
        let path = meta.location.clone();
        operation(meta)
            .await
            .map_err(|cause| BatchError::Operation {
                completed,
                total,
                path,
                cause,
            })?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use std::cell::Cell;

    fn meta(name: &str) -> ObjectMeta {
        ObjectMeta {
            location: Path::from(name),
            last_modified: chrono::Utc::now(),
            size: 1,
            e_tag: None,
            version: None,
        }
    }

    #[tokio::test]
    async fn listing_failure_prevents_all_operations() {
        let calls = Cell::new(0);
        let listing = stream::iter(vec![
            Ok(meta("first")),
            Err(object_store::Error::Generic {
                store: "test",
                source: "injected listing failure".into(),
            }),
        ]);
        let error = run(listing, |_| {
            calls.set(calls.get() + 1);
            async { Ok(()) }
        })
        .await
        .unwrap_err();
        assert!(matches!(error, BatchError::Listing(_)));
        assert!(error.to_string().contains("no objects processed"));
        assert_eq!(calls.get(), 0);
    }

    #[tokio::test]
    async fn operation_failure_reports_progress_and_stops() {
        for fail_at in 0..3 {
            let calls = Cell::new(0);
            let listing = stream::iter(["first", "second", "third"].map(|name| Ok(meta(name))));
            let error = run(listing, |_| {
                let index = calls.get();
                calls.set(index + 1);
                async move {
                    if index == fail_at {
                        color_eyre::eyre::bail!("injected operation failure");
                    }
                    Ok(())
                }
            })
            .await
            .unwrap_err();
            assert!(
                matches!(&error, BatchError::Operation { completed, total: 3, .. } if *completed == fail_at)
            );
            assert_eq!(calls.get(), fail_at + 1);
            assert!(error.to_string().contains(if fail_at == 0 {
                "Failed:"
            } else {
                "Partially completed:"
            }));
            assert!(error.to_string().contains("injected operation failure"));
        }
    }

    #[tokio::test]
    async fn complete_and_empty_batches_succeed() {
        for count in [0, 3] {
            let calls = Cell::new(0);
            let listing = stream::iter((0..count).map(|_| Ok(meta("file"))));
            assert_eq!(
                run(listing, |_| {
                    calls.set(calls.get() + 1);
                    async { Ok(()) }
                })
                .await
                .unwrap(),
                count
            );
            assert_eq!(calls.get(), count);
        }
    }
}
