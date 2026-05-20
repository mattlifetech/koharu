use anyhow::Result;
use koharu_types::{AppState, Document};

pub async fn read_doc(state: &AppState, index: usize) -> Result<Document> {
    let guard = state.read().await;
    guard
        .documents
        .get(index)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {index}"))
}

pub async fn update_doc(state: &AppState, index: usize, document: Document) -> Result<()> {
    let mut guard = state.write().await;
    let target = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {index}"))?;
    *target = document;
    Ok(())
}

pub async fn drop_intermediates(state: &AppState, index: usize) -> Result<()> {
    mutate_doc(state, index, |doc| {
        doc.segment = None;
        doc.brush_layer = None;
        // Save inpainted to disk then free it from RAM
        if let Some(inpainted) = doc.inpainted.take() {
            match doc.inpainted_path() {
                Some(path) => {
                    tracing::info!("Saving inpainted to {:?}", path);
                    if let Err(e) = inpainted.save_to_path(&path) {
                        tracing::warn!("Failed to save inpainted image to {:?}: {e}", path);
                    }
                }
                None => tracing::warn!("Could not derive inpainted path for doc {:?}", doc.path),
            }
        }
        Ok(())
    })
    .await
}

/// Saves the rendered image for `index` to `{image_dir}/Rendered/{name}.webp` then
/// clears it from the in-memory document to reduce RAM pressure during batch runs.
pub async fn persist_rendered(state: &AppState, index: usize) -> Result<()> {
    mutate_doc(state, index, |doc| {
        if let Some(rendered) = doc.rendered.take() {
            match doc.rendered_path() {
                Some(path) => {
                    tracing::info!("Saving rendered to {:?}", path);
                    if let Err(e) = rendered.save_to_path(&path) {
                        tracing::warn!("Failed to save rendered image to {:?}: {e}", path);
                        doc.rendered = Some(rendered);
                    }
                }
                None => {
                    tracing::warn!("Could not derive rendered path for doc {:?}", doc.path);
                    doc.rendered = Some(rendered);
                }
            }
        }
        Ok(())
    })
    .await
}

pub async fn mutate_doc<T, F>(state: &AppState, index: usize, mutator: F) -> Result<T>
where
    F: FnOnce(&mut Document) -> Result<T>,
{
    let mut guard = state.write().await;
    let target = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {index}"))?;
    mutator(target)
}

#[cfg(test)]
mod tests {
    use super::{mutate_doc, read_doc, update_doc};
    use koharu_types::{AppState, State};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn test_state() -> AppState {
        Arc::new(RwLock::new(State {
            documents: vec![Default::default()],
        }))
    }

    #[tokio::test]
    async fn read_update_mutate_doc_round_trip() {
        let state = test_state();

        let mut doc = read_doc(&state, 0).await.expect("doc should exist");
        doc.name = "before".to_string();
        update_doc(&state, 0, doc)
            .await
            .expect("update should work");

        mutate_doc(&state, 0, |doc| {
            doc.name = "after".to_string();
            Ok(())
        })
        .await
        .expect("mutation should work");

        let doc = read_doc(&state, 0).await.expect("doc should exist");
        assert_eq!(doc.name, "after");
    }

    #[tokio::test]
    async fn index_not_found_errors_are_stable() {
        let state = test_state();
        let err = read_doc(&state, 1)
            .await
            .expect_err("missing document should fail");
        assert_eq!(err.to_string(), "Document not found at index 1");

        let err = mutate_doc(&state, 1, |_| Ok(()))
            .await
            .expect_err("missing document should fail");
        assert_eq!(err.to_string(), "Document not found at index 1");
    }
}
