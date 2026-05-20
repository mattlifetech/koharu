use std::path::{Path, PathBuf};

use image::ImageFormat;
use koharu_types::commands::{
    DeviceInfo, FileResult, IndexPayload, NativeOpenDocumentsPayload, OpenDocumentsPayload,
    OpenExternalPayload, ThumbnailResult,
};
use rfd::FileDialog;

use crate::{AppResources, state_tx};

use super::utils::{encode_image, encode_image_with_quality, load_documents, mime_from_ext};

fn next_available_path(output_dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut candidate = output_dir.join(format!("{stem}.{ext}"));
    let mut suffix = 2usize;
    while candidate.exists() {
        candidate = output_dir.join(format!("{stem}_{suffix}.{ext}"));
        suffix += 1;
    }
    candidate
}

async fn pick_output_dir() -> anyhow::Result<Option<PathBuf>> {
    Ok(tokio::task::spawn_blocking(|| FileDialog::new().pick_folder()).await?)
}

fn supported_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp"
            )
        })
        .unwrap_or(false)
}

async fn pick_native_document_paths(folder: bool) -> anyhow::Result<Vec<PathBuf>> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<PathBuf>> {
        if folder {
            let Some(dir) = FileDialog::new().pick_folder() else {
                return Ok(Vec::new());
            };
            let mut paths = std::fs::read_dir(dir)?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.is_file() && supported_image_path(path))
                .collect::<Vec<_>>();
            paths.sort_by(|a, b| {
                natord::compare(
                    &a.file_name().unwrap_or_default().to_string_lossy(),
                    &b.file_name().unwrap_or_default().to_string_lossy(),
                )
            });
            Ok(paths)
        } else {
            Ok(FileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                .pick_files()
                .unwrap_or_default())
        }
    })
    .await?
}

fn load_documents_from_paths(paths: Vec<PathBuf>) -> anyhow::Result<Vec<koharu_types::Document>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut documents = Vec::new();
    for path in paths {
        if !supported_image_path(&path) {
            continue;
        }
        match koharu_types::Document::open(path) {
            Ok(doc) => documents.push(doc),
            Err(err) => tracing::warn!(?err, "Failed to parse document from native path"),
        }
    }
    documents.sort_by(|a, b| natord::compare(&a.name, &b.name));
    Ok(documents)
}

fn document_ext(document: &koharu_types::Document) -> String {
    document
        .path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("jpg")
        .to_string()
}

fn export_documents_matching(
    documents: &[koharu_types::Document],
    output_dir: &Path,
    suffix: &str,
    missing_error: &str,
    image: impl Fn(&koharu_types::Document) -> Option<koharu_types::SerializableDynamicImage>,
) -> anyhow::Result<usize> {
    let mut exported = 0usize;

    for document in documents {
        let Some(image) = image(document) else {
            continue;
        };

        let ext = document_ext(document);
        let output_path =
            next_available_path(output_dir, &format!("{}_{}", document.name, suffix), &ext);
        let bytes = encode_image(&image, &ext)?;
        std::fs::write(&output_path, bytes)?;
        exported += 1;
    }

    anyhow::ensure!(exported > 0, "{missing_error}");
    Ok(exported)
}

async fn save_state(state: &AppResources) -> anyhow::Result<()> {
    let guard = state.state.read().await;
    guard.save(&state.state_path)?;
    Ok(())
}

pub async fn app_version(state: AppResources) -> anyhow::Result<String> {
    Ok(state.version.to_string())
}

pub async fn device(state: AppResources) -> anyhow::Result<DeviceInfo> {
    Ok(DeviceInfo {
        ml_device: match state.device {
            koharu_ml::Device::Cpu => "CPU".to_string(),
            koharu_ml::Device::Cuda(_) => "CUDA".to_string(),
            koharu_ml::Device::Metal(_) => "Metal".to_string(),
        },
    })
}

pub async fn open_external(
    _state: AppResources,
    payload: OpenExternalPayload,
) -> anyhow::Result<()> {
    open::that(&payload.url)?;
    Ok(())
}

pub async fn get_documents(state: AppResources) -> anyhow::Result<usize> {
    let guard = state.state.read().await;
    Ok(guard.documents.len())
}

pub async fn get_document_names(state: AppResources) -> anyhow::Result<Vec<String>> {
    let guard = state.state.read().await;
    Ok(guard.documents.iter().map(|d| d.name.clone()).collect())
}

pub async fn clear_documents(state: AppResources) -> anyhow::Result<()> {
    let mut guard = state.state.write().await;
    guard.documents.clear();
    drop(guard);
    save_state(&state).await?;
    Ok(())
}

pub async fn get_document(
    state: AppResources,
    payload: IndexPayload,
) -> anyhow::Result<koharu_types::Document> {
    let mut doc = state_tx::read_doc(&state.state, payload.index).await?;
    // Completed layers may have been persisted to disk to free RAM; load them for this response.
    if doc.inpainted.is_none() {
        doc.inpainted = doc.load_inpainted();
    }
    if doc.rendered.is_none() {
        doc.rendered = doc.load_rendered();
    }
    Ok(doc)
}

pub async fn get_thumbnail(
    state: AppResources,
    payload: IndexPayload,
) -> anyhow::Result<ThumbnailResult> {
    let doc = state_tx::read_doc(&state.state, payload.index).await?;

    let rendered_from_disk = if doc.rendered.is_none() {
        doc.load_rendered()
    } else {
        None
    };
    let source = doc
        .rendered
        .as_ref()
        .or(rendered_from_disk.as_ref())
        .unwrap_or(&doc.image);
    let thumbnail = source.thumbnail(200, 200);

    let mut buf = std::io::Cursor::new(Vec::new());
    thumbnail.write_to(&mut buf, ImageFormat::WebP)?;

    Ok(ThumbnailResult {
        data: buf.into_inner(),
        content_type: "image/webp".to_string(),
    })
}

pub async fn get_rendered_image(
    state: AppResources,
    payload: IndexPayload,
) -> anyhow::Result<ThumbnailResult> {
    let doc = state_tx::read_doc(&state.state, payload.index).await?;

    let rendered_from_disk = if doc.rendered.is_none() {
        doc.load_rendered()
    } else {
        None
    };
    let rendered_ref = doc.rendered.as_ref().or(rendered_from_disk.as_ref());
    let mut source = (**rendered_ref.unwrap_or(&doc.image)).clone();

    // Perform resizing if max_size is provided and image is larger
    if let Some(max_size) = payload.max_size {
        let (w, h) = (source.width(), source.height());
        let shortest = w.min(h);
        if shortest > max_size {
            let scale = max_size as f32 / shortest as f32;
            let nw = (w as f32 * scale).round() as u32;
            let nh = (h as f32 * scale).round() as u32;
            source = source.resize(nw, nh, image::imageops::FilterType::Lanczos3);
        }
    }

    let serializable_source = source.into();

    let ext = payload.format.unwrap_or_else(|| document_ext(&doc));
    let bytes = if let Some(q) = payload.quality {
        encode_image_with_quality(&serializable_source, &ext, q)?
    } else {
        encode_image(&serializable_source, &ext)?
    };
    let content_type = mime_from_ext(&ext).to_string();

    Ok(ThumbnailResult {
        data: bytes,
        content_type,
    })
}

pub async fn open_documents(
    state: AppResources,
    payload: OpenDocumentsPayload,
) -> anyhow::Result<usize> {
    let inputs: Vec<(PathBuf, Vec<u8>)> = payload
        .files
        .into_iter()
        .map(|f| (PathBuf::from(f.name), f.data))
        .collect();

    if inputs.is_empty() {
        anyhow::bail!("No files uploaded");
    }

    let docs = load_documents(inputs)?;
    let count = docs.len();
    let mut guard = state.state.write().await;
    guard.documents = docs;
    drop(guard);
    save_state(&state).await?;
    Ok(count)
}

pub async fn add_documents(
    state: AppResources,
    payload: OpenDocumentsPayload,
) -> anyhow::Result<usize> {
    let inputs: Vec<(PathBuf, Vec<u8>)> = payload
        .files
        .into_iter()
        .map(|f| (PathBuf::from(f.name), f.data))
        .collect();

    if inputs.is_empty() {
        anyhow::bail!("No files uploaded");
    }

    let docs = load_documents(inputs)?;
    let mut guard = state.state.write().await;
    guard.documents.extend(docs);
    guard
        .documents
        .sort_by(|a, b| natord::compare(&a.name, &b.name));
    let count = guard.documents.len();
    drop(guard);
    save_state(&state).await?;
    Ok(count)
}

pub async fn native_open_documents(
    state: AppResources,
    payload: NativeOpenDocumentsPayload,
) -> anyhow::Result<usize> {
    let paths = pick_native_document_paths(payload.folder).await?;
    let docs = load_documents_from_paths(paths)?;
    let count = docs.len();
    if count == 0 {
        return Ok(0);
    }

    let mut guard = state.state.write().await;
    guard.documents = docs;
    drop(guard);
    save_state(&state).await?;
    Ok(count)
}

pub async fn native_add_documents(
    state: AppResources,
    payload: NativeOpenDocumentsPayload,
) -> anyhow::Result<usize> {
    let paths = pick_native_document_paths(payload.folder).await?;
    let docs = load_documents_from_paths(paths)?;
    if docs.is_empty() {
        let guard = state.state.read().await;
        return Ok(guard.documents.len());
    }

    let mut guard = state.state.write().await;
    guard.documents.extend(docs);
    guard
        .documents
        .sort_by(|a, b| natord::compare(&a.name, &b.name));
    let count = guard.documents.len();
    drop(guard);
    save_state(&state).await?;
    Ok(count)
}

pub async fn export_document(
    state: AppResources,
    payload: IndexPayload,
) -> anyhow::Result<FileResult> {
    let document = state_tx::read_doc(&state.state, payload.index).await?;

    let ext = document
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg")
        .to_string();

    let rendered = document
        .load_rendered()
        .ok_or_else(|| anyhow::anyhow!("No rendered image found"))?;

    let bytes = encode_image(&rendered, &ext)?;
    let filename = format!("{}_koharu.{}", document.name, ext);
    let content_type = mime_from_ext(&ext).to_string();

    Ok(FileResult {
        filename,
        data: bytes,
        content_type,
    })
}

pub async fn export_all_inpainted(state: AppResources) -> anyhow::Result<usize> {
    let Some(output_dir) = pick_output_dir().await? else {
        return Ok(0);
    };

    let guard = state.state.read().await;
    export_documents_matching(
        &guard.documents,
        &output_dir,
        "inpainted",
        "No inpainted images found to export",
        |document| document.load_inpainted(),
    )
}

pub async fn export_all_rendered(state: AppResources) -> anyhow::Result<usize> {
    let Some(output_dir) = pick_output_dir().await? else {
        return Ok(0);
    };

    let guard = state.state.read().await;
    export_documents_matching(
        &guard.documents,
        &output_dir,
        "rendered",
        "No rendered images found to export",
        |document| document.load_rendered(),
    )
}
