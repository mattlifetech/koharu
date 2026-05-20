use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ::image::{ColorType, DynamicImage, codecs::webp::WebPEncoder};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize, Serializer};

static CACHE_DIR: OnceCell<PathBuf> = OnceCell::new();

pub fn set_cache_dir(path: PathBuf) -> anyhow::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(&path)?;
    }
    CACHE_DIR
        .set(path)
        .map_err(|_| anyhow::anyhow!("Cache dir already set"))?;
    Ok(())
}

pub fn get_cache_dir() -> Option<&'static Path> {
    CACHE_DIR.get().map(|p| p.as_path())
}

#[derive(Debug, Clone)]
pub struct SerializableDynamicImage(pub Arc<DynamicImage>);

impl Default for SerializableDynamicImage {
    fn default() -> Self {
        Self(Arc::new(DynamicImage::ImageRgba8(image::RgbaImage::new(
            1, 1,
        ))))
    }
}

impl SerializableDynamicImage {
    pub fn id(&self) -> String {
        let rgba = self.0.to_rgba8();
        blake3::hash(&rgba).to_hex().to_string()
    }

    pub fn save_to_cache(&self) -> anyhow::Result<PathBuf> {
        let cache_dir = get_cache_dir().ok_or_else(|| anyhow::anyhow!("Cache dir not set"))?;
        let id = self.id();
        let path = cache_dir.join(format!("{}.webp", id));
        if !path.exists() {
            let rgba = self.0.to_rgba8();
            let (width, height) = rgba.dimensions();
            let mut buf = std::fs::File::create(&path)?;
            let enc = WebPEncoder::new_lossless(&mut buf);
            enc.encode(&rgba, width, height, ColorType::Rgba8.into())?;
        }
        Ok(path)
    }

    pub fn load_from_cache(id: &str) -> anyhow::Result<Self> {
        let cache_dir = get_cache_dir().ok_or_else(|| anyhow::anyhow!("Cache dir not set"))?;
        let path = cache_dir.join(format!("{}.webp", id));
        let bytes = std::fs::read(&path)?;
        let img = ::image::load_from_memory(&bytes)?;
        Ok(Self(Arc::new(img)))
    }

    pub fn save_to_path(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let rgba = self.0.to_rgba8();
        let (width, height) = rgba.dimensions();
        let mut file = std::fs::File::create(path)?;
        let enc = WebPEncoder::new_lossless(&mut file);
        enc.encode(&rgba, width, height, ColorType::Rgba8.into())?;
        Ok(())
    }

    pub fn load_from_path(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        let img = ::image::load_from_memory(&bytes)?;
        Ok(Self(Arc::new(img)))
    }
}

impl Serialize for SerializableDynamicImage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // When serializing for RPC, we still send the bytes
        let rgba = self.0.to_rgba8();
        let (width, height) = rgba.dimensions();
        let raw = rgba.into_raw();

        let mut buf = Vec::new();
        let enc = WebPEncoder::new_lossless(&mut buf);
        enc.encode(&raw, width, height, ColorType::Rgba8.into())
            .map_err(serde::ser::Error::custom)?;

        serde_bytes::serialize(&buf, serializer)
    }
}

impl<'de> Deserialize<'de> for SerializableDynamicImage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes: Vec<u8> = serde_bytes::deserialize(deserializer)?;
        let img = ::image::load_from_memory(&bytes).map_err(serde::de::Error::custom)?;
        Ok(SerializableDynamicImage(Arc::new(img)))
    }
}

impl Deref for SerializableDynamicImage {
    type Target = DynamicImage;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<DynamicImage> for SerializableDynamicImage {
    fn from(image: DynamicImage) -> Self {
        SerializableDynamicImage(Arc::new(image))
    }
}

impl From<Arc<DynamicImage>> for SerializableDynamicImage {
    fn from(image: Arc<DynamicImage>) -> Self {
        SerializableDynamicImage(image)
    }
}

impl From<SerializableDynamicImage> for DynamicImage {
    fn from(wrapper: SerializableDynamicImage) -> Self {
        Arc::try_unwrap(wrapper.0).unwrap_or_else(|arc| (*arc).clone())
    }
}

impl From<&SerializableDynamicImage> for DynamicImage {
    fn from(wrapper: &SerializableDynamicImage) -> Self {
        (*wrapper.0).clone()
    }
}
