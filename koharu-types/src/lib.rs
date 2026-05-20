pub mod commands;
pub mod events;
pub mod method;
pub mod parse;
pub mod views;

mod effect;
mod font;
mod image;

pub use commands::*;
pub use effect::TextShaderEffect;
pub use events::*;
pub use font::{FontPrediction, NamedFontPrediction, TextDirection};
pub use image::{SerializableDynamicImage, get_cache_dir, set_cache_dir};
pub use method::Method;

use std::{path::PathBuf, sync::Arc};

use ::image::GenericImageView;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextBlock {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
    pub line_polygons: Option<Vec<[[f32; 2]; 4]>>,
    pub source_direction: Option<TextDirection>,
    pub source_language: Option<String>,
    pub rotation_deg: Option<f32>,
    pub detected_font_size_px: Option<f32>,
    pub detector: Option<String>,
    pub text: Option<String>,
    pub translation: Option<String>,
    pub style: Option<TextStyle>,
    pub font_prediction: Option<FontPrediction>,
    #[serde(skip)]
    pub rendered: Option<SerializableDynamicImage>,
    #[serde(skip)]
    pub lock_layout_box: bool,
    #[serde(skip)]
    pub layout_seed_x: Option<f32>,
    #[serde(skip)]
    pub layout_seed_y: Option<f32>,
    #[serde(skip)]
    pub layout_seed_width: Option<f32>,
    #[serde(skip)]
    pub layout_seed_height: Option<f32>,
}

impl TextBlock {
    pub fn set_layout_seed(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.layout_seed_x = Some(x);
        self.layout_seed_y = Some(y);
        self.layout_seed_width = Some(width.max(1.0));
        self.layout_seed_height = Some(height.max(1.0));
    }

    pub fn seed_layout_box(&mut self) -> (f32, f32, f32, f32) {
        match (
            self.layout_seed_x,
            self.layout_seed_y,
            self.layout_seed_width,
            self.layout_seed_height,
        ) {
            (Some(x), Some(y), Some(width), Some(height))
                if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 =>
            {
                (x, y, width, height)
            }
            _ => {
                self.set_layout_seed(self.x, self.y, self.width, self.height);
                (self.x, self.y, self.width.max(1.0), self.height.max(1.0))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStrokeStyle {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_stroke_color")]
    pub color: [u8; 4],
    #[serde(default)]
    pub width_px: Option<f32>,
}

impl Default for TextStrokeStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            color: [255, 255, 255, 255],
            width_px: None,
        }
    }
}

const fn default_true() -> bool {
    true
}

const fn default_stroke_color() -> [u8; 4] {
    [255, 255, 255, 255]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStyle {
    pub font_families: Vec<String>,
    pub font_size: Option<f32>,
    pub color: [u8; 4],
    pub effect: Option<TextShaderEffect>,
    pub stroke: Option<TextStrokeStyle>,
    #[serde(default)]
    pub text_align: Option<TextAlign>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub image: SerializableDynamicImage,
    pub width: u32,
    pub height: u32,
    pub text_blocks: Vec<TextBlock>,
    pub segment: Option<SerializableDynamicImage>,
    pub inpainted: Option<SerializableDynamicImage>,
    pub rendered: Option<SerializableDynamicImage>,
    pub brush_layer: Option<SerializableDynamicImage>,
}

/// Subset of Document fields written to state.bin. Image data is excluded so
/// startup doesn't load hundreds of MB of pixel data into RAM; images are
/// reloaded from their original paths on load.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistentDocument {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub text_blocks: Vec<TextBlock>,
}

impl From<&Document> for PersistentDocument {
    fn from(doc: &Document) -> Self {
        Self {
            id: doc.id.clone(),
            path: doc.path.clone(),
            name: doc.name.clone(),
            width: doc.width,
            height: doc.height,
            text_blocks: doc.text_blocks.clone(),
        }
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PersistentState {
    pub documents: Vec<PersistentDocument>,
}

impl Document {
    fn output_base_dir(&self) -> Option<PathBuf> {
        let parent = self.path.parent()?;
        if parent.as_os_str().is_empty() {
            // Source path is relative/bare filename — fall back to cache dir
            get_cache_dir().map(|p| p.to_path_buf())
        } else {
            Some(parent.to_path_buf())
        }
    }

    pub fn rendered_path(&self) -> Option<PathBuf> {
        Some(
            self.output_base_dir()?
                .join("rendered")
                .join(format!("{}.webp", self.name)),
        )
    }

    pub fn inpainted_path(&self) -> Option<PathBuf> {
        Some(
            self.output_base_dir()?
                .join("inpainted")
                .join(format!("{}.webp", self.name)),
        )
    }

    fn legacy_rendered_path(&self) -> Option<PathBuf> {
        Some(
            self.output_base_dir()?
                .join("Rendered")
                .join(format!("{}.webp", self.name)),
        )
    }

    fn legacy_inpainted_path(&self) -> Option<PathBuf> {
        Some(
            self.output_base_dir()?
                .join("Inpainted")
                .join(format!("{}.webp", self.name)),
        )
    }

    /// Returns the rendered image from memory if present, otherwise loads it from disk.
    pub fn load_rendered(&self) -> Option<SerializableDynamicImage> {
        if let Some(ref r) = self.rendered {
            return Some(r.clone());
        }
        self.rendered_path()
            .and_then(|path| SerializableDynamicImage::load_from_path(&path).ok())
            .or_else(|| {
                self.legacy_rendered_path()
                    .and_then(|path| SerializableDynamicImage::load_from_path(&path).ok())
            })
    }

    /// Returns the inpainted image from memory if present, otherwise loads it from disk.
    pub fn load_inpainted(&self) -> Option<SerializableDynamicImage> {
        if let Some(ref r) = self.inpainted {
            return Some(r.clone());
        }
        self.inpainted_path()
            .and_then(|path| SerializableDynamicImage::load_from_path(&path).ok())
            .or_else(|| {
                self.legacy_inpainted_path()
                    .and_then(|path| SerializableDynamicImage::load_from_path(&path).ok())
            })
    }

    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let bytes = std::fs::read(&path)?;

        let documents = Self::from_bytes(path, bytes)?;
        documents
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No document found in file"))
    }

    pub fn from_bytes(path: impl Into<PathBuf>, bytes: Vec<u8>) -> anyhow::Result<Vec<Self>> {
        let path = path.into();
        Ok(vec![Self::image(path, bytes)?])
    }

    fn image(path: PathBuf, bytes: Vec<u8>) -> anyhow::Result<Self> {
        let img = ::image::load_from_memory(&bytes)?;
        let (width, height) = img.dimensions();
        let id = blake3::hash(&bytes).to_hex().to_string();
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let serializable_img = SerializableDynamicImage(Arc::new(img));

        Ok(Document {
            id,
            path,
            name,
            image: serializable_img,
            width,
            height,
            ..Default::default()
        })
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub documents: Vec<Document>,
}

impl State {
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
        let persistent = PersistentState {
            documents: self
                .documents
                .iter()
                .map(PersistentDocument::from)
                .collect(),
        };
        let bytes = postcard::to_stdvec(&persistent)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        let persistent: PersistentState = postcard::from_bytes(&bytes)?;
        let documents: Vec<Document> = persistent
            .documents
            .into_iter()
            .filter_map(|p| {
                Document::open(p.path.clone()).ok().map(|loaded| Document {
                    text_blocks: p.text_blocks,
                    ..loaded
                })
            })
            .collect();
        Ok(Self { documents })
    }
}

pub type AppState = Arc<RwLock<State>>;

#[cfg(test)]
mod tests {
    use super::TextBlock;

    #[test]
    fn seed_layout_box_stays_stable_until_explicit_reset() {
        let mut block = TextBlock {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
            ..Default::default()
        };

        let first = block.seed_layout_box();
        assert_eq!(first, (10.0, 20.0, 30.0, 40.0));

        block.x = 100.0;
        block.y = 200.0;
        block.width = 300.0;
        block.height = 400.0;

        let second = block.seed_layout_box();
        assert_eq!(second, first);

        block.set_layout_seed(block.x, block.y, block.width, block.height);
        let third = block.seed_layout_box();
        assert_eq!(third, (100.0, 200.0, 300.0, 400.0));
    }
}
